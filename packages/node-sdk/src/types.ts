import type {
  AgentReplayRecord,
  AgentTaskConfig,
  AgentTaskInfo,
  AgentTaskStatus,
  ContextMessage,
  ExperimentalFeatureState,
  ExperimentalFlagMap,
  ExperimentalFlagSource,
  ExportSessionManifest,
  GoalBudgetLimits,
  GoalBudgetReport,
  GoalChange,
  GoalChangeStats,
  GoalSnapshot,
  GoalStatus,
  GoalToolResult,
  IProviderRuntime,
  ITelemetryAppender,
  LoopControl,
  McpServerConfig as RuntimeMcpServerConfig,
  MoonshotServiceConfig,
  PluginCommandDef,
  PluginGithubMetadata,
  PluginGithubRef,
  PluginInfo,
  PluginMcpServerInfo,
  PluginSource,
  PluginSummary,
  PromptOrigin,
  ReloadSummary,
  ResumeSessionResult,
  ResumedAgentState,
  SecondaryModelConfig,
  ServicesConfig,
  ShellEnvironment,
  SkillSummary,
  TelemetryContextPatch,
  TelemetryProperties,
  ThinkingConfig,
  ToolInfo,
} from "@moonshot-ai/agent-core-v2";
import type {
  ConfigDiagnostics,
  McpServerInfo,
  McpStartupMetrics,
} from "@moonshot-ai/agent-core-v2/agent/rpc/core-api";
import type { Kaos } from "@moonshot-ai/kaos";
import type { KimiHostIdentity } from "@moonshot-ai/kimi-code-oauth";
import type { ContentPart } from "@moonshot-ai/agent-core-v2";

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { readonly [key: string]: JsonValue };
export type JsonObject = { readonly [key: string]: JsonValue };

export type Unsubscribe = () => void;

export type {
  AgentReplayRecord,
  ConfigDiagnostics,
  ContextMessage,
  ExperimentalFeatureState,
  ExperimentalFlagMap,
  ExperimentalFlagSource,
  ExportSessionManifest,
  GoalBudgetLimits,
  GoalBudgetReport,
  GoalChange,
  GoalChangeStats,
  GoalSnapshot,
  GoalStatus,
  GoalToolResult,
  LoopControl,
  McpServerInfo,
  McpStartupMetrics,
  MoonshotServiceConfig,
  PluginCommandDef,
  PluginGithubMetadata,
  PluginGithubRef,
  PluginInfo,
  PluginMcpServerInfo,
  PluginSource,
  PluginSummary,
  PromptOrigin,
  ReloadSummary,
  ResumedAgentState,
  ServicesConfig,
  ShellEnvironment,
  SkillSummary,
  ThinkingConfig,
  ToolInfo,
};

export type TelemetryClient = ITelemetryAppender;
export type BackgroundConfig = AgentTaskConfig;
export type BackgroundTaskInfo = AgentTaskInfo;
export type BackgroundTaskStatus = AgentTaskStatus;
export type AgentBackgroundTaskInfo = Extract<AgentTaskInfo, { kind: "agent" }>;
export type ProcessBackgroundTaskInfo = Extract<AgentTaskInfo, { kind: "process" }>;
export type QuestionBackgroundTaskInfo = Extract<AgentTaskInfo, { kind: "question" }>;
export type ToolBackgroundTaskInfo = Extract<AgentTaskInfo, { kind: "tool" }>;
/** UI/host projection of a runtime model; it is not persisted in config.toml. */
export interface ModelAlias {
  readonly provider: string;
  readonly model: string;
  readonly displayName?: string;
  readonly maxContextSize: number;
  readonly maxOutputSize?: number;
  readonly capabilities?: readonly string[];
  readonly supportEfforts?: readonly string[];
  readonly defaultEffort?: string;
}

export interface KimiConfig {
  defaultProvider?: string;
  defaultModel?: string;
  secondaryModel?: SecondaryModelConfig;
  services?: ServicesConfig;
  thinking?: ThinkingConfig;
  telemetry?: boolean;
  readonly [domain: string]: unknown;
}

