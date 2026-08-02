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

import { RustEngine, RustTurnSession } from "@dimi-agent/dimi-native";

import { IAgentContextMemoryService } from "#/agent/contextMemory/contextMemory";
import type { ContextMessage, PromptOrigin } from "#/agent/contextMemory/types";
import { IAgentLLMRequesterService } from "#/agent/llmRequester/llmRequester";
import { IAgentPermissionModeService } from "#/agent/permissionMode/permissionMode";
import { IAgentPermissionRulesService } from "#/agent/permissionRules/permissionRules";
import { promptTurn, TurnModel } from "#/agent/loop/turnOps";
import { IEventBus } from "#/app/event/eventBus";
import { IConfigService } from "#/app/config/config";
import { ISessionApprovalService, type ApprovalResponse } from "#/session/approval/approval";
import type { ContentPart } from "#/llmProtocol/message";
import { createToolMessage, type ToolCall } from "#/llmProtocol/message";
import { emptyUsage } from "#/llmProtocol/usage";
import { IWireService } from "#/wire/wire";

import { createDecorator, IInstantiationService } from "#/_base/di/instantiation";
import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";

export const IRustEngineTurnRunner = createDecorator<IRustEngineTurnRunner>(
  "rustEngineTurnRunner",
);

export interface IRustEngineTurnRunner {
  runTurn(input: { readonly input: readonly ContentPart[]; readonly origin: PromptOrigin }): Promise<{
    readonly turnId: number;
  }>;
}

function rustEngineEnabled(): boolean {
  return process.env["DIMI_RUST_ENGINE"] === "1";
}

/** Turn an engine event batch into bus events + context records. */
export class RustEngineTurnRunner implements IRustEngineTurnRunner {
  declare readonly _serviceBrand: undefined;

  constructor(
    @IAgentContextMemoryService private readonly context: IAgentContextMemoryService,
    @IEventBus private readonly eventBus: IEventBus,
    @IWireService private readonly wire: IWireService,
    @IConfigService private readonly config: IConfigService,
    @IAgentLLMRequesterService private readonly llmRequester: IAgentLLMRequesterService,
    @IAgentPermissionModeService private readonly modeService: IAgentPermissionModeService,
    @IAgentPermissionRulesService private readonly rulesService: IAgentPermissionRulesService,
    @IInstantiationService private readonly instantiation: IInstantiationService,
  ) {}

  static isEnabled(): boolean {
    return rustEngineEnabled();
  }

