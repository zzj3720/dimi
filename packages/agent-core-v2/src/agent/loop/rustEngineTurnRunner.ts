/**
 * `rustEngineTurnRunner` — the default turn runner: one turn through the Rust
 * engine (`RustTurnSession` napi socket) instead of the TS loop.
 *
 * Enabled by default; the CLI `--legacy` flag sets `DIMI_LEGACY=1` to keep
 * the TS loop. The runner:
 *  1. records `turn.prompt` (turn clock) and appends the user message;
 *  2. assembles the LLM messages from the context (context assembly stays
 *     on the TS side until slice 3);
 *  3. runs `RustTurnSession` (aimux-backed LLM in production, scripted
 *     segments under test);
 *  4. streams the engine events onto the event bus as they happen (the
 *     bridge pushes each event through a per-event callback while the turn
 *     is in flight) — the transcript projection layer (`coreEventMap`)
 *     folds them into wire ops exactly as it does for the TS loop — and
 *     mirrors them into the context (`step.begin` / `content.part` /
 *     `tool.call` / `tool.result` / `step.end` + the assistant message) so
 *     the next turn's history is intact.
 *
 * Slice-1 scope: single turn, no queue/cancellation/undo (those land with
 * later slices). The runner is a parallel path — `loopService` is untouched.
 */

import { randomUUID } from "node:crypto";

import { RustTurnSession } from "@dimi-agent/dimi-native";

import { escapeXml } from "#/_base/utils/xml-escape";
import { IAgentContextMemoryService } from "#/agent/contextMemory/contextMemory";
import type {
  ContextMessage,
  PromptOrigin,
  SystemTriggerOrigin,
  TaskOrigin,
} from "#/agent/contextMemory/types";
import { IAgentLLMRequesterService } from "#/agent/llmRequester/llmRequester";
import { IAgentPermissionModeService } from "#/agent/permissionMode/permissionMode";
import { IAgentToolRegistryService } from "#/agent/toolRegistry/toolRegistry";
import { IAgentToolPolicyService } from "#/agent/toolPolicy/toolPolicy";
import { IAgentPermissionRulesService } from "#/agent/permissionRules/permissionRules";
import { EngineTaskAdapter } from "#/agent/loop/engineTaskAdapter";
import { renderNotificationXml } from "#/agent/task/notificationXml";
import { taskStarted, taskTerminated } from "#/agent/task/taskOps";
import { IAgentTaskService } from "#/agent/task/task";
import type { AgentTaskInfo, AgentTaskSettlement, AgentTaskStatus } from "#/agent/task/types";
import { DEFAULT_KILL_GRACE_MS, resolveAgentTaskConfig } from "#/agent/task/configSection";
import { cancelTurn, promptTurn, steerTurn, TurnModel } from "#/agent/loop/turnOps";
import { IEventBus } from "#/app/event/eventBus";
import { IConfigService } from "#/app/config/config";
import { ILogService } from "#/_base/log/log";
import { isPlainRecord } from "#/_base/utils/canonical-args";
import { toDimiErrorPayload } from "#/errors";
import { IExternalHooksRunnerService } from "#/app/externalHooksRunner/externalHooksRunner";
import { IAgentToolResultTruncationService } from "#/agent/toolResultTruncation/toolResultTruncation";
import { ISessionApprovalService, type ApprovalResponse } from "#/session/approval/approval";
import { IAgentUsageService } from "#/agent/usage/usage";
import { IAgentProfileService } from "#/agent/profile/profile";
import { IAgentScopeContext } from "#/agent/scopeContext/scopeContext";
import { IModelCatalog } from "#/app/modelCatalog/catalog";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import { ISessionContext } from "#/session/sessionContext/sessionContext";
import { ISessionMetadata } from "#/session/sessionMetadata/sessionMetadata";
import type { ContentPart } from "#/llmProtocol/message";
import { emptyUsage } from "#/llmProtocol/usage";
import { IWireService } from "#/wire/wire";
import { buildCompactionSummaryText } from "#/agent/contextMemory/compactionHandoff";
import {
  fullCompactionBegin,
  fullCompactionComplete,
} from "#/agent/fullCompaction/compactionOps";
import {
  ALL_DONE_TOOL_NAME,
  COMPLETION_REVIEW_MIN_STEPS,
  COMPLETION_REVIEW_REMINDER,
} from "#/agent/completion/completion";

// P1-2 (review): the engine's WaitFor waits for a background SUBAGENT task by
// `agent_id` — a different tool from the TS `waitForTool.ts`, whose schema
// (`reason`/`timeout_seconds`, no `agent_id`) describes TS user-wait /
// notification-wake semantics (a parking gap the engine does not implement).
// The def the model sees must match the engine implementation: `agent_id`
// required + `timeout_seconds` optional + an honest description of the
// boundary (subagent wait only, not user wait). An unknown `agent_id` now
// fails fast on the engine side, so the model gets a real error instead of a
// guaranteed full-timeout blind wait.
const WAIT_FOR_NATIVE_DESCRIPTION = [
  "Wait for a background subagent task to finish (or time out).",
  "Pass the `agent_id` of a subagent launched by the Agent tool (from its launch output).",
  "The call blocks until that subagent completes, fails, or the timeout expires, then returns its final status and output.",
  "This waits on a specific subagent task, NOT on the user: user-wait (waking on user notifications) is not implemented.",
  "Prefer AgentOutput for a quick status check; use WaitFor when the turn should pause until the subagent finishes.",
].join(" ");

/** Engine-native tools that execute inside Rust (not through the TS
 *  toolExecutor): the PreToolUse gate and the external-tool registration
 *  distinguish them from TS-side tools. */
const ENGINE_NATIVE_TOOLS = new Set(['Bash', 'Agent', 'AgentOutput', 'WaitFor']);

const WAIT_FOR_NATIVE_PARAMETERS = {
  type: "object",
  properties: {
    agent_id: {
      type: "string",
      description:
        "Agent id of the running subagent task to wait for, as returned by the Agent tool launch output.",
    },
    timeout_seconds: {
      type: "integer",
      minimum: 1,
      maximum: 1800,
      description:
        "How long to wait for the subagent before giving up. Defaults to 60; maximum 1800.",
    },
  },
  required: ["agent_id"],
  additionalProperties: false,
};

// P2-4 (review): TS `AgentSystemReminderService.appendSystemReminder` parity —
// idempotently wrap a reminder in `<system-reminder>` markers. The engine
// already wraps the completion-review reminder it injects (and mirrors on the
// `completion.review.injected` event), so an already-wrapped text is left
// untouched; a bare one is wrapped before it reaches the context.
function wrapSystemReminder(text: string): string {
  const trimmed = text.trim();
  if (trimmed.startsWith("<system-reminder>") && trimmed.endsWith("</system-reminder>")) {
    return trimmed;
  }
  return `<system-reminder>\n${trimmed}\n</system-reminder>`;
}

import { createDecorator, IInstantiationService } from "#/_base/di/instantiation";
import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";

export const IRustEngineTurnRunner = createDecorator<IRustEngineTurnRunner>(
  "rustEngineTurnRunner",
);

export interface IRustEngineTurnRunner {
  /**
   * Run one turn through the Rust engine. Resolves once the turn is
   * launched (TS `launched` parity): `undefined` when the turn is queued
   * behind an active one (TS `state === 'pending'`).
   */
  runTurn(input: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin }): Promise<{
    readonly turnId: number;
  } | undefined>;
  /**
   * Steer the currently running Rust-engine turn (mid-turn injection).
   * Returns `false` when no turn is active — the caller then starts a new
   * turn with the input instead.
   */
  steer(input: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin }): boolean;
  /**
   * Cancel the active turn (RPC cancel parity); with `turnId`, also cancels
   * a queued turn. Returns whether something was cancelled.
   */
  cancel(turnId?: number): boolean;
}

/**
 * The Rust engine is the default runtime; the CLI `--legacy` flag sets
 * `DIMI_LEGACY=1` to keep the TS loop (and the node-local OS backends).
 */
function rustEngineEnabled(): boolean {
  return process.env["DIMI_LEGACY"] !== "1";
}

/** Render an engine event/tool value as text (strings pass through). */
function toText(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value) ?? "";
}

/**
 * Serialize a TS `PromptOrigin` into the wire `TurnOrigin` JSON shape the
 * engine deserializes (`{ kind: 'user' }` / `{ kind: 'task', taskId }` /
 * … — see `dimi-wire` `model.rs`). Unknown kinds fall back to a plain user
 * origin (the wire default).
 */
function toEngineTurnOrigin(origin: PromptOrigin): Record<string, unknown> {
  switch (origin.kind) {
    case "task":
      return { kind: "task", taskId: origin.taskId };
    case "cron_job":
      return { kind: "cron", taskId: origin.jobId };
    case "cron_missed":
      return { kind: "cron" };
    case "hook_result":
      return { kind: "hook" };
    case "compaction_summary":
      return { kind: "compaction" };
    default:
      return { kind: "user" };
  }
}

/**
 * TS `isDisplayablePromptOrigin` parity: only user-origin (and user-slash
 * skill/plugin) turns expose their prompt on `turn.started`; task/cron/hook
 * steering text must never leak into the transcript.
 */
