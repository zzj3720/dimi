export type AgentTaskStatus =
  | 'running'
  | 'completed'
  | 'failed'
  | 'timed_out'
  | 'killed'
  | 'lost';

export const TERMINAL_STATUSES: ReadonlySet<AgentTaskStatus> = new Set<AgentTaskStatus>([
  'completed',
  'failed',
  'timed_out',
  'killed',
  'lost',
]);
export type AgentTaskSettlementStatus = 'completed' | 'failed' | 'timed_out' | 'killed';

export interface AgentTaskSettlement {
  readonly status: AgentTaskSettlementStatus;
  readonly stopReason?: string;
}

export interface AgentTaskInfoBase {
  readonly taskId: string;
  readonly description: string;
  readonly status: AgentTaskStatus;
  readonly detached?: boolean;
  readonly startedAt: number;
  readonly endedAt: number | null;
  readonly stopReason?: string;
  readonly terminalNotificationSuppressed?: boolean;
  readonly timeoutMs?: number;
}

export interface ToolAgentTaskInfo extends AgentTaskInfoBase {
  readonly kind: 'tool';
  readonly turnId: number;
  readonly toolCallId: string;
  readonly toolName: string;
  readonly autoWaitTimeoutSeconds: number;
}

export interface AgentTaskInfoByKind {
  readonly tool: ToolAgentTaskInfo;
}

export type AgentTaskKind = Extract<keyof AgentTaskInfoByKind, string>;

export type AgentTaskInfo = AgentTaskInfoByKind[AgentTaskKind];

export interface AgentTaskSink {
  readonly signal: AbortSignal;
  appendOutput(chunk: string): void;
  settle(settlement: AgentTaskSettlement): Promise<boolean>;
}

export interface AgentTaskInput {
  readonly data: string;
  readonly close?: boolean;
}

export type AgentTaskInputResult = { readonly ok: true } | { readonly ok: false; readonly error: string };

export interface AgentTask {
  readonly idPrefix: string;
  readonly kind: AgentTaskKind;
  readonly description: string;
  readonly timeoutMs?: number;

  start(sink: AgentTaskSink): void | Promise<void>;
  sendInput?(input: AgentTaskInput): Promise<AgentTaskInputResult>;
  onDetach?(): void;
  forceStop?(): Promise<void>;
  toInfo(base: AgentTaskInfoBase): AgentTaskInfo;
}
