export type KnownApi =
  | "openai-completions"
  | "mistral-conversations"
  | "openai-responses"
  | "azure-openai-responses"
  | "openai-codex-responses"
  | "anthropic-messages"
  | "bedrock-converse-stream"
  | "google-generative-ai"
  | "google-vertex"
  | "pi-messages";
export type Api = KnownApi | (string & {});
/** The provider runtime can serialize only text and image user content. */
export type ModelInput = "text" | "image";

export type ProviderHeaders = Record<string, string | null>;
export type ProviderEnv = Record<string, string>;
export type AuthType = "api_key" | "oauth";

export interface ModelAuth {
  apiKey?: string;
  headers?: ProviderHeaders;
  baseUrl?: string;
}

export interface ApiKeyCredential {
  type: "api_key";
  key?: string;
  env?: ProviderEnv;
}

export interface OAuthCredential {
  type: "oauth";
  access: string;
  refresh: string;
  expires: number;
  /** Provider-owned values used by request and header templates after OAuth. */
  env?: ProviderEnv;
  [key: string]: unknown;
}

export type Credential = ApiKeyCredential | OAuthCredential;

export interface CredentialInfo {
  providerId: string;
  type: Credential["type"];
}

export interface CredentialStore {
  read(providerId: string): Promise<Credential | undefined>;
  list(): Promise<readonly CredentialInfo[]>;
  modify(
    providerId: string,
    fn: (current: Credential | undefined) => Promise<Credential | undefined>,
  ): Promise<Credential | undefined>;
  delete(providerId: string): Promise<void>;
}

export interface AuthContext {
  env(name: string): Promise<string | undefined>;
  fileExists(path: string): Promise<boolean>;
}

export interface AuthResult {
  auth: ModelAuth;
  env?: ProviderEnv;
  source?: string;
}

export interface AuthCheck {
  source?: string;
  type: AuthType;
}

export type AuthPrompt = { signal?: AbortSignal } & (
  | { type: "text"; message: string; placeholder?: string }
  | { type: "secret"; message: string; placeholder?: string }
  | {
      type: "select";
      message: string;
      options: readonly { id: string; label: string; description?: string }[];
    }
  | { type: "manual_code"; message: string; placeholder?: string }
);

export interface AuthInfoLink {
  url: string;
  label?: string;
}

export type AuthEvent =
  | { type: "info"; message: string; links?: readonly AuthInfoLink[] }
  | { type: "auth_url"; url: string; instructions?: string }
  | {
      type: "device_code";
      userCode: string;
      verificationUri: string;
      intervalSeconds?: number;
      expiresInSeconds?: number;
    }
  | { type: "progress"; message: string };

export interface AuthInteraction {
  signal?: AbortSignal;
  prompt(prompt: AuthPrompt): Promise<string>;
  notify(event: AuthEvent): void;
}

export interface ApiKeyAuth {
  name: string;
  login?(interaction: AuthInteraction): Promise<ApiKeyCredential>;
  check?(input: {
    ctx: AuthContext;
    credential?: ApiKeyCredential;
  }): Promise<AuthCheck | undefined>;
  resolve(input: {
    ctx: AuthContext;
    credential?: ApiKeyCredential;
  }): Promise<AuthResult | undefined>;
}

export interface OAuthAuth {
  name: string;
  loginLabel?: string;
  login(interaction: AuthInteraction): Promise<OAuthCredential>;
  refresh(credential: OAuthCredential, signal?: AbortSignal): Promise<OAuthCredential>;
  toAuth(credential: OAuthCredential): Promise<ModelAuth>;
}

export interface ModelCostTier extends ModelCost {
  inputTokensAbove: number;
}

export interface ModelCost {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  tiers?: readonly ModelCostTier[];
}

export interface Model<TApi extends Api = Api> {
  id: string;
  name: string;
  api: TApi;
  provider: string;
  baseUrl: string;
  reasoning: boolean;
  input: readonly ModelInput[];
  cost: ModelCost;
  contextWindow: number;
  maxTokens: number;
  dynamicTools?: boolean;
  headers?: ProviderHeaders;
  compat?: Readonly<Record<string, unknown>>;
  thinkingLevelMap?: Readonly<Record<string, string | number | null>>;
  defaultThinkingLevel?: string;
}