function isDisplayablePromptOrigin(origin: PromptOrigin): boolean {
  if (origin.kind === "user") return true;
  return (
    (origin.kind === "skill_activation" || origin.kind === "plugin_command") &&
    origin.trigger === "user-slash"
  );
}

/**
 * Build a DimiErrorPayload-shaped error from the engine's `{ message, code }`
 * payload (TS `toDimiErrorPayload` parity): always carries `name` (the error
 * class name, mapped from the code when the engine omitted it), `retryable`
 * and `message`.
 */
function buildErrorPayload(rawError: Record<string, unknown>): Record<string, unknown> {
  const error = { ...rawError };
  if (error["name"] === undefined || error["name"] === null) {
    error["name"] = error["code"] === "PROVIDER_FILTERED" ? "ProviderFilteredError" : "Error";
  }
  if (error["retryable"] === undefined) {
    error["retryable"] = false;
  }
  return error;
}

/** Optional numeric engine field (pid / exitCode). */
function toOptionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/** `task.terminated` output tail cap (taskService TERMINAL_OUTPUT_TAIL_BYTES). */
const TERMINAL_OUTPUT_TAIL_CHARS = 4 * 1024;

/** Notification output preview cap (taskService NOTIFICATION_FALLBACK_PREVIEW_BYTES). */
const NOTIFICATION_OUTPUT_PREVIEW_BYTES = 3_000;

/** Mirrors the task domain's `buildAgentTaskNotificationBody` (minus the
 *  output-file block — the engine carries the tail inline). */
function buildTaskNotificationBody(task: {
  readonly taskId: string;
  readonly agentId: string;
  readonly kind: string;
  readonly status: string;
  readonly description: string;
  readonly error?: string;
}): string {
  const baseLine =
    task.status === "timed_out"
      ? `${task.description} timed out.`
      : task.status === "killed" && task.error !== undefined
        ? `${task.description} was stopped. Reason: ${task.error}`
        : task.status === "failed" && task.error !== undefined
          ? `${task.description} failed. Reason: ${task.error}`
          : `${task.description} ${task.status}.`;
  if (task.kind !== "agent" || task.status === "completed") return baseLine;
  const recovery = [
    "",
    `To recover or continue this subagent, call Agent(resume="${task.agentId}", prompt="Pick up where you left off; redo the last tool call if its result was never observed.").`,
    `Use agent_id ("${task.agentId}"), NOT source_id / task_id ("${task.taskId}") — the two look alike but only agent_id is accepted by the resume parameter.`,
    "Add run_in_background=true to keep it backgrounded, or omit it to take the result inline in the current turn.",
    "The subagent retains its full prior context across the restart, but any in-flight tool call lost its result and may need to be redone.",
  ].join("\n");
  return `${baseLine}${recovery}`;
}

/** Mirrors the task domain's `renderOutputPreviewBlock` (tail inline): the
 *  engine's accumulated output is the "currently buffered" output, so the
 *  preview is the last `NOTIFICATION_OUTPUT_PREVIEW_BYTES` bytes with the
 *  same heading / `truncated` / `bytes` semantics the TS service renders. */
function renderOutputPreviewBlock(output: string): string {
  const fullBytes = Buffer.byteLength(output, "utf-8");
  const previewBytes = Math.min(NOTIFICATION_OUTPUT_PREVIEW_BYTES, fullBytes);
  const truncated = fullBytes > previewBytes;
  const preview =
    previewBytes > 0
      ? Buffer.from(output, "utf-8")
          .subarray(fullBytes - previewBytes)
          .toString("utf-8")
      : "";
  return [
    `<output-preview bytes="${String(previewBytes)}" total_bytes="${String(fullBytes)}" truncated="${String(truncated)}">`,
    truncated
      ? `Showing the last ${String(previewBytes)} bytes. No persisted full output is available.`
      : "No persisted full output is available; this preview is the currently buffered task output.",
    escapeXml(preview),
    "</output-preview>",
  ].join("\n");
}

/** Resolve once the signal aborts (used to race an approval wait). */
function abortOnSignal(signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    signal.addEventListener("abort", () => { resolve(); }, { once: true });
  });
}

/** Turn an engine event batch into bus events + context records. */
export class RustEngineTurnRunner implements IRustEngineTurnRunner {
  declare readonly _serviceBrand: undefined;

  /** The in-flight Rust session, while a turn is running (steer/cancel target). */
  private activeSession: RustTurnSession | undefined;