  async runTurn(payload: {
    readonly input: readonly ContentPart[];
    readonly origin: PromptOrigin;
  }): Promise<{ readonly turnId: number }> {
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

    // 2. Assemble LLM messages from the context.
    const messages = this.context.get().map((message) => this.toLlmMessage(message));

    // 3. Run the Rust engine (session API: approvals pause and resume).
    const provider = this.providerConfig();
    const inputJson = JSON.stringify({
      turnId,
      messages,
      tools: [],
      provider,
      maxStepsPerTurn: this.maxStepsPerTurn() ?? null,
      cwd: this.config.get<string>("cwd"),
      shell: "/bin/sh",
    });
    const policyJson = JSON.stringify({
      mode: this.modeService.mode,
      rules: this.rulesService.rules,
      sessionApprovedPatterns: this.rulesService.sessionApprovalRulePatterns,
    });
    // Test hook: DIMI_RUST_ENGINE_SCRIPTED injects scripted LLM segments.
    const scripted = process.env["DIMI_RUST_ENGINE_SCRIPTED"];
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
    const session = new RustTurnSession(inputJson, policyJson, scripted ?? undefined);
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
      try {
        const approvalService = this.instantiation.invokeFunction(
          (accessor) => accessor.get(ISessionApprovalService) as ISessionApprovalService | undefined,
        );
        response = approvalService !== undefined ? await approvalService.request(approvalRequest) : { decision: "approved" };
      } catch {
        response = { decision: "rejected" };
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
    const stepUuid = randomUUID();
    let assistantParts: ContentPart[] = [];
    let openText = "";
    let openThinking = "";
    let usage = emptyUsage();
    let finishReason: string | undefined;
    const toolCalls: ToolCall[] = [];

    const publish = (event: Record<string, unknown>): void => {
      const { type, ...rest } = event;
      this.eventBus.publish({ type, ...rest } as never);
    };

    const flushText = (): void => {
      if (openText.length === 0) return;
      assistantParts.push({ type: "text", text: openText });
      openText = "";
    };

    const flushThinking = (): void => {
      if (openThinking.length === 0) return;
      assistantParts.push({ type: "think", think: openThinking });
      openThinking = "";
    };

    // Context mirroring: step.begin at the first step.
    let stepBegun = false;
    for (const event of batch.events) {
      publish(event);
      switch (event["type"]) {
        case "turn.step.started": {
          if (!stepBegun) {
            stepBegun = true;
            this.context.appendLoopEvent({ type: "step.begin", uuid: stepUuid, turnId: String(turnId), step: 1 });
          }
          break;
        }
        case "thinking.delta": {
          openThinking += String(event["delta"] ?? "");
          break;
        }
        case "assistant.delta": {
          openText += String(event["delta"] ?? "");
          break;
        }
        case "tool.call.delta": {
          const id = String(event["toolCallId"] ?? "");
          const name = event["name"] as string | undefined;
          const existing = toolCalls.find((call) => call.id === id);
          if (existing === undefined && name !== undefined) {
            const call: ToolCall = { type: "function", id, name, arguments: "" };
            toolCalls.push(call);
            this.context.appendLoopEvent({
              type: "tool.call",
              stepUuid,
              toolCallId: id,
              name,
              uuid: randomUUID(),
              turnId: String(turnId),
              step: 1,
            });
          }
          break;
        }
        case "tool.result": {
          const id = String(event["toolCallId"] ?? "");
          const output = String(event["output"] ?? "");
          const isError = event["isError"] === true;
          this.context.appendLoopEvent({
            type: "tool.result",
            toolCallId: id,
            result: { output, isError },
            parentUuid: stepUuid,
          });
          break;
        }
        case "turn.step.completed": {
          flushThinking();
          flushText();
          const stepFinish = String(event["finishReason"] ?? "end_turn");
          finishReason = stepFinish;
          this.context.appendLoopEvent({
            type: "step.end",
            uuid: stepUuid,
            turnId: String(turnId),
            step: 1,
            finishReason: stepFinish,
            usage,
          });
          break;
        }
        default:
          break;
      }
    }

    // 5. Append the assistant message to the context (loopEventFold folds
    //    content.part + step.end into the assistant message; the runner
    //    mirrors the same shape directly).
    flushThinking();
    flushText();
    if (assistantParts.length > 0 || toolCalls.length > 0) {
      this.context.append({
        role: "assistant",
        content: assistantParts,
        toolCalls,
        id: randomUUID(),
      } as ContextMessage);
      for (const call of toolCalls) {
        this.context.append(
          createToolMessage(call.id, `Tool "${call.name}" ran in the Rust engine.`) as ContextMessage,
        );
      }
    }

    return { turnId };
  }

  private toLlmMessage(message: ContextMessage): Record<string, unknown> {
    const text = message.content
      .filter((part) => part.type === "text")
      .map((part) => (part as { text?: string }).text ?? "")
      .join("");
    const toolCalls = message.toolCalls.map((call) => ({
      id: call.id,
      type: "function",
      function: { name: call.name, arguments: call.arguments },
    }));
    return {
      role: message.role,
      content: text,
      ...(message.role === "assistant" && toolCalls.length > 0 ? { toolCalls } : {}),
      ...(message.role === "tool" && message.toolCallId !== undefined
        ? { toolCallId: message.toolCallId }
        : {}),
    };
  }

  private maxStepsPerTurn(): number | undefined {
    return this.config.get<{ maxStepsPerTurn?: number }>("loop_control")?.maxStepsPerTurn;
  }

  private providerConfig(): Record<string, unknown> {
    // Slice 1: read the active model's provider config through the LLM
    // requester's catalog (the TS side owns provider resolution).
    const model = this.config.get<{ model?: string }>("model")?.model;
    const provider = this.config.get<{ provider?: string }>("provider")?.provider;
    const apiKey = this.config.get<{ apiKey?: string }>("apiKey")?.apiKey;
    return {
      baseUrl: this.config.get<{ baseUrl?: string }>("baseUrl")?.baseUrl ?? "https://api.openai.com/v1",
      apiKey: apiKey ?? "",
      model: model ?? "gpt-4o",
      thinkingEffort: this.config.get<{ thinkingEffort?: string }>("thinkingEffort")?.thinkingEffort,
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
