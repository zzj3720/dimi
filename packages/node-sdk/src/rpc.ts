import { AsyncLocalStorage } from "node:async_hooks";

import type {
  AgentContextData,
  ExperimentalFeatureState,
  SwarmModeTrigger,
} from "@dimi-agent/agent-core-v2";
import type { Event } from "@dimi-agent/protocol";

import type {
  ApprovalHandler,
  ApprovalRequest,
  ApprovalResponse,
  QuestionHandler,
  QuestionRequest,
  QuestionResult,
  ToolCallRequest,
  ToolCallResponse,
} from "#/events";
import type {
  AddAdditionalDirInput,
  AddAdditionalDirResult,
  BackgroundTaskInfo,
  CompactOptions,
  ConfigDiagnostics,
  CreateGoalInput,
  CreateSessionOptions,
  ExportSessionInput,
  ExportSessionResult,
  ForkSessionInput,
  GetConfigOptions,
  GetCronTasksResult,
  GoalSnapshot,
  GoalToolResult,
  JsonObject,
  DimiConfig,
  DimiConfigPatch,
  ListSessionsOptions,
  McpServerConfig,
  McpServerInfo,
  McpStartupMetrics,
  McpTestResult,
  PermissionMode,
  PluginCommandDef,
  PluginInfo,
  PluginSummary,
  PromptInput,
  ReloadSummary,
  RenameSessionInput,
  ResumedSessionSummary,
  ResumeSessionInput,
  SessionPlan,
  SessionStatus,
  SessionSummary,
  SessionUsage,
  SkillSummary,
  Unsubscribe,
} from "#/types";
import { ErrorCodes, DimiError } from "#/errors";

const MAIN_AGENT_ID = "main";

export interface SessionPromptRpcInput {
  readonly sessionId: string;
  readonly input: PromptInput;
  readonly disabledTools?: readonly string[];
}

export interface SessionIdRpcInput {
  readonly sessionId: string;
}

export interface ImportContextRpcInput extends SessionIdRpcInput {
  readonly content: string;
  readonly source: string;
}

export interface ReloadSessionRpcInput extends SessionIdRpcInput {
  readonly forcePluginSessionStartReminder?: boolean;
}

export interface SetSessionModelRpcInput extends SessionIdRpcInput {
  readonly model: string;
}

export interface SetSessionModelRpcResult {
  readonly model: string;
  readonly providerName?: string;
}

export interface SetSessionThinkingRpcInput extends SessionIdRpcInput {
  readonly effort: string;
}

export interface SetSessionPermissionRpcInput extends SessionIdRpcInput {
  readonly mode: PermissionMode;
}

export interface UpdateSessionMetadataRpcInput extends SessionIdRpcInput {
  readonly metadata: JsonObject;
}

export interface SetSessionPlanModeRpcInput extends SessionIdRpcInput {
  readonly enabled: boolean;
}

export type SetSessionSwarmModeRpcInput =
  | (SessionIdRpcInput & { readonly enabled: true; readonly trigger: SwarmModeTrigger })
  | (SessionIdRpcInput & { readonly enabled: false });

export interface ActivateSkillRpcInput extends SessionIdRpcInput {
  readonly name: string;
  readonly args?: string;
}

export interface ActivatePluginCommandRpcInput extends SessionIdRpcInput {
  readonly pluginId: string;
  readonly commandName: string;
  readonly args?: string;
}

export interface ReconnectMcpServerRpcInput extends SessionIdRpcInput {
  readonly name: string;
}

export type BeginGlobalMcpServerAuthResult =
  | { readonly status: "already-authorized" }
  | {
      readonly status: "authorization-required";
      readonly flowId: string;
      readonly authorizationUrl: string;
    };