export type KimiConfigPatch = Partial<KimiConfig>;

export type SessionMcpServerConfig = RuntimeMcpServerConfig;
export type McpServerConfig = SessionMcpServerConfig & { readonly name: string };

export interface McpTestResult {
  readonly success: boolean;
  readonly output: string;
}

export interface CronTaskSnapshot {
  readonly id: string;
  readonly cron: string;
  readonly recurring: boolean;
  readonly createdAt: number;
  readonly lastFiredAt?: number;
  readonly nextFireAt?: number;
}

export interface GetCronTasksResult {
  readonly tasks: readonly CronTaskSnapshot[];
}

export type { KimiHostIdentity };
export type { TelemetryContextPatch, TelemetryProperties };
export type { ContentPart, Role, ThinkingEffort, ToolCall } from "@moonshot-ai/agent-core-v2";

export type PermissionMode = "yolo" | "manual" | "auto";

export interface CreateGoalInput {
  readonly objective: string;
  readonly replace?: boolean;
}

export type TextPromptPart = Extract<ContentPart, { type: "text" }>;
export type PromptPart = Extract<ContentPart, { type: "text" | "image_url" | "video_url" }>;

export type PromptInput = readonly PromptPart[];

export interface KimiHarnessOptions {
  readonly identity?: KimiHostIdentity | undefined;
  readonly homeDir?: string | undefined;
  readonly configPath?: string | undefined;
  readonly autoLoadConfig?: boolean | undefined;
  readonly uiMode?: string;
  readonly skillDirs?: readonly string[];
  readonly telemetry?: TelemetryClient | undefined;
  readonly sessionStartedProperties?: TelemetryProperties;
  /** Replaces the built-in provider runtime for custom hosts and deterministic tests. */
  readonly providerRuntime?: IProviderRuntime;
}

export interface CreateSessionOptions {
  readonly id?: string | undefined;
  readonly workDir: string;
  readonly model?: string | undefined;
  readonly thinking?: string | undefined;
  readonly permission?: PermissionMode | undefined;
  readonly planMode?: boolean;
  readonly metadata?: JsonObject | undefined;
  readonly kaos?: Kaos | undefined;
  readonly persistenceKaos?: Kaos | undefined;
  readonly additionalDirs?: readonly string[];
  readonly mcpServers?: Readonly<Record<string, SessionMcpServerConfig>>;
  /**
   * Main-agent profile name (`--agent`): a builtin profile or one defined by
   * an agentfile discovered from the user/project agent directories.
   */
  readonly agentProfile?: string;
  /**
   * Explicit agentfiles (`--agent-file`) loaded for this session with the
   * highest precedence; an invalid file fails session creation.
   */
  readonly agentFiles?: readonly string[];
  readonly sessionStartedProperties?: TelemetryProperties;
  /**
   * Print-mode (`kimi -p`) only: when the main agent ends a turn while
   * background subagents (`kind === 'agent'`) are still running, hold the turn
   * open and idle-wait until they all finish, flushing their completions into
   * the turn so the model can react before the run exits. Ignored by
   * interactive / SDK sessions.
   */
  readonly drainAgentTasksOnStop?: boolean;
}

export interface RenameSessionInput {
  readonly id: string;
  readonly title: string;
}

export interface ResumeSessionInput {
  readonly id: string;
  readonly kaos?: Kaos | undefined;
  readonly persistenceKaos?: Kaos | undefined;
  readonly additionalDirs?: readonly string[];
  readonly mcpServers?: Readonly<Record<string, SessionMcpServerConfig>>;
  /** Re-select the session's already-bound main profile; a different name fails. */
  readonly agentProfile?: string;
  /** Include persisted subagent states in the returned replay snapshot. */
  readonly includeSubagents?: boolean;
  /**
   * Limit each returned agent replay to the most recent N user turns. Omit to
   * return the full replay. Lets UI callers that only render the tail avoid
   * transferring the entire history over the RPC boundary.
   */
  readonly replayTurnLimit?: number;
  readonly sessionStartedProperties?: TelemetryProperties;
}