  /** FIFO queue of turns waiting behind the running one (TS loop queue
   *  parity): prompts during an active turn wait for it to finish. */
  private readonly queued: Array<{
    readonly turnId: number;
    readonly payload: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin };
    cancelled: boolean;
  }> = [];

  /** Whether a turn is currently executing (queue gate). */
  private turnRunning = false;

  /** turnId of the executing turn (cancel validation). */
  private executingTurnId: number | undefined;

  /** Aborts the in-flight approval wait on cancel. */
  private approvalAbort: AbortController | undefined;

  /**
   * Rust-side task registry mirror: taskId → launch facts (description /
   * startedAt / pid) recorded from `task.started`, used to build the
   * terminal `task.terminated` info from `task.settled`. Entries are removed
   * once the settle is handled (a task settles exactly once), so the map
   * stays bounded by the number of tasks in flight.
   */
  private readonly taskInfos = new Map<
    string,
    { readonly description: string; readonly startedAt: number; readonly pid?: number }
  >();

  /** Notification dedupe (TS `deliveredNotificationKeys` parity): guards a
   *  single settle from being delivered twice. Keys are removed after the
   *  delivery completes — a settled task never settles again — so the set
   *  stays bounded; the mid-turn+idle double-delivery guard is the
   *  single-call structure of `deliverTaskNotification`, not this set. */
  private readonly deliveredNotificationKeys = new Set<string>();

  /** Registry entries for engine background tasks (TaskList/TaskStop parity):
   *  taskId → adapter registered with `IAgentTaskService`. Removed once the
   *  task settles (a task settles exactly once), so the map stays bounded. */
  private readonly engineTaskAdapters = new Map<string, EngineTaskAdapter>();

  /** Owning RustTurnSession per engine task id (F1): recorded at
   *  `task.started` from the session whose EventSink delivered the event.
   *  Tasks launched from background workers (a subagent spawning a
   *  sub-subagent, or a subagent launching a backgrounded bash) emit their
   *  start AFTER the launching turn ended, when `activeSession` is undefined
   *  or a newer turn's session — and the engine's per-task cancel signal
   *  lives on the OWNING session's task map (`RustTurnSession::new` creates
   *  a fresh map per turn), so TaskStop must cancel through this session.
   *  Removed once the task settles, so the map stays bounded. */
  private readonly taskSessions = new Map<string, RustTurnSession>();

  /** Every subagent agent id this runner has handed out (the engine's
   *  `agent-<n>` ids across turns): the next-turn seed must continue past
   *  them so ids stay monotonic within the session (TS
   *  `nextAvailableAgentId` parity). */
  private readonly engineAgentIds = new Set<string>();

  /** Every RustTurnSession this runner created. Held until the agent scope
   *  is disposed so a session's EventSink stays open while its background
   *  tasks are still settling (a subagent launched in turn 1 must be able to
   *  notify after turn 1 ends), and so teardown can close them all at once
   *  (TS `taskService.dispose` parity). */
  private readonly sessions = new Set<RustTurnSession>();

  /** Whether the agent scope was disposed: late engine events (background
   *  task settles racing the teardown) are ignored instead of dispatching
   *  wire ops on a disposed wire / appending to a disposed context. */
  private disposed = false;

  constructor(
    @IAgentContextMemoryService private readonly context: IAgentContextMemoryService,
    @IEventBus private readonly eventBus: IEventBus,
    @IWireService private readonly wire: IWireService,
    @IConfigService private readonly config: IConfigService,
    @IAgentLLMRequesterService private readonly llmRequester: IAgentLLMRequesterService,
    @IAgentPermissionModeService private readonly modeService: IAgentPermissionModeService,
    @IAgentPermissionRulesService private readonly rulesService: IAgentPermissionRulesService,
    @IAgentToolRegistryService private readonly toolRegistry: IAgentToolRegistryService,
    @IAgentUsageService private readonly usageService: IAgentUsageService,
    @IAgentProfileService private readonly profile: IAgentProfileService,
    @IAgentScopeContext private readonly scopeContext: IAgentScopeContext,
    @ISessionContext private readonly sessionContext: ISessionContext,
    @ISessionMetadata private readonly sessionMetadata: ISessionMetadata,
    @IAgentTaskService private readonly tasks: IAgentTaskService,
    @IModelCatalog private readonly modelCatalog: IModelCatalog,
    @IProviderRuntime private readonly providerRuntime: IProviderRuntime,
    @IInstantiationService private readonly instantiation: IInstantiationService,
    @IAgentToolPolicyService private readonly toolPolicy: IAgentToolPolicyService,
    @ILogService private readonly log: ILogService,
  ) {
    // Agent-scoped subagent registry id: every RustTurnSession of this agent
    // shares one registry (subagent tasks + steering queues), and no other
    // agent can see them. Released on dispose.
    this.registryId = randomUUID();
  }

  /** Agent-scoped Rust subagent registry id (see constructor). */
  private readonly registryId: string;

  /** TS `stopHookContinuationUsed` parity: the Stop hook fires at most once
   * after a continuation was delivered. */
  private stopHookContinuationUsed = false;

  static isEnabled(): boolean {
    return rustEngineEnabled();
  }

  /**
   * Steer the running turn. Mirrors the TS steer path (`steerTurn` op +
   * user message in the context) and forwards the text into the engine's
   * steer queue, where it is drained into the next LLM request. Returns
   * `false` when there is no steerable turn: either no session is active
   * (the caller starts a new turn) or the engine's turn already finished
   * (a steer racing the teardown — between the engine's final steer check
   * and this runner clearing `activeSession` — would land in a queue that
   * is never drained again; the caller's `runTurn` fallback queues it as
   * the next turn instead, so it is never lost).
   */
  steer(payload: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin }): boolean {
    if (this.disposed) return false;
    const session = this.activeSession;
    if (session === undefined) return false;
    const text = payload.input
      .filter((part) => part.type === "text")
      .map((part) => (part as { text?: string }).text ?? "")
      .join("");
    if (text.length === 0) return false;
    // Ask the engine first: when its turn already ended it refuses the
    // steer (returns false) and the caller falls back to `runTurn` — the
    // wire op / context append must not run in that case or the fallback
    // would record the user message twice.
    if (!session.steer(text)) return false;
    this.wire.dispatch(steerTurn({ input: [...payload.input], origin: payload.origin }));
    this.context.append({
      role: "user",
      content: [...payload.input],
      toolCalls: [],
      origin: payload.origin,
      id: randomUUID(),
    });
    return true;
  }

  /**
   * Run one turn through the Rust engine. The turn clock advances at enqueue
   * time (TS parity); the turn itself either starts immediately (resolves
   * with its id) or waits behind the running turn (resolves `undefined`,
   * like TS `state === 'pending'`).
   */
  async runTurn(payload: {
    readonly input: readonly ContentPart[];
    readonly origin: PromptOrigin;
  }): Promise<{ readonly turnId: number } | undefined> {
    if (this.disposed) return undefined;
    // 1. Turn clock + user message (mirrors loopService.startTurn).
    this.wire.dispatch(promptTurn({ input: [...payload.input], origin: payload.origin }));
    // Turn ids are 0-based (TS parity): the TS loop reserves `nextTurnId - 1`
    // — `reserveTurnId` reads the wire clock *before* the `turn.prompt` op is
    // dispatched at start, so the first turn is 0 (verified by loop.test.ts
    // telemetry `turn_id: 0` and the TurnModel clock reducer, which only stays
    // consistent when engine event turn ids are 0-based). Do not "fix" this to
    // 1-based: it diverges from TS and double-advances the wire clock.
    const turnId = this.wire.getModel(TurnModel).nextTurnId - 1;

    const userMessage: ContextMessage = {
      role: "user",
      content: [...payload.input],
      toolCalls: [],
      origin: payload.origin,
      id: randomUUID(),
    };
    this.context.append(userMessage);

    if (this.turnRunning) {
      this.queued.push({ turnId, payload, cancelled: false });
      return undefined;
    }
    this.startQueuedTurn({ turnId, payload, cancelled: false });
    return { turnId };
  }

  /** Cancel the active turn; with `turnId`, also cancels a queued turn. */
  cancel(turnId?: number): boolean {
    const session = this.activeSession;
    if (
      session !== undefined &&
      (turnId === undefined || turnId === this.executingTurnId)
    ) {
      // Interrupt an in-flight approval wait first (it blocks the runner's
      // resume loop), then the engine's own cancellation.
      this.approvalAbort?.abort();
      session.cancel();
      return true;
    }
    let cancelled = false;
    for (const entry of this.queued) {
      if (entry.turnId === turnId) {
        entry.cancelled = true;
        this.wire.dispatch(cancelTurn({ turnId }));
        cancelled = true;
      }
    }
    return cancelled;
  }

  /**
   * Agent-scope teardown (TS `taskService.dispose` parity): every Rust
   * session is closed — its EventSink stops forwarding, the in-flight turn
   * is cancelled, and background workers/pollers observe the closed flag and
   * kill their processes — and late engine events are ignored instead of
   * dispatching wire ops on a disposed wire. The DI container invokes this
   * when the agent scope is disposed.
   */
  dispose(): void {
    this.disposed = true;
    this.approvalAbort?.abort();
    for (const session of this.sessions) {
      session.close();
    }
    this.sessions.clear();
    this.taskSessions.clear();
    this.engineTaskAdapters.clear();
    // Release the agent-scoped subagent registry (tasks + steering queues).
    RustTurnSession.dropTaskRegistry(this.registryId);
  }

  private startQueuedTurn(entry: {
    readonly turnId: number;
    readonly payload: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin };
    cancelled: boolean;
  }): void {
    this.turnRunning = true;
    this.executingTurnId = entry.turnId;
    void this.runTurnNow(entry.turnId, entry.payload.origin)
      .catch(() => undefined)
      .finally(() => {
        this.turnRunning = false;
        this.executingTurnId = undefined;
        if (this.disposed) {
          // Teardown raced the queue: drop the queued turns instead of
          // starting them against a closed session.
          this.queued.length = 0;
          return;
        }
        const next = this.queued.find((queued) => !queued.cancelled);
        if (next === undefined) return;
        this.queued.splice(this.queued.indexOf(next), 1);
        this.startQueuedTurn(next);
      });
  }

  private async runTurnNow(turnId: number, origin: PromptOrigin): Promise<{ readonly turnId: number }> {
    if (this.disposed) return { turnId };
    // 2. Assemble LLM messages: the profile system prompt (the TS loop
    //    injects it per-request through `generate(systemPrompt, …)`, not via
    //    the context) plus the context history.
    const messages = [
      { role: "system", content: this.profile.getSystemPrompt() },
      ...this.context.get().map((message) => this.toLlmMessage(message)),
    ];

    // 3. Run the Rust engine (session API: approvals pause and resume).
    const provider = await this.providerConfig();
    const inputJson = JSON.stringify({
      turnId,
      origin: toEngineTurnOrigin(origin),
      // TS `usesWorkerRejectionGuidance` parity: subagent/worker turns append
      // the "Try a different approach…" suffix to permission-deny and
      // approval-rejected tool outputs (toolApprovalService.ts).
      usesWorkerRejectionGuidance: this.scopeContext.agentId !== 'main',
      // TS `isToolActive` parity, engine side: the bridge filters its native
      // defs (Bash/…) by this allowlist, so an inactive Bash no longer leaks
      // into the request `tools` field. `null` = unconstrained (all tools).
      activeTools: this.effectiveActiveTools() ?? null,
      messages,
      tools: [],
      provider,
      maxStepsPerTurn: this.maxStepsPerTurn() ?? null,
      maxRetriesPerStep: this.maxRetriesPerStep() ?? null,
      maxContextTokens: this.maxContextTokens() ?? null,
      nextAgentId: await this.computeNextAgentId(),
      // TaskStop SIGTERM grace (TS `killGracePeriodMs` parity): the engine's
      // bash poller waits this long between SIGTERM and SIGKILL so a trap
      // keeps its cleanup window; the TS task service reads the same config.
      killGraceMs: this.killGracePeriodMs(),
      // Completion-review protocol (TS `COMPLETION_REVIEW_MIN_STEPS`
      // parity): after a tool-free step at/after the threshold the engine
      // injects the reminder and keeps the turn alive until AllDone. Always
      // passed for runnable profiles; short turns below the threshold are
      // unaffected by the engine.
      completionReview: {
        minSteps: COMPLETION_REVIEW_MIN_STEPS,
        reminder: COMPLETION_REVIEW_REMINDER,
      },
      cwd: this.profile.data().cwd ?? process.cwd(),
      // No `shell`: the engine resolves its own bash-preferring default
      // (the TS probe chain `/bin/bash` → `/usr/bin/bash` →
      // `/usr/local/bin/bash` → `/bin/sh`), matching the TS bash tool's
      // `env.shellPath` spawn target and `SHELL` env value.
    });
    const policyJson = JSON.stringify({
      mode: this.modeService.mode,
      rules: this.rulesService.rules,
      sessionApprovedPatterns: this.rulesService.sessionApprovalRulePatterns,
    });
    // Test hook: DIMI_RUST_ENGINE_SCRIPTED injects scripted LLM segments.
    const scripted = process.env["DIMI_RUST_ENGINE_SCRIPTED"];
    const session = new RustTurnSession(
      inputJson,
      policyJson,
      scripted ?? undefined,
      this.registryId,
    );
    this.sessions.add(session);
    this.activeSession = session;
    // A2 (review): native tools (Bash/Agent/…) execute inside the engine,
    // bypassing the TS toolExecutor — register the PreToolUse gate so
    // user-configured veto hooks apply to them too. External tools answer
    // `allow` here (their callback runs its own PreToolUse).
    session.setToolGate((payloadJson: string) => {
      void (async () => {
        const payload = JSON.parse(payloadJson) as {
          requestId: string;
          toolName: string;
          arguments: unknown;
        };
        let verdict: { decision: "allow" } | { decision: "block"; reason: string } = {
          decision: "allow",
        };
        try {
          const hooksRunner = this.instantiation.invokeFunction(
            (accessor) =>
              accessor.get(IExternalHooksRunnerService) as IExternalHooksRunnerService | undefined,
          );
          if (hooksRunner !== undefined && ENGINE_NATIVE_TOOLS.has(payload.toolName)) {
            const block = await hooksRunner.triggerBlock("PreToolUse", {
              matcherValue: payload.toolName,
              signal: new AbortController().signal,
              sessionId: this.sessionContext.sessionId,
              inputData: {
                toolName: payload.toolName,
                toolInput: isPlainRecord(payload.arguments) ? payload.arguments : {},
                toolCallId: `native-${payload.toolName}`,
              },
            });
            if (block !== undefined) {
              verdict = { decision: "block", reason: block.reason };
            }
          }
        } catch {
          // Fail-open: no hooks configured / runner unavailable.
        }
        session.completeToolGate(payload.requestId, JSON.stringify(verdict));
      })();
    });
    try {
      await this.runEngineSession(session, turnId, provider["model"] as string, origin);
    } finally {
      this.activeSession = undefined;
    }
    return { turnId };
  }

  /**
   * Run one engine session to completion: register the TS tool ecosystem,
   * drive the approval loop, mirror the engine events into the context.
   */
  private async runEngineSession(
    session: RustTurnSession,
    turnId: number,
    providerModel: string,
    origin: PromptOrigin,
  ): Promise<void> {
    interface EngineProgress {
      progress: {
        status: string;
        outcome?: { status: string; error?: string; errorCode?: string };
        approval?: {
          requestId: string;
          toolCallId: string;
          toolName: string;
          action?: string;
          display?: unknown;
          toolInput?: unknown;
        };
      };
    }
    // Register the TS tool ecosystem (MCP / plugins / skills / built-in file
    // tools) into the engine; the Rust-native tools stay on the Rust side.
    // Registration is a synchronous napi call that can throw (unknown native
    // tool name / malformed parameter JSON / registry lock busy) — a throw
    // here must NOT escape into `startQueuedTurn`'s `.catch(() => undefined)`
    // (the prompt op is already recorded, so the turn would never end): log
    // it, then skip the tool, or fail the turn for the native-def path.
    const engineNativeTools = ENGINE_NATIVE_TOOLS;
    // TS `isToolActive` parity (profile allowlist/denylist): only active
    // tools are exposed to the model. AllDone is always active (the
    // completion-review protocol needs it). The engine's hardcoded native
    // defs (Bash/…) are filtered by the same set via `activeTools`.
    const activeSet = this.effectiveActiveTools();
    const isToolActiveForRunner = (name: string): boolean =>
      activeSet === undefined || activeSet.includes(name);
    for (const info of this.toolRegistry.list()) {
      if (!isToolActiveForRunner(info.name)) continue;
      try {
        if (engineNativeTools.has(info.name)) {
          // The Rust-native tools (Agent / AgentOutput / WaitFor) are
          // registered executor-first on the engine side — without an
          // LLM-facing def, so the model could not see them in the request
          // `tools` field. Advertise their defs (name/description/parameters)
          // through the bridge; the engine re-syncs them into every request
          // before each run/resume. Bash keeps the bridge's hardcoded def, so
          // only the three async tools are pushed from here.
          if (info.name !== 'Bash') {
            if (info.name === 'WaitFor') {
              // P1-2 (review): the TS `waitForTool.ts` def (user-wait
              // semantics, no `agent_id`) does NOT describe the engine's
              // WaitFor (waits for a subagent task by `agent_id`). Push the
              // engine-matching def so the model passes `agent_id`.
              session.registerNativeToolDef(
                info.name,
                WAIT_FOR_NATIVE_DESCRIPTION,
                JSON.stringify(WAIT_FOR_NATIVE_PARAMETERS),
              );
            } else {
              session.registerNativeToolDef(
                info.name,
                info.description,
                JSON.stringify(info.parameters ?? { type: "object", properties: {} }),
              );
            }
          }
          continue;
        }
        const tool = this.toolRegistry.resolve(info.name);
        if (tool === undefined) continue;
        // The def (name/description/parameters) advertises the tool to the
        // model; the engine re-syncs it into every request's `tools` field.
        session.registerExternalTool(
          info.name,
          info.description,
          JSON.stringify(info.parameters ?? { type: "object", properties: {} }),
          (payloadJson: string) => {
        void (async () => {
          const payload = JSON.parse(payloadJson) as {
            requestId: string;
            toolCallId?: string;
            name: string;
            arguments: unknown;
            toolCalls?: Array<{ id?: string; name: string; arguments: unknown }>;
          };
          try {
            // The engine carries the full assistant-message batch this call
            // is part of (`ToolContext.tool_calls` → bridge `payload.toolCalls`):
            // rebuild the TS `ToolResolutionContext` so same-round validations
            // (AllDone's "only tool call in its round" / background-task
            // guards) behave exactly like the TS loop, which passes
            // `{ toolCalls: allCalls }` (toolExecutorService).
            const toolCalls = (payload.toolCalls ?? []).map((call) => ({
              type: "function" as const,
              id: call.id ?? payload.toolCallId ?? payload.requestId,
              name: call.name,
              arguments:
                typeof call.arguments === "string"
                  ? call.arguments
                  : (JSON.stringify(call.arguments) ?? "null"),
            }));
            const execution = await tool.resolveExecution(payload.arguments, { toolCalls });
            if (execution.isError === true) {
              session.completeToolCall(
                payload.requestId,
                JSON.stringify({
                  toolCallId: payload.toolCallId ?? payload.requestId,
                  toolName: payload.name,
                  output: toText(execution.output),
                  isError: true,
                  stopTurn: execution.stopTurn === true,
                  updates: [],
                }),
              );
              return;
            }
            // P1-8 (review): the Rust path bypasses the TS toolExecutor, so
            // PreToolUse external hooks (veto) never fire on their own — the
            // runner triggers them explicitly, exactly where TS does
            // (`beforeExecuteEmitter.fireBeforeExecute` after resolve,
            // before execute). A block vetoes the call.
            const signal = new AbortController().signal;
            const hooksRunner = this.instantiation.invokeFunction(
              (accessor) =>
                accessor.get(IExternalHooksRunnerService) as IExternalHooksRunnerService | undefined,
            );
            const toolInput = isPlainRecord(payload.arguments) ? payload.arguments : {};
            const toolCallId = payload.toolCallId ?? payload.requestId;
            if (hooksRunner !== undefined) {
              const block = await hooksRunner.triggerBlock("PreToolUse", {
                matcherValue: payload.name,
                signal,
                sessionId: this.sessionContext.sessionId,
                inputData: {
                  toolName: payload.name,
                  toolInput,
                  toolCallId,
                },
              });
              if (block !== undefined) {
                session.completeToolCall(
                  payload.requestId,
                  JSON.stringify({
                    toolCallId,
                    toolName: payload.name,
                    output: block.reason,
                    isError: true,
                    stopTurn: false,
                    updates: [],
                  }),
                );
                return;
              }
            }
            const result = await execution.execute({
              turnId: 0,
              toolCallId,
              signal,
              onUpdate: (update) => {
                if (signal.aborted) return;
                // TS `dispatchToolProgress` parity: stream live tool
                // updates as `tool.progress` bus events.
                this.eventBus.publish({
                  type: "tool.progress",
                  turnId,
                  toolCallId,
                  update,
                } as never);
              },
            });
            // P1-7 (review): truncate oversized tool results for the model
            // (TS toolResultTruncation parity). The engine feeds the raw
            // output text into the next request, so a multi-MB result must
            // be replaced by the persisted-file preview; fail soft if the
            // truncation service is unavailable.
            let finalResult = result;
            const truncation = this.instantiation.invokeFunction(
              (accessor) =>
                accessor.get(IAgentToolResultTruncationService) as
                  | IAgentToolResultTruncationService
                  | undefined,
            );
            if (truncation !== undefined) {
              try {
                finalResult = await truncation.truncateForModel({
                  toolName: payload.name,
                  toolCallId,
                  result,
                });
              } catch {
                // Keep the full output (fail soft).
              }
            }
            session.completeToolCall(
              payload.requestId,
              JSON.stringify({
                toolCallId,
                toolName: payload.name,
                output: toText(finalResult.output),
                isError: finalResult.isError === true,
                stopTurn: finalResult.stopTurn === true,
                updates: [],
              }),
            );
            // P1-8 (review): PostToolUse fire-and-forget (TS
            // `notifyPostToolUse` parity — informational, never blocks).
            if (hooksRunner !== undefined) {
              const outputText = toText(finalResult.output);
              const isError = finalResult.isError === true;
              void hooksRunner.fireAndForgetTrigger(
                isError ? "PostToolUseFailure" : "PostToolUse",
                {
                  matcherValue: payload.name,
                  signal,
                  sessionId: this.sessionContext.sessionId,
                  inputData: {
                    toolName: payload.name,
                    toolInput,
                    toolCallId,
                    error: isError ? toDimiErrorPayload(outputText) : undefined,
                    toolOutput: isError ? undefined : outputText.slice(0, 2000),
                  },
                },
              );
            }
          } catch (error) {
            session.completeToolCall(
              payload.requestId,
              JSON.stringify({
                toolCallId: payload.toolCallId ?? payload.requestId,
                toolName: payload.name,
                output: `Tool "${payload.name}" failed: ${error instanceof Error ? error.message : String(error)}`,
                isError: true,
                stopTurn: false,
                updates: [],
              }),
            );
          }
        })();
      });
      } catch (error) {
        if (engineNativeTools.has(info.name) && info.name !== 'Bash') {
          // A native-def registration failure (unknown native tool name /
          // malformed parameter JSON / registry lock busy) is a config/code
          // error: the model would silently lose Agent/AgentOutput/WaitFor
          // for the whole turn. Log it, surface a failed turn.ended + error
          // on the bus (TS loopService parity — the activity view's busy
          // flag and pending interactions reset on turn.ended), then fail
          // the turn explicitly.
          this.log.error('[rustEngineTurnRunner] failed to register native tool def', {
            name: info.name,
            error,
          });
          this.eventBus.publish({
            type: 'turn.ended',
            turnId,
            reason: 'failed',
            error: {
              name: 'ToolRegistrationError',
              message: `Failed to register native tool def "${info.name}"`,
            },
          } as never);
          this.eventBus.publish({
            type: 'error',
            name: 'ToolRegistrationError',
            message: `Failed to register native tool def "${info.name}"`,
          } as never);
          throw error;
        }
        // An external-tool registration failure is transient (the same list
        // is re-registered on the next turn's fresh session): log and skip
        // the tool so the rest of the turn can proceed.
        this.log.error('[rustEngineTurnRunner] failed to register external tool', {
          name: info.name,
          error,
        });
      }
    }
    // 4. Engine events → bus (projection folds them into transcript ops).
    //    Context mirroring mirrors the TS loop's record stream exactly:
    //    step.begin (fresh uuid per step) → content.part → tool.call (full
    //    args) → tool.result → step.end — `loopEventFold` reconstructs the
    //    assistant + tool messages, so the runner never appends messages by
    //    hand (that produced duplicated/placeholder history).
    let stepUuid: string | undefined;
    // Streamed parts are recorded in arrival order (TS `appendResponseContent`
    // iterates the provider message content in stream order): consecutive
    // deltas of the same type merge into one part, a type change opens a new
    // part — never reordered into a merged think-before-text pair.
    type OpenSegment = { type: "think"; text: string } | { type: "text"; text: string };
    const segments: OpenSegment[] = [];
    // Whether the CURRENT step's LLM response completed (a tool call started).
    // TS lands the step's text as soon as its response completes; a step
    // interrupted mid-tool must flush, a step interrupted mid-stream must not.
    let stepSawToolCall = false;
    // Native (in-engine) tool calls seen this turn: toolCallId → {name, input}.
    // TS fires PostToolUse for EVERY executed tool; native tools bypass the
    // external-tool callback, so the runner fires it on the mirrored
    // `tool.result` (fire-and-forget, informational).
    const nativeToolCalls = new Map<string, { name: string; input: unknown }>();
    let usage = emptyUsage();

    const publish = (event: Record<string, unknown>): void => {
      const { type, ...rest } = event;
      this.eventBus.publish({ type, ...rest } as never);
    };

    const flushParts = (turnId: number, step: number): void => {
      for (const segment of segments.splice(0)) {
        const part: ContentPart =
          segment.type === "think"
            ? { type: "think", think: segment.text }
            : { type: "text", text: segment.text };
        this.context.appendLoopEvent({
          type: "content.part",
          stepUuid: stepUuid!,
          part,
          uuid: randomUUID(),
          turnId: String(turnId),
          step,
        });
      }
    };

    const handleEngineEvent = (event: Record<string, unknown>): void => {
      // The agent was disposed while this turn/worker was in flight: ignore
      // the event (wire/context are gone; TS parity — a disposed session
      // suppresses everything, not just task settles).
      if (this.disposed) return;
      // Task lifecycle events are transport shapes, not bus events: translate
      // them into the TS task/subagent records (wire ops + bus events) —
      // publishing the raw shape would collide with the protocol's
      // `task.started` / `task.terminated` session events (which carry the
      // folded `info`).
      if (event["type"] === "task.started") {
        // F1: record the session whose event sink delivered the start — the
        // engine's per-task cancel signal lives on THIS session's task map
        // (`RustTurnSession::new` creates a fresh map per turn), so TaskStop
        // on a worker-launched task (sub-subagent / subagent-launched bash)
        // must cancel through it, not through `activeSession` (which is
        // undefined or a newer turn's session by then).
        this.taskSessions.set(toText(event["taskId"]), session);
        this.handleTaskStarted(event);
        return;
      }
      if (event["type"] === "task.output") {
        // Live output stream (TS ProcessTask parity): forward the delta into
        // the task-service entry's sink so TaskOutput shows output while the
        // engine task is still running, not only at settle.
        this.handleTaskOutput(event);
        return;
      }
      if (event["type"] === "task.settled") {
        this.handleTaskSettled(event);
        return;
      }
      if (event["type"] === "completion.review.injected") {
        // Mirror the completion-review reminder into the context (TS
        // loopContinuationService parity: origin
        // `system_trigger`/`completion_review`); NOT published on the bus
        // (TS emits no bus event for the reminder).
        this.context.append({
          role: "user",
          content: [
            {
              type: "text",
              // P2-4 (review): TS `appendSystemReminder` wraps reminders in
              // `<system-reminder>` markers. The engine injects (and
              // announces) the wrapped reminder; wrap idempotently here so a
              // bare reminder from any source still lands wrapped.
              text: wrapSystemReminder(toText(event["reminder"])),
            },
          ],
          toolCalls: [],
          origin: { kind: "system_trigger", name: "completion_review" },
          id: randomUUID(),
        });
        return;
      }
      // P0-1 / P1-1 / P1-5 (adversarial review): the engine's wire shapes
      // differ from what TS bus consumers expect — normalize before
      // publishing:
      //  - `turn.step.completed.usage` → TS four-component TokenUsage (the
      //    live transcript projector folds it per step; the engine's
      //    TranscriptUsage shape made turn-header usage NaN→null and broke
      //    the transcript zod contract).
      //  - `turn.started.prompt` → stripped for non-displayable origins
      //    (task/cron/hook notifications must not leak their steering XML).
      //  - `turn.ended.error` → full DimiErrorPayload (name/retryable).
      const busEvent = { ...event };
      if (event["type"] === "turn.step.completed") {
        const engineUsage = event["usage"] as
          | { inputTokens?: number; outputTokens?: number; cachedTokens?: number }
          | undefined;
        if (engineUsage !== undefined) {
          const inputCacheRead = engineUsage.cachedTokens ?? 0;
          busEvent["usage"] = {
            inputOther: Math.max((engineUsage.inputTokens ?? 0) - inputCacheRead, 0),
            output: engineUsage.outputTokens ?? 0,
            inputCacheRead,
            inputCacheCreation: 0,
          };
        } else {
          busEvent["usage"] = emptyUsage();
        }
      } else if (event["type"] === "turn.ended" && event["reason"] === "failed") {
        const rawError = event["error"] as Record<string, unknown> | undefined;
        if (rawError !== undefined) {
          busEvent["error"] = buildErrorPayload(rawError);
        }
      } else if (event["type"] === "turn.started" && !isDisplayablePromptOrigin(origin)) {
        delete busEvent["prompt"];
      }
      publish(busEvent);
      switch (event["type"]) {
        case "turn.step.started": {
          const stepNumber = Number(event["step"] ?? 1);
          stepUuid = randomUUID();
          stepSawToolCall = false;
          this.context.appendLoopEvent({
            type: "step.begin",
            uuid: stepUuid,
            turnId: String(turnId),
            step: stepNumber,
          });
          break;
        }
        case "thinking.delta": {
          const last = segments[segments.length - 1];
          if (last?.type === "think") {
            last.text += toText(event["delta"]);
          } else {
            segments.push({ type: "think", text: toText(event["delta"]) });
          }
          break;
        }
        case "assistant.delta": {
          const last = segments[segments.length - 1];
          if (last?.type === "text") {
            last.text += toText(event["delta"]);
          } else {
            segments.push({ type: "text", text: toText(event["delta"]) });
          }
          break;
        }
        case "tool.call.delta": {
          // Streamed tool-call arguments: nothing to record per delta; the
          // full call lands on tool.call.started.
          break;
        }
        case "tool.call.started": {
          const id = toText(event["toolCallId"]);
          const name = toText(event["name"]);
          stepSawToolCall = true;
          if (ENGINE_NATIVE_TOOLS.has(name)) {
            nativeToolCalls.set(id, { name, input: event["args"] });
          }
          this.context.appendLoopEvent({
            type: "tool.call",
            stepUuid: stepUuid!,
            toolCallId: id,
            name,
            args: event["args"],
            uuid: randomUUID(),
            turnId: String(turnId),
            step: Number(event["step"] ?? 1),
          });
          break;
        }
        case "tool.result": {
          const id = toText(event["toolCallId"]);
          const output = toText(event["output"]);
          const isError = event["isError"] === true;
          // TS `notifyPostToolUse` parity for native tools: the engine
          // executed the call in-process, so the runner fires the
          // fire-and-forget hook on the mirrored result.
          const native = nativeToolCalls.get(id);
          if (native !== undefined) {
            nativeToolCalls.delete(id);
            const hooksRunner = this.instantiation.invokeFunction(
              (accessor) =>
                accessor.get(IExternalHooksRunnerService) as IExternalHooksRunnerService | undefined,
            );
            if (hooksRunner !== undefined) {
              void hooksRunner.fireAndForgetTrigger(
                isError ? "PostToolUseFailure" : "PostToolUse",
                {
                  matcherValue: native.name,
                  signal: new AbortController().signal,
                  sessionId: this.sessionContext.sessionId,
                  inputData: {
                    toolName: native.name,
                    toolInput: isPlainRecord(native.input) ? native.input : {},
                    toolCallId: id,
                    error: isError ? toDimiErrorPayload(output) : undefined,
                    toolOutput: isError ? undefined : output.slice(0, 2000),
                  },
                },
              );
            }
          }
          this.context.appendLoopEvent({
            type: "tool.result",
            toolCallId: id,
            result: { output, isError },
            parentUuid: stepUuid,
          });
          break;
        }
        case "context.compacted": {
          // The engine compacted its working history mid-turn: mirror the
          // same wire record + context shape the TS fullCompaction service
          // applies (live projections and cold rebuilds stay consistent).
          const summary = toText(event["summary"]);
          const tokensBefore = Number(event["tokensBefore"] ?? 0);
          const compactedCount = Number(event["compactedCount"] ?? 0);
          this.wire.dispatch(fullCompactionBegin({ source: "auto" }));
          const result = this.context.applyCompaction({
            summary,
            contextSummary: buildCompactionSummaryText(summary),
            compactedCount,
            tokensBefore,
          });
          this.wire.dispatch(fullCompactionComplete({}));
          this.eventBus.publish({
            type: "compaction.completed",
            result,
          } as never);
          break;
        }
        case "turn.step.interrupted": {
          // TS `appendResponseContent` lands the step's text the moment its
          // LLM response completes (before tools run). A step interrupted
          // mid-tool has a completed response (tool calls were seen) → flush
          // the buffered parts so the text survives a cancel; a step
          // interrupted mid-LLM-stream (no tool calls yet) drops the partial
          // text — the response never completed, matching TS.
          if (stepSawToolCall) {
            flushParts(turnId, Number(event["step"] ?? 1));
          }
          break;
        }
        case "turn.step.completed": {
          const stepFinish = toText(event["finishReason"] ?? "end_turn");
          const stepNumber = Number(event["step"] ?? 1);
          flushParts(turnId, stepNumber);
          // Engine usage (inputTokens/outputTokens/cachedTokens) → the TS
          // four-component TokenUsage + wire usage.record. Per-step parity:
          // every step.end carries THIS step's LLM usage (TS
          // llmRequesterService starts each request at emptyUsage), so a
          // step without recorded tokens gets zeros — never the previous
          // step's numbers carried forward.
          const engineUsage = event["usage"] as
            | { inputTokens?: number; outputTokens?: number; cachedTokens?: number }
            | undefined;
          if (engineUsage !== undefined) {
            const inputCacheRead = engineUsage.cachedTokens ?? 0;
            usage = {
              inputOther: Math.max((engineUsage.inputTokens ?? 0) - inputCacheRead, 0),
              output: engineUsage.outputTokens ?? 0,
              inputCacheRead,
              inputCacheCreation: 0,
            };
          } else {
            usage = emptyUsage();
          }
          this.usageService.record(
            providerModel,
            usage,
            { kind: "loop", turnId: String(turnId), step: stepNumber } as never,
          );
          this.context.appendLoopEvent({
            type: "step.end",
            uuid: stepUuid!,
            turnId: String(turnId),
            step: stepNumber,
            finishReason: stepFinish,
            usage,
          });
          // P1-3 (adversarial review): TS fires the Stop hook after every
          // non-tool step (`externalHooksService` onDidFinishStep → runStop);
          // a returned reason becomes a continuation (system_trigger /
          // stop_hook) and the turn continues.
          if (stepFinish !== "tool_use" && stepFinish !== "filtered") {
            this.runStopHook(turnId);
          }
          break;
        }
        case "turn.ended": {
          // Failed turns surface the error bus event (TS failLoopStep
          // parity) so error handlers/subscribers see it, with the same
          // DimiErrorPayload shape the `turn.ended` event now carries.
          // P1-3 (focused review): TS resets `stopHookContinuationUsed` on
          // every turn.ended — the Stop hook must fire again on the next turn.
          this.stopHookContinuationUsed = false;
          if (event["reason"] === "failed" && event["error"] !== undefined) {
            this.eventBus.publish({
              type: "error",
              ...buildErrorPayload(event["error"] as Record<string, unknown>),
            } as never);
          }
          break;
        }
        default:
          break;
      }
    };

    // Stream: the engine pushes every event through this callback as it is
    // emitted (napi ThreadsafeFunction — main-thread delivery, FIFO order),
    // so the bus sees turn.started / assistant.delta / tool.* / turn.ended
    // as they happen, exactly like the TS loop. `run()`/`resume()` resolve
    // with only the progress.
    session.setOnEvent((eventJson: string) => {
      try {
        handleEngineEvent(JSON.parse(eventJson) as Record<string, unknown>);
      } catch (error) {
        // The callback cannot propagate to the runner's await chain (the
        // ThreadsafeFunction is fire-and-forget) — surface it instead of
        // dropping the failure silently.
        this.log.error("[rustEngineTurnRunner] failed to process engine event", { error });
      }
    });

    let progress: EngineProgress = JSON.parse(await session.run()) as EngineProgress;
    // Approval loop: surface the request, wait for the user, resume.
    while (progress.progress.status === "needsApproval") {
      const approval = progress.progress.approval!;
      const approvalRequest = {
        sessionId: this.sessionContext.sessionId,
        agentId: this.scopeContext.agentId,
        turnId,
        toolCallId: approval.toolCallId,
        toolName: approval.toolName,
        action: `Approve ${approval.toolName}`,
        display: {
          kind: "generic",
          summary: `Approve ${approval.toolName}`,
          detail: approval.toolInput,
        },
      } as Parameters<ISessionApprovalService["request"]>[0];
      this.eventBus.publish({ type: "permission.approval.requested", ...approvalRequest } as never);
      let response: ApprovalResponse = { decision: "approved" };
      // The approval wait is cancellable: `cancel()` aborts it so the turn
      // resolves as cancelled without waiting for the user (TS abortable
      // parity).
      const approvalController = new AbortController();
      this.approvalAbort = approvalController;
      try {
        const approvalService = this.instantiation.invokeFunction(
          (accessor) => accessor.get(ISessionApprovalService) as ISessionApprovalService | undefined,
        );
        response =
          approvalService !== undefined
            ? await Promise.race([
                approvalService.request(approvalRequest),
                abortOnSignal(approvalController.signal).then(() => ({ decision: "cancelled" as const })),
              ])
            : { decision: "approved" };
      } catch {
        response = { decision: "rejected" };
      } finally {
        this.approvalAbort = undefined;
      }
      this.eventBus.publish({
        type: "permission.approval.resolved",
        ...approvalRequest,
        decision: response.decision,
      } as never);
      // Session-scope approval memory (TS toolApproval parity): an approved
      // `scope: session` response records the rule so the same tool pattern
      // is not re-asked within the session.
      this.rulesService.recordApprovalResult({
        turnId,
        toolCallId: approvalRequest.toolCallId!,
        toolName: approvalRequest.toolName,
        action: approvalRequest.action,
        sessionApprovalRule:
          response.decision === "approved" && response.scope === "session"
            ? approvalRequest.toolName
            : undefined,
        result: response,
      });
      // P1-6 (review): the engine's policy is frozen per session — push a
      // session-scope approval into the live policy so the SAME turn's
      // remaining batch auto-approves instead of re-asking (TS
      // session-approval-history parity). Must happen before resume so the
      // continued batch evaluates with the updated policy.
      if (response.decision === "approved" && response.scope === "session") {
        session.addSessionApproval(approvalRequest.toolName);
      }
      progress = JSON.parse(await session.resume(JSON.stringify(response))) as typeof progress;
    }
  }

  /**
   * TS `externalHooksService.runStop` parity (P1-3): after a non-tool step,
   * fire the Stop hook; a returned reason becomes a continuation message
   * (origin `system_trigger`/`stop_hook`) that keeps the turn alive —
   * steered into the running turn, or launched as a fresh turn when the
   * engine already finished (the task-notification fallback). Fires at most
   * once after a continuation was used, mirroring `stopHookContinuationUsed`.
   */
  private runStopHook(turnId: number): void {
    if (this.stopHookContinuationUsed) return;
    // Synchronous latch (P1-3 focused review): set BEFORE any await so a
    // concurrent `turn.ended` reset cannot clobber an in-flight delivery.
    // TS sets it after runStop resolves, inside the step lifecycle; the
    // runner's step handler and turn.ended handler are sequential, so the
    // synchronous set here is equivalent (a non-tool step without a
    // continuation ends the turn — a second non-tool step in the same turn
    // cannot occur).
    this.stopHookContinuationUsed = true;
    const hooksRunner = this.instantiation.invokeFunction(
      (accessor) =>
        accessor.get(IExternalHooksRunnerService) as IExternalHooksRunnerService | undefined,
    );
    if (hooksRunner === undefined) return;
    void (async () => {
      try {
        const block = await hooksRunner.triggerBlock("Stop", {
          signal: new AbortController().signal,
          sessionId: this.sessionContext.sessionId,
          inputData: { stopHookActive: false },
        });
        const reason = block?.reason;
        if (reason === undefined || reason.length === 0) return;
        const origin: SystemTriggerOrigin = { kind: "system_trigger", name: "stop_hook" };
        if (
          this.turnRunning &&
          this.activeSession !== undefined &&
          this.activeSession.steer(reason)
        ) {
          this.context.append({
            role: "user",
            content: [{ type: "text", text: reason }],
            toolCalls: [],
            origin,
            id: randomUUID(),
          });
          this.wire.dispatch(steerTurn({ input: [{ type: "text", text: reason }], origin }));
        } else {
          void this.runTurn({ input: [{ type: "text", text: reason }], origin }).catch(
            () => undefined,
          );
        }
      } catch {
        // Fail-open: no hooks configured / runner unavailable.
      }
    })();
  }

  /**
   * `task.started` (engine transport) → TS records: the persisted
   * `task.started` wire op (TaskModel / transcript / TaskList) plus the
   * `subagent.spawned` session event for subagent launches.
   */
  private handleTaskStarted(event: Record<string, unknown>): void {
    const taskId = toText(event["taskId"]);
    const agentId = toText(event["agentId"]);
    const kind = toText(event["kind"]);
    const description = toText(event["description"]);
    const startedAt = Date.now();
    const pid = toOptionalNumber(event["pid"]);
    this.taskInfos.set(taskId, { description, startedAt, pid });
    // Subagent agent ids are engine-issued (`agent-<n>`): remember them so
    // the next turn's seed continues past them (TS `nextAvailableAgentId`
    // parity; the wire TaskModel also records them, this is the live mirror).
    if (kind === "agent" && agentId !== "") this.engineAgentIds.add(agentId);
    const info = (
      kind === "agent"
        ? {
            taskId,
            kind: "agent",
            agentId,
            subagentType: description,
            description,
            status: "running",
            detached: true,
            startedAt,
            endedAt: null,
          }
        : {
            taskId,
            kind: "process",
            command: description,
            pid: pid ?? 0,
            exitCode: null,
            description,
            status: "running",
            detached: true,
            startedAt,
            endedAt: null,
          }
    ) as AgentTaskInfo;
    this.wire.dispatch(taskStarted({ info }));
    // TaskList / TaskStop / TaskOutput parity: register the engine task with
    // the agent task service (explicit engine task id so the tools address it
    // by the wire-visible id). The service entry is `detached: false` —
    // `recorded: false` — so it dispatches no wire ops and no notification
    // (the runner owns those); the adapter marks the info `detached: true`
    // for TaskList rendering. TaskStop reaches the engine through the
    // adapter's `forceStop` → `session.cancelTask(taskId)`.
    this.registerEngineTask(taskId, agentId, kind, description, pid);
    if (kind === "agent") {
      // TS `emitAgentRunSpawned` parity minus the profile concept: the
      // engine has no subagent profiles (it ignores `subagent_type`), so
      // `subagentName` carries the Agent-tool description — a documented
      // deviation, TS sends the profile name (e.g. "general"). The fields
      // the runner CAN know match: `parentAgentId`/`callerAgentId` are the
      // launching agent's id, `description` is the Agent-tool arg.
      this.eventBus.publish({
        type: "subagent.spawned",
        subagentId: agentId,
        subagentName: description,
        parentToolCallId: toText(event["parentToolCallId"]),
        parentAgentId: this.scopeContext.agentId,
        callerAgentId: this.scopeContext.agentId,
        description,
        runInBackground: false,
      } as never);
      // TS publishes `subagent.started` at the top of `mirrorAgentRun`,
      // immediately after `emitAgentRunSpawned` — same point here.
      this.eventBus.publish({
        type: "subagent.started",
        subagentId: agentId,
      } as never);
    }
  }

  /**
   * `task.output` (engine transport) → the task-service entry's live sink:
   * append the streamed delta so TaskOutput shows partial output while the
   * engine task is still running (TS ProcessTask parity — TS streams chunks
   * as they arrive; the adapter's settle also appends only the not-yet-
   * streamed tail, so nothing is duplicated). No wire op / bus event: the
   * live output is the service's retained buffer, the wire `task.terminated`
   * tail comes from the settle's full output.
   */
  private handleTaskOutput(event: Record<string, unknown>): void {
    const taskId = toText(event["taskId"]);
    const delta = toText(event["delta"]);
    if (delta.length === 0) return;
    const adapter = this.engineTaskAdapters.get(taskId);
    if (adapter !== undefined) {
      adapter.appendOutput(delta);
      return;
    }
    // The adapter may be absent when registration failed (teardown race) or
    // when the delta raced the settle — the settle's full output covers it.
    this.log.debug("[rustEngineTurnRunner] task.output for unregistered task", { taskId });
  }

  /**
   * Register an engine background task with `IAgentTaskService` (TaskList /
   * TaskStop / TaskOutput parity). The adapter bridges TaskStop into the
   * engine's per-task cancel (`session.cancelTask`) — the launching session's
   * worker/poller kills its work and settles "killed" — and its `start`
   * streams the settle output into the service's sink so TaskOutput and the
   * terminal info see it.
   */
  private registerEngineTask(
    taskId: string,
    agentId: string,
    kind: string,
    description: string,
    pid: number | undefined,
  ): void {
    // F1: resolve the task's OWNING session — the one whose event sink
    // delivered `task.started` (recorded in `handleEngineEvent`). Tasks
    // launched from background workers arrive after the launching turn
    // ended, when `activeSession` is undefined or a newer turn's session,
    // and the engine's per-session task map holds the cancel signal on the
    // owning session only. Fall back to `activeSession` (the normal case:
    // top-level tasks start mid-turn).
    const session = this.taskSessions.get(taskId) ?? this.activeSession;
    const adapter = new EngineTaskAdapter({
      taskId,
      agentId,
      kind: kind === "agent" ? "agent" : "process",
      description,
      pid,
      subagentType: kind === "agent" ? description : undefined,
      forceStop: (reason?: string) => {
        // TaskStop / dispose parity: the engine cancels the background work
        // (nested subagent turn or backgrounded bash command), carrying the
        // TaskStop reason so the killed settle reports it on the wire. The
        // session may already be closed — `cancelTask` still flips the task's
        // cancel signal, and the settle is dropped by the closed sink.
        try {
          session?.cancelTask(taskId, reason);
        } catch (error) {
          this.log.error("[rustEngineTurnRunner] failed to cancel engine task", {
            taskId,
            error,
          });
        }
      },
    });
    this.engineTaskAdapters.set(taskId, adapter);
    try {
      this.tasks.registerTask(adapter, { taskId, detached: false });
    } catch (error) {
      // Registration can fail (e.g. the agent scope is being torn down): the
      // wire ops still record the lifecycle; TaskList/TaskStop lose the live
      // entry, matching a task that was never registered.
      this.log.error("[rustEngineTurnRunner] failed to register engine task", {
        taskId,
        error,
      });
      this.engineTaskAdapters.delete(taskId);
    }
  }

  /**
   * `task.settled` (engine transport) → TS records: the persisted
   * `task.terminated` wire op (bounded output tail), the `subagent.completed`
   * / `subagent.failed` session events, and the completion notification
   * delivered to the model (context message + `task.notified` + steer/new
   * turn — TS `activeOrNewTurn` parity).
   */
  private handleTaskSettled(event: Record<string, unknown>): void {
    const taskId = toText(event["taskId"]);
    const agentId = toText(event["agentId"]);
    const kind = toText(event["kind"]);
    const status = toText(event["status"]);
    const output = toText(event["output"]);
    const error =
      event["error"] === undefined || event["error"] === null
        ? undefined
        : toText(event["error"]);
    const launch = this.taskInfos.get(taskId);
    const endedAt = Date.now();
    const stopReason =
      status === "failed" || status === "timed_out" || status === "killed"
        ? error
        : undefined;
    const base = {
      taskId,
      description: launch?.description ?? "",
      status,
      detached: true,
      startedAt: launch?.startedAt ?? endedAt,
      endedAt,
      stopReason,
    };
    const info = (
      kind === "agent"
        ? { ...base, kind: "agent", agentId, subagentType: launch?.description ?? "" }
        : {
            ...base,
            kind: "process",
            command: launch?.description ?? "",
            pid: launch?.pid ?? 0,
            exitCode: toOptionalNumber(event["exitCode"]) ?? null,
          }
    ) as AgentTaskInfo;
    this.wire.dispatch(taskTerminated({ info, outputTail: output.slice(-TERMINAL_OUTPUT_TAIL_CHARS) }));
    if (kind === "agent") {
      if (status === "completed") {
        this.eventBus.publish({
          type: "subagent.completed",
          subagentId: agentId,
          resultSummary: output,
        } as never);
      } else if (status === "failed") {
        this.eventBus.publish({
          type: "subagent.failed",
          subagentId: agentId,
          error: error ?? status,
        } as never);
      }
      // Any other status (e.g. `timed_out` / `killed`): no failure event —
      // TS parity (`mirrorAgentRun` suppresses failure events for
      // aborted/timed-out runs; only the completion notification is
      // delivered).
    }
    // Settle the task-service entry (TaskList stops listing it as running;
    // the final output lands in the service's retained buffer so TaskOutput
    // can read it). The service entry is `recorded: false`, so this fires no
    // wire ops and no duplicate notification.
    const adapter = this.engineTaskAdapters.get(taskId);
    if (adapter !== undefined) {
      adapter.complete(
        output,
        {
          status: status as AgentTaskSettlement["status"],
          stopReason,
        },
        toOptionalNumber(event["exitCode"]),
      );
      this.engineTaskAdapters.delete(taskId);
    }
    // A settled task never settles again — drop the launch facts so the
    // mirror stays bounded by the in-flight task count.
    this.taskInfos.delete(taskId);
    this.taskSessions.delete(taskId);
    // TS `buildAgentTaskNotificationContext` parity: a TaskStop-suppressed
    // task (the tool suppressed the notification before stopping) delivers
    // no notification — the tool result is the only answer the model sees.
    if (this.tasks.getTask(taskId)?.terminalNotificationSuppressed === true) return;
    this.deliverTaskNotification({
      taskId,
      agentId,
      kind,
      status,
      description: launch?.description ?? "",
      output,
      error,
    });
  }

  /**
   * Deliver a task completion notification to the model — the Rust-engine
   * equivalent of the TS task domain's detached-task notification
   * (`TaskNotificationStepRequest`, `activeOrNewTurn` admission): append the
   * `<notification>` XML as a task-origin user message, fire the
   * `task.notified` hook event, then fold it into the running turn's next
   * step (steer) or launch a notification turn when idle.
   */
  private deliverTaskNotification(task: {
    readonly taskId: string;
    readonly agentId: string;
    readonly kind: string;
    readonly status: string;
    readonly description: string;
    readonly output: string;
    readonly error?: string;
  }): void {
    const key = `task:${task.taskId}:${task.status}`;
    if (this.deliveredNotificationKeys.has(key)) return;
    this.deliveredNotificationKeys.add(key);
    const kindLabel = task.kind === "agent" ? "agent" : "process";
    const notification = {
      id: key,
      category: "task",
      type: `task.${task.status}`,
      source_kind: "background_task",
      source_id: task.taskId,
      agent_id: task.kind === "agent" ? task.agentId : undefined,
      title: `Background ${kindLabel} ${task.status}`,
      severity: task.status === "completed" ? "info" : "warning",
      body: buildTaskNotificationBody(task),
      children:
        task.output.length > 0
          ? [renderOutputPreviewBlock(task.output)]
          : undefined,
    };
    const xml = renderNotificationXml(notification);
    const origin: TaskOrigin = {
      kind: "task",
      taskId: task.taskId,
      status: task.status as AgentTaskStatus,
      notificationId: key,
    };
    try {
      // The notification message lands in the context exactly once (TS
      // parity: the loop materializes the request once). Mid-turn: append
      // here, then steer (the engine drains the message into the running
      // turn's next request). Idle: leave the append to `runTurn`, which
      // records the message as the notification turn's user input. When the
      // engine refuses the steer (its turn already ended — a notification
      // racing the teardown), the same idle fallback applies: no pre-append
      // and no `turn.steer` op, and `runTurn` launches/queues a new turn, so
      // the message is never recorded twice and no steer is lost.
      if (this.turnRunning && this.activeSession !== undefined && this.activeSession.steer(xml)) {
        this.context.append({
          role: "user",
          content: [{ type: "text", text: xml }],
          toolCalls: [],
          origin,
          id: randomUUID(),
        });
        this.wire.dispatch(steerTurn({ input: [{ type: "text", text: xml }], origin }));
      } else {
        void this.runTurn({ input: [{ type: "text", text: xml }], origin }).catch(() => undefined);
      }
      this.eventBus.publish({
        type: "task.notified",
        notificationType: `task.${task.status}`,
        title: notification.title,
        body: notification.body,
        severity: notification.severity,
        sourceKind: notification.source_kind,
        sourceId: notification.source_id,
      } as never);
    } finally {
      // A settled task never settles again, so the dedupe key is no longer
      // needed once the delivery is done — dropping it keeps the set bounded
      // by the notifications in flight. The mid-turn+idle double-delivery
      // guard is the single-call structure above, not this set.
      this.deliveredNotificationKeys.delete(key);
    }
  }

  private toLlmMessage(message: ContextMessage): Record<string, unknown> {
    const text = message.content
      .filter((part) => part.type === "text")
      .map((part) => (part as { text?: string }).text ?? "")
      .join("");
    const media = message.content
      .filter((part) => part.type === "image_url" || part.type === "audio_url" || part.type === "video_url")
      .map((part) => {
        const url = (part as { imageUrl?: { url?: string }; audioUrl?: { url?: string }; videoUrl?: { url?: string } }).imageUrl
          ?.url ?? (part as { audioUrl?: { url?: string } }).audioUrl?.url ?? (part as { videoUrl?: { url?: string } }).videoUrl?.url;
        return { type: "media_url", url };
      })
      .filter((part) => part.url !== undefined);
    const toolCalls = message.toolCalls.map((call) => ({
      id: call.id,
      type: "function",
      function: { name: call.name, arguments: call.arguments },
    }));
    return {
      role: message.role,
      content: media.length > 0 ? [...media, ...(text.length > 0 ? [{ type: "text", text }] : [])] : text,
      ...(message.role === "assistant" && toolCalls.length > 0 ? { toolCalls } : {}),
      ...(message.role === "tool" && message.toolCallId !== undefined
        ? { toolCallId: message.toolCallId }
        : {}),
    };
  }

  /**
   * The next subagent agent-id number to hand to the engine for this turn
   * (TS `nextAvailableAgentId` parity): the max known `agent-<n>` suffix
   * among (a) the ids this runner's engine has already handed out across
   * turns and (b) the session's persisted agents, plus one. The engine seeds
   * its per-session counter from it, so ids stay monotonic across turns and
   * server restarts and never collide with TS-assigned ids.
   */
  private async computeNextAgentId(): Promise<number> {
    let maxSuffix = -1;
    const consider = (id: string): void => {
      const match = /^agent-(\d+)$/.exec(id);
      if (match !== null) maxSuffix = Math.max(maxSuffix, Number(match[1]));
    };
    for (const id of this.engineAgentIds) consider(id);
    try {
      const persisted = (await this.sessionMetadata.read()).agents ?? {};
      for (const id of Object.keys(persisted)) consider(id);
    } catch {
      // Metadata unreadable — the in-session ids above still seed the
      // counter monotonically across turns, but the seed can collide with
      // TS-assigned agent ids persisted before a server restart (review
      // nit): surface the degraded mode instead of failing silently.
      this.log.warn(
        "[rustEngineTurnRunner] failed to read session metadata; seeding next agent id from in-session engine ids only",
      );
    }
    return maxSuffix + 1;
  }

  private maxStepsPerTurn(): number | undefined {
    return this.config.get<{ maxStepsPerTurn?: number }>("loop_control")?.maxStepsPerTurn;
  }

  /**
   * The effective engine-side tool allowlist (TS `isToolActiveComposed`
   * parity): the runner's `IAgentToolPolicyService.isToolActive` composes
   * the profile allowlist/denylist, the global `[tools]` config, and the
   * session denylist — with MCP glob semantics for `mcp__*` tools. AllDone
   * stays active regardless (the completion-review protocol needs it).
   * `undefined` = unconstrained (all tools).
   */
  private effectiveActiveTools(): string[] | undefined {
    const all = this.toolRegistry.list().map((info) => info.name);
    const active = all.filter((name) => {
      if (name === ALL_DONE_TOOL_NAME) return true;
      return this.toolPolicy.isToolActive(name);
    });
    return active.length === all.length ? undefined : active;
  }

  private maxRetriesPerStep(): number | undefined {
    return this.config.get<{ maxRetriesPerStep?: number }>("loop_control")?.maxRetriesPerStep;
  }

  private killGracePeriodMs(): number {
    return resolveAgentTaskConfig(this.config)?.killGracePeriodMs ?? DEFAULT_KILL_GRACE_MS;
  }

  private maxContextTokens(): number | undefined {
    const capability = this.profile.data().modelCapabilities;
    const max = capability.max_input_tokens ?? capability.max_context_tokens;
    return max > 0 ? max : undefined;
  }

  private async providerConfig(): Promise<Record<string, unknown>> {
    // Resolve through the product's own provider pipeline (profile + model
    // catalog + credential store), mirroring what the TS loop's requester
    // assembles per request — the config sections the slice-1 version read
    // do not exist.
    const modelAlias = this.profile.data().modelAlias ?? this.profile.getModel();
    let baseUrl = "";
    let apiKey = "";
    try {
      const model = this.modelCatalog.get(modelAlias);
      baseUrl = model.baseUrl;
      const auth = await this.providerRuntime.getAuth(model);
      apiKey = auth?.auth.apiKey ?? "";
    } catch {
      // Unknown model: fall back to the profile defaults below.
    }
    return {
      baseUrl: baseUrl || "https://api.openai.com/v1",
      apiKey,
      model: modelAlias || "gpt-4o",
      thinkingEffort: this.profile.getEffectiveThinkingLevel(),
    };
  }
}

registerScopedService(
  LifecycleScope.Agent,
  IRustEngineTurnRunner,
  RustEngineTurnRunner,
  ScopeActivation.OnScopeCreated,
  "rustEngineTurnRunner",
);
