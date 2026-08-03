/**
 * `rustEngineTurnRunner` — M3 slice-1 swap-in: run one turn through the Rust
 * engine (`RustEngine` napi socket) instead of the TS loop.
 *
 * Enabled by `DIMI_RUST_ENGINE=1` (default off). The runner:
 *  1. records `turn.prompt` (turn clock) and appends the user message;
 *  2. assembles the LLM messages from the context (context assembly stays
 *     on the TS side until slice 3);
 *  3. runs `RustEngine.startTurn` (aimux-backed LLM in production, scripted
 *     segments under test);
 *  4. publishes the engine events on the event bus — the transcript
 *     projection layer (`coreEventMap`) folds them into wire ops exactly as
 *     it does for the TS loop — and mirrors them into the context
 *     (`step.begin` / `content.part` / `tool.call` / `tool.result` /
 *     `step.end` + the assistant message) so the next turn's history is
 *     intact.
 *
 * Slice-1 scope: single turn, no queue/cancellation/undo (those land with
 * later slices). The runner is a parallel path — `loopService` is untouched.
 */

import { randomUUID } from "node:crypto";

import { RustTurnSession } from "@dimi-agent/dimi-native";

import { IAgentContextMemoryService } from "#/agent/contextMemory/contextMemory";
import type { ContextMessage, PromptOrigin } from "#/agent/contextMemory/types";
import { IAgentLLMRequesterService } from "#/agent/llmRequester/llmRequester";
import { IAgentPermissionModeService } from "#/agent/permissionMode/permissionMode";
import { IAgentToolRegistryService } from "#/agent/toolRegistry/toolRegistry";
import { IAgentPermissionRulesService } from "#/agent/permissionRules/permissionRules";
import { cancelTurn, promptTurn, steerTurn, TurnModel } from "#/agent/loop/turnOps";
import { IEventBus } from "#/app/event/eventBus";
import { IConfigService } from "#/app/config/config";
import { ISessionApprovalService, type ApprovalResponse } from "#/session/approval/approval";
import { IAgentUsageService } from "#/agent/usage/usage";
import { IAgentProfileService } from "#/agent/profile/profile";
import { IModelCatalog } from "#/app/modelCatalog/catalog";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import { createToolMessage, type ContentPart, type ToolCall } from "#/llmProtocol/message";
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

/** Resolve once the signal aborts (used to race an approval wait). */
function abortOnSignal(signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    signal.addEventListener("abort", () => resolve(), { once: true });
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
    @IModelCatalog private readonly modelCatalog: IModelCatalog,
    @IProviderRuntime private readonly providerRuntime: IProviderRuntime,
    @IInstantiationService private readonly instantiation: IInstantiationService,
  ) {}

  static isEnabled(): boolean {
    return rustEngineEnabled();
  }

  /**
   * Steer the running turn. Mirrors the TS steer path (`steerTurn` op +
   * user message in the context) and forwards the text into the engine's
   * steer queue, where it is drained into the next LLM request.
   */
  steer(payload: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin }): boolean {
    const session = this.activeSession;
    if (session === undefined) return false;
    const text = payload.input
      .filter((part) => part.type === "text")
      .map((part) => (part as { text?: string }).text ?? "")
      .join("");
    if (text.length === 0) return false;
    this.wire.dispatch(steerTurn({ input: [...payload.input], origin: payload.origin }));
    this.context.append({
      role: "user",
      content: [...payload.input],
      toolCalls: [],
      origin: payload.origin,
      id: randomUUID(),
    });
    session.steer(text);
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
      shell: "/bin/sh",
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
      events: Array<Record<string, unknown>>;
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
    const allEvents: Array<Record<string, unknown>> = [];
    const advance = async (): Promise<EngineProgress> => {
      const next = JSON.parse(await session.run()) as EngineProgress;
      return next;
    };
    let progress: EngineProgress = await advance();
    allEvents.push(...progress.events);
    // Approval loop: surface the request, wait for the user, resume.
    while (progress.progress.status === "needsApproval") {
      const approval = progress.progress.approval!;
      const approvalRequest = {
        sessionId: "session",
        agentId: "main",
        turnId,
        toolCallId: approval.toolCallId,
        toolName: approval.toolName,
        action: approval.action ?? `Approve ${approval.toolName}`,
        display: approval.display as never,
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
      progress = JSON.parse(await session.resume(JSON.stringify(response))) as typeof progress;
      allEvents.push(...progress.events);
    }
    const batch = {
      events: allEvents,
      outcome: progress.progress.outcome ?? { status: progress.progress.status },
    };

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

    for (const event of batch.events) {
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
        default:
          break;
      }
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