export interface ReloadSessionInput extends ResumeSessionInput {
  readonly forcePluginSessionStartReminder?: boolean;
}

export interface AddAdditionalDirInput {
  readonly id: string;
  readonly path: string;
  readonly persist: boolean;
}

export interface AddAdditionalDirOptions {
  /** When true, share the directory through workspace local config. When false,
   * keep it scoped to this session while still restoring it on session resume. */
  readonly persist: boolean;
}

export interface ForkSessionInput {
  readonly id: string;
  readonly forkId?: string;
  readonly title?: string;
  readonly metadata?: JsonObject;
  /**
   * Zero-based index of the user-visible turn to retain through. Omit it to
   * preserve the existing full-session fork behavior.
   */
  readonly turnIndex?: number;
}

export interface ExportSessionInput {
  readonly id: string;
  readonly outputPath?: string | undefined;
  readonly includeGlobalLog?: boolean | undefined;
  /** Host version to record in the export manifest. */
  readonly version: string;
  /** How the CLI was installed (e.g. 'npm-global', 'native'). */
  readonly installSource?: string | undefined;
  readonly shellEnv?: ShellEnvironment | undefined;
}

export interface ExportSessionResult {
  readonly zipPath: string;
  readonly entries: readonly string[];
  readonly sessionDir: string;
  readonly manifest: ExportSessionManifest;
}

export interface ListSessionsOptions {
  readonly workDir?: string;
  readonly sessionId?: string;
}

export interface GetConfigOptions {
  readonly reload?: boolean | undefined;
}

export interface AuthenticateMcpServerOptions {
  readonly onAuthorizationUrl: (url: string) => void | boolean | PromiseLike<void | boolean>;
  readonly signal?: AbortSignal;
  readonly timeoutMs?: number;
}

export interface TestMcpServerOptions {
  readonly cwd?: string;
}

export interface CompactOptions {
  readonly instruction?: string | undefined;
}

export interface ReloadSessionOptions {
  readonly forcePluginSessionStartReminder?: boolean;
}

export interface PlanInfo {
  readonly id: string;
  readonly content: string;
  readonly path: string;
}

export type SessionPlan = PlanInfo | null;

export interface TokenUsage {
  readonly inputOther: number;
  readonly output: number;
  readonly inputCacheRead: number;
  readonly inputCacheCreation: number;
}

export interface SessionUsage {
  readonly byModel?: Record<string, TokenUsage> | undefined;
  readonly currentTurn?: TokenUsage | undefined;
  readonly total?: TokenUsage | undefined;
}

export interface SessionStatus {
  readonly model?: string;
  readonly thinkingEffort: string;
  readonly permission: PermissionMode;
  readonly planMode: boolean;
  readonly swarmMode?: boolean | undefined;
  readonly contextTokens: number;
  readonly maxContextTokens: number;
  readonly contextUsage: number;
  readonly usage?: SessionUsage;
}

export interface SessionSummary {
  readonly id: string;
  readonly title?: string | undefined;
  readonly lastPrompt?: string;
  readonly workDir: string;
  readonly sessionDir: string;
  readonly createdAt: number;
  readonly updatedAt: number;
  readonly archived?: boolean | undefined;
  readonly metadata?: JsonObject | undefined;
  readonly additionalDirs?: readonly string[];
}

export interface AddAdditionalDirResult {
  readonly additionalDirs: readonly string[];
  readonly projectRoot: string;
  readonly configPath: string;
  readonly persisted: boolean;
}

export type ResumedSessionState = Pick<
  ResumeSessionResult,
  "sessionMetadata" | "agents" | "warning"
>;

export interface ResumedSessionSummary extends SessionSummary, ResumedSessionState {}
