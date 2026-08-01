import { createDecorator } from "#/_base/di/instantiation";
import type { FinishReason, ThinkingEffort } from "#/llmProtocol/provider";
import type { Message, StreamedMessagePart } from "#/llmProtocol/message";
import type { Tool } from "#/llmProtocol/tool";
import type { TokenUsage } from "#/llmProtocol/usage";
import type { LLMRequestTrace } from "#/llmProtocol/requestTrace";
import type { ModelRequestTiming } from "#/app/modelCatalog/modelRequester";
import type { LogContext } from "#/_base/log/log";

export type AgentLLMRequestLogFields = Readonly<LogContext>;

export type AgentLLMRequestSource =
  | {
      readonly type: "turn";
      readonly turnId: number;
      readonly step?: number;
      readonly logFields?: AgentLLMRequestLogFields;
    }
  | {
      readonly type: "operation";
      readonly turnId?: number;
      readonly requestKind?: string;
      readonly logFields?: AgentLLMRequestLogFields;
    };

export interface AgentLLMRequestFinish {
  message: Message;
  usage: TokenUsage;
  model?: string | undefined;
  providerFinishReason?: FinishReason;
  rawFinishReason?: string;
  providerMessageId?: string;
  timing?: ModelRequestTiming;
  /** Trace id of the request that produced this finish (Dimi `x-trace-id`). */
  traceId?: string;
}

export type AgentLLMRequestPartHandler = (part: StreamedMessagePart) => void | Promise<void>;

export interface AgentLLMRequestOverrides {
  messages?: readonly Message[];
  tools?: readonly Tool[];
  systemPrompt?: string;
  source?: AgentLLMRequestSource;
  maxOutputSize?: number;
}

export interface AgentLLMRequestTask {
  readonly trace: LLMRequestTrace;
  readonly result: Promise<AgentLLMRequestFinish>;
}

export interface PreparedTurnRequestConfig {
  readonly thinkingEffort: ThinkingEffort;
}

export interface IAgentLLMRequesterService {
  readonly _serviceBrand: undefined;

  prepareTurnConfig(turnId: number): PreparedTurnRequestConfig | undefined;

  request(
    overrides?: AgentLLMRequestOverrides,
    onPart?: AgentLLMRequestPartHandler,
    signal?: AbortSignal,
  ): Promise<AgentLLMRequestFinish>;

  start(
    overrides?: AgentLLMRequestOverrides,
    onPart?: AgentLLMRequestPartHandler,
    signal?: AbortSignal,
  ): AgentLLMRequestTask;
}

export const IAgentLLMRequesterService = createDecorator<IAgentLLMRequesterService>(
  "agentLLMRequesterService",
);
