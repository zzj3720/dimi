import type { ILogger, ModelRecord, ProviderType } from '@moonshot-ai/agent-core-v2';
import type {
  ModelCapability,
  ProviderConfig as KosongProviderConfig,
  ProviderRequestAuth,
} from '@moonshot-ai/kosong';

export type Logger = ILogger;

export interface ResolvedRuntimeProvider {
  readonly providerName: string;
  readonly provider: KosongProviderConfig;
  readonly modelCapabilities: ModelCapability;
  readonly alwaysThinking?: boolean;
  readonly supportEfforts?: readonly string[];
  readonly defaultEffort?: string;
  readonly maxOutputSize?: number;
  readonly type: ProviderType;
  readonly protocol: ModelRecord['protocol'];
}

type AuthorizedRequest = <T>(request: (auth: ProviderRequestAuth) => Promise<T>) => Promise<T>;

export interface ModelProvider {
  readonly defaultModel?: string;
  resolveProviderConfig(model: string): ResolvedRuntimeProvider;
  resolveAuth?(model: string, options?: { readonly log?: Logger }): AuthorizedRequest | undefined;
}
