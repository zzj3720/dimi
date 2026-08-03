/**
 * `rustEngineTurnRunner` — M3 slice-1 swap-in: run one turn through the Rust
 * engine (`RustEngine` napi socket) instead of the TS loop.
 *
 * Enabled by `DIMI_RUST_ENGINE=1` (default off). The runner:
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
import type { ContextMessage, PromptOrigin, TaskOrigin } from "#/agent/contextMemory/types";
import { IAgentLLMRequesterService } from "#/agent/llmRequester/llmRequester";
import { IAgentPermissionModeService } from "#/agent/permissionMode/permissionMode";
import { IAgentToolRegistryService } from "#/agent/toolRegistry/toolRegistry";
import { IAgentPermissionRulesService } from "#/agent/permissionRules/permissionRules";
import { renderNotificationXml } from "#/agent/task/notificationXml";
import { taskStarted, taskTerminated } from "#/agent/task/taskOps";
import type { AgentTaskInfo, AgentTaskStatus } from "#/agent/task/types";
import { cancelTurn, promptTurn, steerTurn, TurnModel } from "#/agent/loop/turnOps";
import { IEventBus } from "#/app/event/eventBus";
import { IConfigService } from "#/app/config/config";
import { ILogService } from "#/_base/log/log";
import { ISessionApprovalService, type ApprovalResponse } from "#/session/approval/approval";
import { IAgentUsageService } from "#/agent/usage/usage";
import { IAgentProfileService } from "#/agent/profile/profile";
import { IAgentScopeContext } from "#/agent/scopeContext/scopeContext";
import { IModelCatalog } from "#/app/modelCatalog/catalog";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import { ISessionContext } from "#/session/sessionContext/sessionContext";
import type { ContentPart } from "#/llmProtocol/message";
import { emptyUsage } from "#/llmProtocol/usage";
import { IWireService } from "#/wire/wire";
import { buildCompactionSummaryText } from "#/agent/contextMemory/compactionHandoff";
import {
  fullCompactionBegin,
  fullCompactionComplete,
} from "#/agent/fullCompaction/compactionOps";

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

function rustEngineEnabled(): boolean {
  return process.env["DIMI_RUST_ENGINE"] === "1";
}

/** Render an engine event/tool value as text (strings pass through). */
function toText(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value) ?? "";
}

/** Optional numeric engine field (pid / exitCode). */
function toOptionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/** `task.terminated` output tail cap (taskService TERMINAL_OUTPUT_TAIL_BYTES). */
const TERMINAL_OUTPUT_TAIL_CHARS = 4 * 1024;

/** Notification output preview cap (taskService NOTIFICATION_FALLBACK_PREVIEW_BYTES). */
const NOTIFICATION_OUTPUT_PREVIEW_CHARS = 3_000;

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
      : task.status === "failed" && task.error !== undefined
        ? `${task.description} failed. Reason: ${task.error}`
        : `${task.description} ${task.status}.`;
  if (task.kind !== "agent" || task.status === "completed") return baseLine;
  const recovery = [
    "",
    `To recover or continue this subagent, call Agent(resume="${task.agentId}", prompt="Pick up where you left off; redo the last tool call if its result was never observed.").`,
    `Use agent_id ("${task.agentId}"), NOT source_id / task_id ("${task.taskId}") — the two look alike but only agent_id is accepted by the resume parameter.`,
    "The subagent retains its full prior context across the restart, but any in-flight tool call lost its result and may need to be redone.",
  ].join("\n");
  return `${baseLine}${recovery}`;
}

