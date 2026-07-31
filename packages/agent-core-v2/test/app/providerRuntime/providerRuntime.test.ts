import { describe, expect, it, vi } from "vitest";

import { envApiKeyAuth } from "#/app/providerRuntime/auth";
import { createProvider, hasApi } from "#/app/providerRuntime/createProvider";
import { createModels, ProviderModels } from "#/app/providerRuntime/models";
import type {
  AssistantMessage,
  Credential,
  CredentialInfo,
  CredentialStore,
  Model,
  ModelsStore,
  ModelsStoreEntry,
  Provider,
} from "#/app/providerRuntime/types";

const model = (provider: string, id: string): Model => ({
  id,
  name: id,
  api: "openai-completions",
  provider,
  baseUrl: "https://api.example.test/v1",
  reasoning: false,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 32_000,
});

class MemoryCredentialStore implements CredentialStore {
  private readonly values = new Map<string, Credential>();

  read(providerId: string): Promise<Credential | undefined> {
    return Promise.resolve(this.values.get(providerId));
  }

  list(): Promise<readonly CredentialInfo[]> {
    return Promise.resolve(
      [...this.values].map(([providerId, credential]) => ({
        providerId,
        type: credential.type,
      })),
    );
  }

  async modify(
    providerId: string,
    fn: (current: Credential | undefined) => Promise<Credential | undefined>,
  ): Promise<Credential | undefined> {
    const next = await fn(this.values.get(providerId));
    if (next !== undefined) this.values.set(providerId, next);
    return next ?? this.values.get(providerId);
  }

  delete(providerId: string): Promise<void> {
    this.values.delete(providerId);
    return Promise.resolve();
  }
}

class MemoryModelsStore implements ModelsStore {
  private readonly values = new Map<string, ModelsStoreEntry>();

  read(providerId: string): Promise<ModelsStoreEntry | undefined> {
    return Promise.resolve(this.values.get(providerId));
  }

  write(providerId: string, entry: ModelsStoreEntry): Promise<void> {
    this.values.set(providerId, entry);
    return Promise.resolve();
  }

  delete(providerId: string): Promise<void> {
    this.values.delete(providerId);
    return Promise.resolve();
  }
}

function provider(id: string, refreshModels?: Provider["refreshModels"]): Provider {
  const configuredModel = model(id, `${id}-model`);
  return {
    id,
    name: id,
    baseUrl: configuredModel.baseUrl,
    auth: { apiKey: envApiKeyAuth(`${id} key`, [`${id.toUpperCase()}_API_KEY`]) },
    getModels: () => [configuredModel],
    refreshModels,
    stream: async function* () {},
  };
}

