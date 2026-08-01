import type { ContentPart } from "@dimi-agent/agent-core-v2";
import type {
  ApprovalRequest,
  ApprovalResponse,
} from "@dimi-agent/agent-core-v2/session/approval/approval";
import type {
  QuestionRequest,
  QuestionResult,
} from "@dimi-agent/agent-core-v2/session/question/question";

export type {
  AgentStatusUpdatedEvent,
  AssistantDeltaEvent,
  CompactionBlockedEvent,
  CompactionCancelledEvent,
  CompactionCompletedEvent,
  CompactionResult,
  CompactionStartedEvent,
  CronFiredEvent,
  ErrorEvent,
  Event,
  GoalUpdatedEvent,
  HookResultEvent,
  McpOAuthAuthorizationUrlUpdateData,
  McpServerStatusEvent,
  McpServerStatusPayload,
  PluginCommandActivatedEvent,
  SessionMetaUpdatedEvent,
  SkillActivatedEvent,
  SubagentCompletedEvent,
  SubagentFailedEvent,
  SubagentSpawnedEvent,
  SubagentStartedEvent,
  SubagentSuspendedEvent,
  ThinkingDeltaEvent,
  TaskStartedEvent,
  TaskTerminatedEvent,
  ToolCallDeltaEvent,
  ToolCallStartedEvent,
  ToolInputDisplay,
  ToolListUpdatedEvent,
  ToolListUpdatedReason,
  ToolProgressEvent,
  ToolResultEvent,
  ToolUpdate,
  TurnEndedEvent,
  TurnEndReason,
  TurnStartedEvent,
  TurnStepCompletedEvent,
  TurnStepInterruptedEvent,
  TurnStepRetryingEvent,
  TurnStepStartedEvent,
  UsageStatus,
  WarningEvent,
} from "@dimi-agent/protocol";

export type {
  ApprovalDecision,
  ApprovalRequest,
  ApprovalResponse,
} from "@dimi-agent/agent-core-v2/session/approval/approval";
export type {
  QuestionAnswerMethod,
  QuestionAnswers,
  QuestionItem,
  QuestionOption,
  QuestionRequest,
  QuestionResponse,
  QuestionResult,
} from "@dimi-agent/agent-core-v2/session/question/question";

export interface ToolCallRequest {
  readonly turnId?: number;
  readonly toolCallId: string;
  readonly args: unknown;
}

export interface ToolCallResponse {
  readonly output: string | ContentPart[];
  readonly isError?: boolean;
}

export { MCP_OAUTH_AUTHORIZATION_URL_TOOL_UPDATE } from "@dimi-agent/protocol";

export type MaybePromise<T> = T | Promise<T>;

export type ApprovalHandler = (request: ApprovalRequest) => MaybePromise<ApprovalResponse>;

export type QuestionHandler = (request: QuestionRequest) => MaybePromise<QuestionResult>;