/**
 * A user-owned `models.json` provider layer. Its fields are deliberately
 * optional: an entry may overlay a built-in provider, add models to one, or
 * define a complete new provider. Credentials are never persisted here.
 */
export interface CustomProviderDefinition {
  id: string;
  name?: string;
  api?: Api;
  baseUrl?: string;
  /** A literal key, `$ENVIRONMENT_VARIABLE` template, or `!command`. */
  apiKey?: string;
  oauth?: "radius";
  authHeader?: boolean;
  headers?: ProviderHeaders;
  compat?: Record<string, unknown>;
  models?: readonly CustomModelDefinition[];
  modelOverrides?: Readonly<Record<string, CustomModelOverride>>;
}

export interface CustomModelDefinition {
  id: string;
  name?: string;
  api?: Api;
  baseUrl?: string;
  reasoning?: boolean;
  input?: readonly ModelInput[];
  contextWindow?: number;
  maxTokens?: number;
  cost?: ModelCost;
  headers?: ProviderHeaders;
  compat?: Record<string, unknown>;
  thinkingLevelMap?: Readonly<Record<string, string | number | null>>;
}

/** Pi-compatible model override: cost fields are independently overridable. */
export type CustomModelOverride = Omit<Partial<CustomModelDefinition>, "id" | "cost"> & {
  cost?: Partial<ModelCost>;
};

export interface ModelsStoreEntry {
  models: readonly Model[];
  lastModified?: number;
  checkedAt?: number;
  etag?: string;
}

export interface ModelsStore {
  read(providerId: string): Promise<ModelsStoreEntry | undefined>;
  write(providerId: string, entry: ModelsStoreEntry): Promise<void>;
  delete(providerId: string): Promise<void>;
}

export interface RefreshModelsContext {
  auth?: AuthResult;
  store: {
    read(): Promise<ModelsStoreEntry | undefined>;
    write(entry: ModelsStoreEntry): Promise<void>;
    delete(): Promise<void>;
  };
  allowNetwork: boolean;
  force?: boolean;
  signal?: AbortSignal;
}

export interface TextContent {
  type: "text";
  text: string;
}

export interface ImageContent {
  type: "image";
  mimeType?: string;
  data?: string;
  url?: string;
}

export interface ThinkingContent {
  type: "thinking";
  thinking: string;
  thinkingSignature?: string;
  itemId?: string;
  /** The provider's completed reasoning item, retained for lossless replay. */
  reasoningItem?: Readonly<Record<string, unknown>>;
}

export interface ToolCall {
  type: "toolCall";
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  argumentsRaw?: string;
  thoughtSignature?: string;
}

export interface UserMessage {
  role: "user";
  content: string | (TextContent | ImageContent)[];
  timestamp: number;
}

export interface Usage {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  reasoning?: number;
  totalTokens: number;
  cost: {
    input: number;
    output: number;
    cacheRead: number;
    cacheWrite: number;
    total: number;
  };
}

export interface AssistantMessage {
  role: "assistant";
  content: (TextContent | ThinkingContent | ToolCall)[];
  api: Api;
  provider: string;
  model: string;
  responseModel?: string;
  responseId?: string;
  traceId?: string;
  usage: Usage;
  stopReason: "pending" | "stop" | "length" | "toolUse" | "error" | "aborted";
  finishReason?: "completed" | "tool_calls" | "truncated" | "filtered" | "paused" | "other";
  errorMessage?: string;
  rawStopReason?: string;
  timestamp: number;
}

export interface ToolResultMessage {
  role: "toolResult";
  toolCallId: string;
  toolName: string;
  content: (TextContent | ImageContent)[];
  isError: boolean;
  timestamp: number;
}

export type Message = UserMessage | AssistantMessage | ToolResultMessage;

export interface Tool {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
}

export interface Context {
  systemPrompt?: string;
  messages: Message[];
  tools?: Tool[];
}

