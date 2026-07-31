import type { IProviderRuntime } from "../../packages/agent-core-v2/src/app/providerRuntime/providerRuntime";
import { ProviderModels } from "../../packages/agent-core-v2/src/app/providerRuntime/models";
import { streamProvider } from "../../packages/agent-core-v2/src/app/providerRuntime/stream";
import type {
  Credential,
  CredentialInfo,
  CredentialStore,
  Model,
  ModelsStore,
  ModelsStoreEntry,
  Provider,
} from "../../packages/agent-core-v2/src/app/providerRuntime/types";

export interface TestProviderRuntimeOptions {
  readonly providerId?: string;
  readonly providerName?: string;
  readonly modelId?: string;
  readonly modelIds?: readonly string[];
  readonly baseUrl?: string;
  readonly apiKey?: string;
  readonly model?: Partial<Model>;
  readonly stream?: Provider["stream"];
}

export function createTestProviderRuntime(
  options: TestProviderRuntimeOptions = {},
): IProviderRuntime {
  const providerId = options.providerId ?? "kimi-coding";
  const modelId = options.modelId ?? options.modelIds?.[0] ?? "kimi-for-coding";
  const baseUrl = options.baseUrl ?? "https://example.test/v1";
  const apiKey = options.apiKey ?? "test-key";
  const models: readonly Model[] = (options.modelIds ?? [modelId]).map((id) => ({
    id,
    name: id,
    api: "openai-completions",
    provider: providerId,
    baseUrl,
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 32_000,
    ...options.model,
  }));
  const provider: Provider = {
    id: providerId,
    name: options.providerName ?? providerId,
    baseUrl,
    auth: {
      apiKey: {
        name: "Test API key",
        resolve: async () => ({
          auth: { apiKey, baseUrl },
          source: "test",
        }),
      },
    },
    getModels: () => models,
    stream: options.stream ?? streamProvider,
  };
  return new TestProviderRuntime(provider, apiKey);
}

class TestProviderRuntime extends ProviderModels implements IProviderRuntime {
  declare readonly _serviceBrand: undefined;
  readonly ready = Promise.resolve();

  constructor(provider: Provider, apiKey: string) {
    super([provider], testCredentialStore(apiKey), testModelsStore);
  }

  listCredentials(): Promise<readonly CredentialInfo[]> {
    const provider = this.getProviders()[0];
    return Promise.resolve(
      provider === undefined ? [] : [{ providerId: provider.id, type: "api_key" }],
    );
  }
}

function testCredentialStore(apiKey: string): CredentialStore {
  const credential: Credential = { type: "api_key", key: apiKey };
  return {
    read: async () => credential,
    list: async () => [],
    modify: async (_providerId, fn) => fn(credential),
    delete: async () => undefined,
  };
}

const testModelsStore: ModelsStore = {
  read: async () => undefined,
  write: async (_providerId: string, _entry: ModelsStoreEntry) => undefined,
  delete: async () => undefined,
};