// The interface is the transport contract implemented by concrete clients;
// the class below owns only shared handler behavior.
// oxlint-disable-next-line typescript-eslint/no-unsafe-declaration-merging
export interface SDKRpcClientBase {
  createSession(input: CreateSessionOptions): Promise<SessionSummary>;
  resumeSession(input: ResumeSessionInput): Promise<ResumedSessionSummary>;
  reloadSession(input: ReloadSessionRpcInput): Promise<ResumedSessionSummary>;
  forkSession(input: ForkSessionInput): Promise<SessionSummary>;
  closeSession(input: SessionIdRpcInput): Promise<void>;
  deleteSession(input: SessionIdRpcInput): Promise<void>;
  listSessions(input?: ListSessionsOptions): Promise<readonly SessionSummary[]>;
  listWorkspaceSkills(workDir: string): Promise<readonly SkillSummary[]>;
  renameSession(input: RenameSessionInput): Promise<void>;
  exportSession(input: ExportSessionInput): Promise<ExportSessionResult>;
  getConfig(input?: GetConfigOptions): Promise<DimiConfig>;
  getConfigDiagnostics(): Promise<ConfigDiagnostics>;
  getExperimentalFeatures(): Promise<readonly ExperimentalFeatureState[]>;
  setConfig(input: DimiConfigPatch): Promise<DimiConfig>;
  listGlobalMcpServers(): Promise<readonly McpServerConfig[]>;
  addGlobalMcpServer(server: McpServerConfig): Promise<readonly McpServerConfig[]>;
  updateGlobalMcpServer(server: McpServerConfig): Promise<readonly McpServerConfig[]>;
  removeGlobalMcpServer(name: string): Promise<readonly McpServerConfig[]>;
  beginGlobalMcpServerAuth(name: string): Promise<BeginGlobalMcpServerAuthResult>;
  completeGlobalMcpServerAuth(
    input: { readonly flowId: string; readonly timeoutMs?: number },
    signal?: AbortSignal,
  ): Promise<void>;
  cancelGlobalMcpServerAuth(flowId: string): Promise<void>;
  resetGlobalMcpServerAuth(name: string): Promise<void>;
  testGlobalMcpServer(name: string, options?: { readonly cwd?: string }): Promise<McpTestResult>;
  prompt(input: SessionPromptRpcInput): Promise<void>;
  runShellCommand(input: {
    readonly sessionId: string;
    readonly command: string;
    readonly commandId?: string;
  }): Promise<{ stdout: string; stderr: string; isError?: boolean; backgrounded?: boolean }>;
  cancelShellCommand(input: {
    readonly sessionId: string;
    readonly commandId: string;
  }): Promise<void>;
  steer(input: SessionPromptRpcInput): Promise<void>;
  generateAgentsMd(input: SessionIdRpcInput): Promise<void>;
  getSessionWarnings(
    input: SessionIdRpcInput,
  ): Promise<readonly { code: string; message: string; severity: "info" | "warning" | "error" }[]>;
  addAdditionalDir(input: AddAdditionalDirInput): Promise<AddAdditionalDirResult>;
  startBtw(input: SessionIdRpcInput): Promise<string>;
  cancel(input: SessionIdRpcInput): Promise<void>;
  clearContext(input: SessionIdRpcInput): Promise<void>;
  importContext(input: ImportContextRpcInput): Promise<void>;
  setModel(input: SetSessionModelRpcInput): Promise<SetSessionModelRpcResult>;
  setThinking(input: SetSessionThinkingRpcInput): Promise<void>;
  applyPersistedSecondaryModel(input: SessionIdRpcInput): Promise<void>;
  setPermission(input: SetSessionPermissionRpcInput): Promise<void>;
  updateSessionMetadata(input: UpdateSessionMetadataRpcInput): Promise<void>;
  setPlanMode(input: SetSessionPlanModeRpcInput): Promise<void>;
  setSwarmMode(input: SetSessionSwarmModeRpcInput): Promise<void>;
  swarm(input: SessionPromptRpcInput): Promise<void>;
  getPlan(input: SessionIdRpcInput): Promise<SessionPlan>;
  clearPlan(input: SessionIdRpcInput): Promise<void>;
  compact(input: SessionIdRpcInput & CompactOptions): Promise<void>;
  cancelCompaction(input: SessionIdRpcInput): Promise<void>;
  undoHistory(input: SessionIdRpcInput & { readonly count: number }): Promise<void>;
  getContext(input: SessionIdRpcInput): Promise<AgentContextData>;
  getUsage(input: SessionIdRpcInput): Promise<SessionUsage>;
  getStatus(input: SessionIdRpcInput): Promise<SessionStatus>;
  listSkills(input: SessionIdRpcInput): Promise<readonly SkillSummary[]>;
  listPluginCommands(input: SessionIdRpcInput): Promise<readonly PluginCommandDef[]>;
  listBackgroundTasks(
    input: SessionIdRpcInput & { readonly activeOnly?: boolean; readonly limit?: number },
  ): Promise<readonly BackgroundTaskInfo[]>;
  getBackgroundTaskOutput(
    input: SessionIdRpcInput & { readonly taskId: string; readonly tail?: number },
  ): Promise<string>;
  stopBackgroundTask(
    input: SessionIdRpcInput & { readonly taskId: string; readonly reason?: string },
  ): Promise<void>;
  detachBackgroundTask(
    input: SessionIdRpcInput & { readonly taskId: string },
  ): Promise<BackgroundTaskInfo | undefined>;
  waitForBackgroundTasksOnPrint(input: SessionIdRpcInput): Promise<void>;
  handlePrintMainTurnCompleted(input: SessionIdRpcInput): Promise<"finish" | "continue">;
  createGoal(input: SessionIdRpcInput & CreateGoalInput): Promise<GoalSnapshot>;
  getGoal(input: SessionIdRpcInput): Promise<GoalToolResult>;
  pauseGoal(input: SessionIdRpcInput): Promise<GoalSnapshot>;
  resumeGoal(input: SessionIdRpcInput): Promise<GoalSnapshot>;
  cancelGoal(input: SessionIdRpcInput): Promise<GoalSnapshot>;
  getCronTasks(input: SessionIdRpcInput): Promise<GetCronTasksResult>;
  listMcpServers(input: SessionIdRpcInput): Promise<readonly McpServerInfo[]>;
  getMcpStartupMetrics(input: SessionIdRpcInput): Promise<McpStartupMetrics>;
  reconnectMcpServer(input: ReconnectMcpServerRpcInput): Promise<void>;
  listPlugins(): Promise<readonly PluginSummary[]>;
  installPlugin(source: string): Promise<PluginSummary>;
  setPluginEnabled(id: string, enabled: boolean): Promise<void>;
  setPluginMcpServerEnabled(id: string, server: string, enabled: boolean): Promise<void>;
  removePlugin(id: string): Promise<void>;
  reloadPlugins(): Promise<ReloadSummary>;
  getPluginInfo(id: string): Promise<PluginInfo>;
  activateSkill(input: ActivateSkillRpcInput): Promise<void>;
  activatePluginCommand(input: ActivatePluginCommandRpcInput): Promise<void>;
}