export type ModelThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

export interface ModelsSimpleStreamOptions {
  signal?: AbortSignal;
  temperature?: number;
  topP?: number;
  maxTokens?: number;
  /** A stable request key used for prompt caching and session affinity. */
  cacheKey?: string;
  /** Kept for callers which only need transport session affinity. */
  sessionId?: string;
  /** `off` is kept through transport so adapters can distinguish it from an unspecified request. */
  reasoning?: ModelThinkingLevel;
  thinkingKeep?: string;
  responseFormat?: {
    type: "text" | "json_object" | "json_schema";
    jsonSchema?: { name?: string; schema: Record<string, unknown>; strict?: boolean };
  };
  onResponse?: (response: { headers: Record<string, string> }) => void;
}

export type AssistantMessageEvent =
  | { type: "start"; partial: AssistantMessage }
  | { type: "text_delta"; delta: string; partial: AssistantMessage }
  | { type: "thinking_delta"; delta: string; partial: AssistantMessage }
  | {
      type: "toolcall_start";
      index: number | string;
      id: string;
      name: string;
      partial: AssistantMessage;
    }
  | {
      type: "toolcall_delta";
      index: number | string;
      delta: string;
      partial: AssistantMessage;
    }
  | { type: "toolcall_end"; toolCall: ToolCall; partial: AssistantMessage }
  | {
      type: "done";
      reason: "stop" | "length" | "toolUse";
      message: AssistantMessage;
    }
  | {
      type: "error";
      reason: "aborted" | "error";
      error: AssistantMessage;
      cause?: unknown;
    };

export interface ProviderStreams<TApi extends Api = Api> {
  stream(
    model: Model<TApi>,
    context: Context,
    auth: AuthResult,
    options?: ModelsSimpleStreamOptions,
  ): AsyncIterable<AssistantMessageEvent>;
}

export interface Provider<TApi extends Api = Api> extends ProviderStreams<TApi> {
  id: string;
  name: string;
  baseUrl?: string;
  headers?: ProviderHeaders;
  auth: {
    apiKey?: ApiKeyAuth;
    oauth?: OAuthAuth;
  };
  getModels(): readonly Model<TApi>[];
  filterModels?(
    models: readonly Model<TApi>[],
    credential: Credential | undefined,
  ): readonly Model<TApi>[];
  /** Recreate a provider-owned dynamic catalog with a persisted endpoint overlay. */
  withBaseUrl?(baseUrl: string): Provider<TApi>;
  refreshModels?(context: RefreshModelsContext): Promise<void>;
}

export interface ModelsRefreshOptions {
  provider?: string;
  allowNetwork?: boolean;
  force?: boolean;
  signal?: AbortSignal;
}

export interface ModelsRefreshResult {
  aborted: boolean;
  errors: ReadonlyMap<string, Error>;
}

export interface Models {
  getProviders(): readonly Provider[];
  getProvider(id: string): Provider | undefined;
  getModels(provider?: string): readonly Model[];
  getModel(provider: string, id: string): Model | undefined;
  refresh(options?: ModelsRefreshOptions): Promise<ModelsRefreshResult>;
  checkAuth(providerId: string): Promise<AuthCheck | undefined>;
  getAvailable(providerId?: string): Promise<readonly Model[]>;
  getAuth(providerId: string): Promise<AuthResult | undefined>;
  getAuth(model: Model): Promise<AuthResult | undefined>;
  login(providerId: string, type: AuthType, interaction: AuthInteraction): Promise<Credential>;
  logout(providerId: string): Promise<void>;
  streamSimple(
    model: Model,
    context: Context,
    options?: ModelsSimpleStreamOptions,
  ): AsyncIterable<AssistantMessageEvent>;
  completeSimple(
    model: Model,
    context: Context,
    options?: ModelsSimpleStreamOptions,
  ): Promise<AssistantMessage>;
}

export interface MutableModels extends Models {
  setProvider(provider: Provider): void;
  deleteProvider(id: string): void;
  clearProviders(): void;
}