describe("ProviderModels", () => {
  it("supports providers whose API id is unknown to the core runtime", () => {
    const custom = model("custom", "custom-model") as Model<"vendor-chat-v1">;
    custom.api = "vendor-chat-v1";
    const runtime = createModels({
      providers: [
        createProvider({
          id: "custom",
          baseUrl: custom.baseUrl,
          auth: { apiKey: envApiKeyAuth("custom key", ["CUSTOM_API_KEY"]) },
          models: [custom],
          stream: async function* () {},
        }),
      ],
    });

    const found = runtime.getModel("custom", "custom-model");
    expect(found).toBeDefined();
    expect(found !== undefined && hasApi(found, "vendor-chat-v1")).toBe(true);
  });

  it("restores and replaces dynamic model overlays through the provider factory", async () => {
    const catalogs = new MemoryModelsStore();
    await catalogs.write("dynamic", {
      models: [model("dynamic", "cached"), model("other", "ignored")],
      checkedAt: 1,
    });
    let release!: () => void;
    const wait = new Promise<void>((resolve) => {
      release = resolve;
    });
    const fetchModels = vi.fn(async () => {
      await wait;
      return [model("dynamic", "live")];
    });
    const dynamic = createProvider({
      id: "dynamic",
      baseUrl: "https://api.example.test/v1",
      auth: { apiKey: envApiKeyAuth("dynamic key", ["DYNAMIC_API_KEY"]) },
      models: [model("dynamic", "baseline")],
      fetchModels,
      stream: async function* () {},
    });
    const runtime = new ProviderModels([dynamic], new MemoryCredentialStore(), catalogs, {
      env: async () => "key",
      fileExists: async () => false,
    });

    await runtime.refresh({ provider: "dynamic", allowNetwork: false });
    expect(runtime.getModels("dynamic").map((entry) => entry.id)).toEqual(["baseline", "cached"]);

    const first = runtime.refresh({ provider: "dynamic", force: true });
    const second = runtime.refresh({ provider: "dynamic", force: true });
    await vi.waitFor(() => {
      expect(fetchModels).toHaveBeenCalledOnce();
    });
    release();
    await Promise.all([first, second]);

    expect(runtime.getModels("dynamic").map((entry) => entry.id)).toEqual(["baseline", "live"]);
    await expect(catalogs.read("dynamic")).resolves.toMatchObject({
      models: [expect.objectContaining({ id: "live" })],
      checkedAt: expect.any(Number),
    });
  });

  it("keeps a stored dynamic overlay when its first online refresh fails", async () => {
    const catalogs = new MemoryModelsStore();
    await catalogs.write("dynamic", {
      models: [model("dynamic", "cached")],
      checkedAt: 1,
    });
    const dynamic = createProvider({
      id: "dynamic",
      baseUrl: "https://api.example.test/v1",
      auth: { apiKey: envApiKeyAuth("dynamic key", ["DYNAMIC_API_KEY"]) },
      models: [model("dynamic", "baseline")],
      fetchModels: async () => {
        throw new Error("catalog unavailable");
      },
      stream: async function* () {},
    });
    const runtime = new ProviderModels([dynamic], new MemoryCredentialStore(), catalogs, {
      env: async () => "key",
      fileExists: async () => false,
    });

    const result = await runtime.refresh({ provider: "dynamic", force: true });

    expect(result.errors.get("dynamic")?.message).toBe("catalog unavailable");
    expect(runtime.getModels("dynamic").map((entry) => entry.id)).toEqual(["baseline", "cached"]);
  });

  it("registers and replaces providers by id", () => {
    const runtime = createModels();
    const first = provider("dynamic");
    const replacement = provider("dynamic");

    runtime.setProvider(first);
    expect(runtime.getProvider("dynamic")).toBe(first);

    runtime.setProvider(replacement);
    expect(runtime.getProvider("dynamic")).toBe(replacement);
    expect(runtime.getProviders()).toEqual([replacement]);
  });

  it("deletes one runtime provider without changing the others", () => {
    const runtime = createModels({ providers: [provider("first"), provider("second")] });

    runtime.deleteProvider("first");

    expect(runtime.getProviders().map((entry) => entry.id)).toEqual(["second"]);
  });

  it("clears every runtime provider", () => {
    const runtime = createModels({ providers: [provider("first"), provider("second")] });

    runtime.clearProviders();

    expect(runtime.getProviders()).toEqual([]);
  });

  it("stores an API key login and exposes that provider models as available", async () => {
    const runtime = new ProviderModels(
      [provider("example")],
      new MemoryCredentialStore(),
      new MemoryModelsStore(),
      { env: async () => undefined, fileExists: async () => false },
    );

    await runtime.login("example", "api_key", {
      prompt: async () => "secret-key",
      notify: () => {},
    });

    await expect(runtime.checkAuth("example")).resolves.toEqual({
      type: "api_key",
      source: "Stored API key",
    });
    await expect(runtime.getAvailable()).resolves.toEqual([model("example", "example-model")]);
  });

  it("refreshes only the requested provider catalog", async () => {
    const firstRefresh = vi.fn<NonNullable<Provider["refreshModels"]>>(async () => {});
    const secondRefresh = vi.fn<NonNullable<Provider["refreshModels"]>>(async () => {});
    const runtime = new ProviderModels(
      [provider("first", firstRefresh), provider("second", secondRefresh)],
      new MemoryCredentialStore(),
      new MemoryModelsStore(),
      {
        env: async (name) => (name === "SECOND_API_KEY" ? "second-key" : undefined),
        fileExists: async () => false,
      },
    );

    const result = await runtime.refresh({
      provider: "second",
      allowNetwork: true,
      force: true,
    });

    expect(result.errors.size).toBe(0);
    expect(firstRefresh).not.toHaveBeenCalled();
    expect(secondRefresh).toHaveBeenCalledOnce();
  });

  it("returns the final message produced by a provider stream", async () => {
    const expected: AssistantMessage = {
      role: "assistant",
      content: [{ type: "text", text: "hello" }],
      api: "openai-completions",
      provider: "example",
      model: "example-model",
      usage: {
        input: 1,
        output: 1,
        cacheRead: 0,
        cacheWrite: 0,
        totalTokens: 2,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
      },
      stopReason: "stop",
      finishReason: "completed",
      timestamp: 1,
    };
    const streamedProvider = provider("example");
    streamedProvider.stream = async function* () {
      yield { type: "done", reason: "stop", message: expected };
    };
    const runtime = new ProviderModels(
      [streamedProvider],
      new MemoryCredentialStore(),
      new MemoryModelsStore(),
      {
        env: async (name) => (name === "EXAMPLE_API_KEY" ? "env-key" : undefined),
        fileExists: async () => false,
      },
    );

    await expect(
      runtime.completeSimple(model("example", "example-model"), { messages: [] }),
    ).resolves.toEqual(expected);
  });
});