// See the contract above. Declaration merging keeps the public base class
// extensible without stub implementations for every transport operation.
// oxlint-disable-next-line typescript-eslint/no-unsafe-declaration-merging
export class SDKRpcClientBase {
  private readonly interactiveAgentScope = new AsyncLocalStorage<string>();
  private readonly eventListeners = new Set<(event: Event) => void>();
  private readonly approvalHandlers = new Map<string, ApprovalHandler>();
  private readonly questionHandlers = new Map<string, QuestionHandler>();

  get interactiveAgentId(): string {
    return this.interactiveAgentScope.getStore() ?? MAIN_AGENT_ID;
  }

  withInteractiveAgent<T>(agentId: string, fn: () => T): T {
    return this.interactiveAgentScope.run(agentId, fn);
  }

  deleteSession(_input: SessionIdRpcInput): Promise<void> {
    return Promise.reject(
      new DimiError(ErrorCodes.NOT_IMPLEMENTED, "Session deletion is not supported."),
    );
  }

  onEvent(listener: (event: Event) => void): Unsubscribe {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  receiveEvent(event: Event): void {
    for (const listener of this.eventListeners) listener(event);
  }

  setApprovalHandler(sessionId: string, handler: ApprovalHandler | undefined): void {
    if (handler === undefined) this.approvalHandlers.delete(sessionId);
    else this.approvalHandlers.set(sessionId, handler);
  }

  setQuestionHandler(sessionId: string, handler: QuestionHandler | undefined): void {
    if (handler === undefined) this.questionHandlers.delete(sessionId);
    else this.questionHandlers.set(sessionId, handler);
  }

  clearSessionHandlers(sessionId: string): void {
    this.approvalHandlers.delete(sessionId);
    this.questionHandlers.delete(sessionId);
  }

  async requestApproval(
    request: ApprovalRequest & { readonly sessionId: string; readonly agentId: string },
  ): Promise<ApprovalResponse> {
    const handler = this.approvalHandlers.get(request.sessionId);
    if (handler === undefined) {
      return { decision: "cancelled", feedback: "No approval handler registered." };
    }
    try {
      return await handler(request);
    } catch (error) {
      this.emitHandlerError(request, "session.approval_handler_error", error);
      return { decision: "cancelled", feedback: "Approval handler failed." };
    }
  }

  async requestQuestion(
    request: QuestionRequest & { readonly sessionId: string; readonly agentId: string },
  ): Promise<QuestionResult> {
    const handler = this.questionHandlers.get(request.sessionId);
    if (handler === undefined) return null;
    try {
      return await handler(request);
    } catch (error) {
      this.emitHandlerError(request, "session.question_handler_error", error);
      return null;
    }
  }

  async toolCall(request: ToolCallRequest): Promise<ToolCallResponse> {
    return {
      output: `SDK custom tool calls are not supported: ${request.toolCallId}`,
      isError: true,
    };
  }

  private emitHandlerError(
    request: { readonly sessionId: string; readonly agentId: string },
    code: "session.approval_handler_error" | "session.question_handler_error",
    error: unknown,
  ): void {
    this.receiveEvent({
      type: "error",
      sessionId: request.sessionId,
      agentId: request.agentId,
      code,
      message: error instanceof Error ? error.message : String(error),
      retryable: false,
    });
  }
}

interface SDKAPI {
  emitEvent(event: Event): void;
  requestApproval(
    request: ApprovalRequest & { readonly sessionId: string; readonly agentId: string },
  ): Promise<ApprovalResponse>;
  requestQuestion(
    request: QuestionRequest & { readonly sessionId: string; readonly agentId: string },
  ): Promise<QuestionResult>;
  toolCall(request: ToolCallRequest): Promise<ToolCallResponse>;
}

export class ClientAPI implements SDKAPI {
  constructor(readonly client: SDKRpcClientBase) {}

  emitEvent(event: Event): void {
    this.client.receiveEvent(event);
  }

  requestApproval(
    request: ApprovalRequest & { readonly sessionId: string; readonly agentId: string },
  ): Promise<ApprovalResponse> {
    return this.client.requestApproval(request);
  }

  requestQuestion(
    request: QuestionRequest & { readonly sessionId: string; readonly agentId: string },
  ): Promise<QuestionResult> {
    return this.client.requestQuestion(request);
  }

  toolCall(request: ToolCallRequest): Promise<ToolCallResponse> {
    return this.client.toolCall(request);
  }
}