/** Mirrors the task domain's `renderOutputPreviewBlock` (tail inline). */
function renderOutputPreviewBlock(output: string): string {
  return [
    `<output-preview bytes="${String(output.length)}" total_bytes="${String(output.length)}" truncated="false">`,
    "Task output tail.",
    escapeXml(output),
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
   * terminal `task.terminated` info from `task.settled`.
   */
  private readonly taskInfos = new Map<
    string,
    { readonly description: string; readonly startedAt: number; readonly pid?: number }
  >();

  /** Notification dedupe (TS `deliveredNotificationKeys` parity). */
  private readonly deliveredNotificationKeys = new Set<string>();

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
    @IModelCatalog private readonly modelCatalog: IModelCatalog,
    @IProviderRuntime private readonly providerRuntime: IProviderRuntime,
    @IInstantiationService private readonly instantiation: IInstantiationService,
    @ILogService private readonly log: ILogService,
  ) {}

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
    // 1. Turn clock + user message (mirrors loopService.startTurn).
    this.wire.dispatch(promptTurn({ input: [...payload.input], origin: payload.origin }));
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

  private startQueuedTurn(entry: {
    readonly turnId: number;
    readonly payload: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin };
    cancelled: boolean;
  }): void {
    this.turnRunning = true;
    this.executingTurnId = entry.turnId;
    void this.runTurnNow(entry.turnId)
      .catch(() => undefined)
      .finally(() => {
        this.turnRunning = false;
        this.executingTurnId = undefined;
        const next = this.queued.find((queued) => !queued.cancelled);
        if (next === undefined) return;
        this.queued.splice(this.queued.indexOf(next), 1);
        this.startQueuedTurn(next);
      });
  }

  private async runTurnNow(turnId: number): Promise<{ readonly turnId: number }> {
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
      messages,
      tools: [],
      provider,
      maxStepsPerTurn: this.maxStepsPerTurn() ?? null,
      maxContextTokens: this.maxContextTokens() ?? null,
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
    const session = new RustTurnSession(inputJson, policyJson, scripted ?? undefined);
    this.activeSession = session;
    try {
      await this.runEngineSession(session, turnId, provider["model"] as string);
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
    const engineNativeTools = new Set(['Bash', 'Agent', 'AgentOutput', 'WaitFor']);
    for (const info of this.toolRegistry.list()) {
      if (engineNativeTools.has(info.name)) continue;
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
          const payload = JSON.parse(payloadJson) as { requestId: string; toolCallId?: string; name: string; arguments: unknown };
          try {
            const execution = await tool.resolveExecution(payload.arguments);
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
            const result = await execution.execute({
              turnId: 0,
              toolCallId: payload.toolCallId ?? payload.requestId,
              signal: new AbortController().signal,
            });
            session.completeToolCall(
              payload.requestId,
              JSON.stringify({
                toolCallId: payload.toolCallId ?? payload.requestId,
                toolName: payload.name,
                output: toText(result.output),
                isError: result.isError === true,
                stopTurn: result.stopTurn === true,
                updates: [],
              }),
            );
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
    }
    // 4. Engine events → bus (projection folds them into transcript ops).
    //    Context mirroring mirrors the TS loop's record stream exactly:
    //    step.begin (fresh uuid per step) → content.part → tool.call (full
    //    args) → tool.result → step.end — `loopEventFold` reconstructs the
    //    assistant + tool messages, so the runner never appends messages by
    //    hand (that produced duplicated/placeholder history).
    let stepUuid: string | undefined;
    let openText = "";
    let openThinking = "";
    let usage = emptyUsage();

    const publish = (event: Record<string, unknown>): void => {
      const { type, ...rest } = event;
      this.eventBus.publish({ type, ...rest } as never);
    };

    const flushParts = (turnId: number, step: number): void => {
      const parts: ContentPart[] = [];
      if (openThinking.length > 0) {
        parts.push({ type: "think", think: openThinking });
        openThinking = "";
      }
      if (openText.length > 0) {
        parts.push({ type: "text", text: openText });
        openText = "";
      }
      for (const part of parts) {
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
      // Task lifecycle events are transport shapes, not bus events: translate
      // them into the TS task/subagent records (wire ops + bus events) —
      // publishing the raw shape would collide with the protocol's
      // `task.started` / `task.terminated` session events (which carry the
      // folded `info`).
      if (event["type"] === "task.started") {
        this.handleTaskStarted(event);
        return;
      }
      if (event["type"] === "task.settled") {
        this.handleTaskSettled(event);
        return;
      }
      publish(event);
      switch (event["type"]) {
        case "turn.step.started": {
          const stepNumber = Number(event["step"] ?? 1);
          stepUuid = randomUUID();
          this.context.appendLoopEvent({
            type: "step.begin",
            uuid: stepUuid,
            turnId: String(turnId),
            step: stepNumber,
          });
          break;
        }
        case "thinking.delta": {
          openThinking += toText(event["delta"]);
          break;
        }
        case "assistant.delta": {
          openText += toText(event["delta"]);
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
        case "turn.step.completed": {
          const stepFinish = toText(event["finishReason"] ?? "end_turn");
          const stepNumber = Number(event["step"] ?? 1);
          flushParts(turnId, stepNumber);
          // Engine usage (inputTokens/outputTokens/cachedTokens) → the TS
          // four-component TokenUsage + wire usage.record.
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
            this.usageService.record(
              providerModel,
              usage,
              { kind: "loop", turnId: String(turnId), step: stepNumber } as never,
            );
          }
          this.context.appendLoopEvent({
            type: "step.end",
            uuid: stepUuid!,
            turnId: String(turnId),
            step: stepNumber,
            finishReason: stepFinish,
            usage,
          });
          break;
        }
        case "turn.ended": {
          // Failed turns surface the error bus event (TS failLoopStep
          // parity) so error handlers/subscribers see it.
          if (event["reason"] === "failed" && event["error"] !== undefined) {
            this.eventBus.publish({
              type: "error",
              ...(event["error"] as Record<string, unknown>),
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
      progress = JSON.parse(await session.resume(JSON.stringify(response))) as typeof progress;
    }
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
    this.taskInfos.set(taskId, { description, startedAt, pid: toOptionalNumber(event["pid"]) });
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
            pid: toOptionalNumber(event["pid"]) ?? 0,
            exitCode: null,
            description,
            status: "running",
            detached: true,
            startedAt,
            endedAt: null,
          }
    ) as AgentTaskInfo;
    this.wire.dispatch(taskStarted({ info }));
    if (kind === "agent") {
      this.eventBus.publish({
        type: "subagent.spawned",
        subagentId: agentId,
        subagentName: description,
        parentToolCallId: toText(event["parentToolCallId"]),
        runInBackground: false,
      } as never);
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
    const base = {
      taskId,
      description: launch?.description ?? "",
      status,
      detached: true,
      startedAt: launch?.startedAt ?? endedAt,
      endedAt,
      stopReason: status === "failed" || status === "timed_out" ? error : undefined,
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
      this.eventBus.publish(
        (status === "completed"
          ? { type: "subagent.completed", subagentId: agentId, resultSummary: output }
          : { type: "subagent.failed", subagentId: agentId, error: error ?? status }) as never,
      );
    }
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
          ? [renderOutputPreviewBlock(task.output.slice(-NOTIFICATION_OUTPUT_PREVIEW_CHARS))]
          : undefined,
    };
    const xml = renderNotificationXml(notification);
    const origin: TaskOrigin = {
      kind: "task",
      taskId: task.taskId,
      status: task.status as AgentTaskStatus,
      notificationId: key,
    };
    this.context.append({
      role: "user",
      content: [{ type: "text", text: xml }],
      toolCalls: [],
      origin,
      id: randomUUID(),
    });
    this.eventBus.publish({
      type: "task.notified",
      notificationType: `task.${task.status}`,
      title: notification.title,
      body: notification.body,
      severity: notification.severity,
      sourceKind: notification.source_kind,
      sourceId: notification.source_id,
    } as never);
    if (this.turnRunning && this.activeSession !== undefined && this.activeSession.steer(xml)) {
      // Mid-turn: fold into the running turn's following step (TS
      // `activeOrNewTurn` parity) — the engine drains it into the next LLM
      // request. When the engine refuses (its turn already ended, a
      // notification racing the teardown), fall through to the idle path so
      // the notification launches a new turn instead of being dropped.
      this.wire.dispatch(steerTurn({ input: [{ type: "text", text: xml }], origin }));
    } else {
      // Idle: launch a notification turn that processes the notification
      // (TS `activeOrNewTurn` parity).
      void this.runTurn({ input: [{ type: "text", text: xml }], origin }).catch(() => undefined);
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

  private maxStepsPerTurn(): number | undefined {
    return this.config.get<{ maxStepsPerTurn?: number }>("loop_control")?.maxStepsPerTurn;
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
