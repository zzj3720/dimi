/**
 * Provider runtime catalog contracts — builtin provider refresh, cache reuse,
 * validators, authentication and host headers. Replaces network with fetch
 * fixtures; run with `vp test -- catalog.test.ts`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { ProviderModels } from "#/app/providerRuntime/models";
import { BUILTIN_CATALOG } from "#/app/providerRuntime/builtinCatalog.generated";
import { builtinProviders } from "#/app/providerRuntime/providers";
import { providerApis } from "#/app/providerRuntime/stream";
import type {
  Credential,
  CredentialInfo,
  CredentialStore,
  Model,
  ModelsStore,
  ModelsStoreEntry,
} from "#/app/providerRuntime/types";

class MemoryCredentials implements CredentialStore {
  private readonly values = new Map<string, Credential>();

  read(providerId: string): Promise<Credential | undefined> {
    return Promise.resolve(this.values.get(providerId));
  }

  list(): Promise<readonly CredentialInfo[]> {
    return Promise.resolve(
      [...this.values].map(([providerId, credential]) => ({ providerId, type: credential.type })),
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

class MemoryCatalogs implements ModelsStore {
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

const defaultHeaders = { "User-Agent": "dimi-cli/catalog-test" };

function runtime(
  credentials = new MemoryCredentials(),
  catalogs = new MemoryCatalogs(),
  headers?: Record<string, string>,
): ProviderModels {
  return new ProviderModels(builtinProviders(headers ?? defaultHeaders), credentials, catalogs);
}

async function setApiKey(
  credentials: MemoryCredentials,
  providerId: string,
  key = `${providerId}-key`,
): Promise<void> {
  await credentials.modify(providerId, async () => ({ type: "api_key", key }));
}

function response(body: unknown, status = 200, headers?: Record<string, string>): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", ...headers },
  });
}

function catalogModel(id: string, options: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id,
    capabilities: { chat: true },
    input_modalities: ["text"],
    reasoning: false,
    context_window: 128_000,
    max_output_tokens: 32_000,
    pricing: { input: 1, output: 2, cache_read: 0.1, cache_write: 0 },
    ...options,
  };
}

function headers(init: RequestInit | undefined): Headers {
  return new Headers(init?.headers);
}

function cachedModel(provider: string, id: string): Model {
  return {
    id,
    name: id,
    api: "openai-responses",
    provider,
    baseUrl: "https://api.openai.com/v1",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 32_000,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("builtin provider model catalogs", () => {
  it("keeps Pi's 38 provider identities in the generated Dimi catalog", () => {
    const ids = BUILTIN_CATALOG.providers.map((provider) => provider.id);

    expect(ids).toHaveLength(38);
    expect([...new Set(ids)]).toHaveLength(38);
    expect(ids).toEqual(
      expect.arrayContaining([
        "amazon-bedrock",
        "anthropic",
        "cloudflare-ai-gateway",
        "google",
        "kimi-coding",
        "openai-codex",
        "qwen-token-plan",
        "vercel-ai-gateway",
        "xai",
      ]),
    );
    expect(BUILTIN_CATALOG.providers.every((provider) => provider.api.length > 0)).toBe(true);
  });

  it("only exposes catalog providers and models whose transport is registered", () => {
    const catalogIds = new Set<string>(BUILTIN_CATALOG.providers.map((provider) => provider.id));
    const supportedApis = new Set<string>(providerApis());

    for (const provider of builtinProviders()) {
      expect(catalogIds.has(provider.id)).toBe(true);
      expect(provider.getModels().every((model) => supportedApis.has(model.api))).toBe(true);
    }
  });

  it("publishes DeepSeek V4 Flash with its selectable effort levels", () => {
    expect(runtime().getModel("deepseek", "deepseek-v4-flash")).toMatchObject({
      name: "DeepSeek V4 Flash",
      reasoning: true,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 384_000,
      cost: { input: 0.14, output: 0.28, cacheRead: 0.0028, cacheWrite: 0 },
      thinkingLevelMap: { low: "low", high: "high", max: "max" },
      defaultThinkingLevel: "high",
      compat: {
        requiresReasoningContentOnAssistantMessages: true,
        supportsReasoningEffort: true,
        thinkingFormat: "deepseek",
      },
    });
  });

  it("publishes DeepSeek V4 Pro with its distinct effort levels", () => {
    expect(runtime().getModel("deepseek", "deepseek-v4-pro")).toMatchObject({
      name: "DeepSeek V4 Pro",
      reasoning: true,
      contextWindow: 1_000_000,
      maxTokens: 384_000,
      cost: { input: 0.435, output: 0.87, cacheRead: 0.003625, cacheWrite: 0 },
      thinkingLevelMap: { high: "high", max: "max" },
      defaultThinkingLevel: "high",
      compat: {
        requiresReasoningContentOnAssistantMessages: true,
        supportsReasoningEffort: true,
        thinkingFormat: "deepseek",
      },
    });
  });

  it("retains DeepSeek V4 Flash effort metadata after a live catalog refresh", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "deepseek");
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => response({ data: [{ id: "deepseek-v4-flash" }] })),
    );
    const models = runtime(credentials);

    await models.refresh({ provider: "deepseek", force: true });

    expect(models.getModel("deepseek", "deepseek-v4-flash")).toMatchObject({
      thinkingLevelMap: { low: "low", high: "high", max: "max" },
      defaultThinkingLevel: "high",
      compat: {
        requiresReasoningContentOnAssistantMessages: true,
        supportsReasoningEffort: true,
        thinkingFormat: "deepseek",
      },
    });
  });

  it("projects Grok 4.5 to the Responses transport and supported thinking controls", () => {
    const model = runtime().getModel("xai", "grok-4.5");

    expect(model).toMatchObject({
      api: "openai-responses",
      input: ["text", "image"],
      contextWindow: 500_000,
      maxTokens: 500_000,
      thinkingLevelMap: {
        off: null,
        minimal: null,
        medium: "medium",
      },
      compat: { supportsLongCacheRetention: false },
    });
    expect(model?.thinkingLevelMap).not.toHaveProperty("none");
  });

  it("loads the authenticated Radius gateway catalog and restores it offline", async () => {
    const credentials = new MemoryCredentials();
    const catalogs = new MemoryCatalogs();
    await setApiKey(credentials, "radius", "radius-key");
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string, init?: RequestInit) => {
        expect(input).toBe("https://radius.pi.dev/v1/config");
        expect(headers(init).get("authorization")).toBe("Bearer radius-key");
        return response({
          baseUrl: "https://gateway.example.test/v1",
          models: [{
            id: "radius-model",
            name: "Radius model",
            reasoning: true,
            input: ["text", "image"],
            cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 0.2 },
            contextWindow: 200_000,
            maxTokens: 16_000,
            thinkingLevelMap: { high: "high" },
            defaultThinkingLevel: "high",
          }],
        });
      }),
    );
    const models = runtime(credentials, catalogs);

    await expect(models.refresh({ provider: "radius", force: true })).resolves.toMatchObject({ errors: new Map() });
    expect(models.getModel("radius", "radius-model")).toMatchObject({
      api: "pi-messages",
      baseUrl: "https://gateway.example.test/v1",
      contextWindow: 200_000,
      maxTokens: 16_000,
      input: ["text", "image"],
    });

    const restored = runtime(credentials, catalogs);
    await restored.refresh({ provider: "radius", allowNetwork: false });
    expect(restored.getModel("radius", "radius-model")).toMatchObject({ api: "pi-messages" });
  });

  it("replaces a provider catalog snapshot and rejects non-chat models", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "xai");
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        response({
          data: [
            catalogModel("current-chat-model"),
            { id: "text-embedding-3-large", type: "embedding" },
          ],
        }),
      ),
    );
    const models = runtime(credentials);

    await models.refresh({ provider: "xai", force: true });

    expect(models.getModels("xai").map((model) => model.id)).toEqual(["current-chat-model"]);
    expect(models.getModel("xai", "grok-4.3")).toBeUndefined();
  });

  it("keeps the generated baseline before an offline provider has a catalog snapshot", async () => {
    const models = runtime();

    await models.refresh({ provider: "xai", allowNetwork: false });

    expect(models.getModel("xai", "grok-4.3")).toMatchObject({ id: "grok-4.3" });
  });

  it("uses a provider's bare model IDs only as an availability overlay", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "xai");
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => response({ data: [{ id: "grok-4.3" }, { id: "unknown-provider-id" }] })),
    );
    const models = runtime(credentials);

    await models.refresh({ provider: "xai", force: true });

    expect(models.getModels("xai")).toMatchObject([
      {
        id: "grok-4.3",
        contextWindow: 1_000_000,
        maxTokens: 30_000,
        cost: { input: 1.25, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
      },
    ]);
    expect(models.getModel("xai", "unknown-provider-id")).toBeUndefined();
  });

  it("uses the catalog TTL before issuing a second network request", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "openai");
    const fetch = vi.fn(async () => response({ data: [catalogModel("catalog-model")] }));
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials);

    await models.refresh({ provider: "openai", force: true });
    await models.refresh({ provider: "openai" });

    expect(fetch).toHaveBeenCalledOnce();
    expect(models.getModel("openai", "catalog-model")).toMatchObject({ id: "catalog-model" });
  });

  it("bypasses the catalog TTL for a forced refresh", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "openai");
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(response({ data: [catalogModel("first-catalog-model")] }))
      .mockResolvedValueOnce(response({ data: [catalogModel("second-catalog-model")] }));
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials);

    await models.refresh({ provider: "openai", force: true });
    await models.refresh({ provider: "openai", force: true });

    expect(fetch).toHaveBeenCalledTimes(2);
    expect(models.getModel("openai", "first-catalog-model")).toBeUndefined();
    expect(models.getModel("openai", "second-catalog-model")).toMatchObject({
      id: "second-catalog-model",
    });
  });

  it("revalidates a catalog with its ETag and retains the cached overlay on 304", async () => {
    const credentials = new MemoryCredentials();
    const catalogs = new MemoryCatalogs();
    await setApiKey(credentials, "openai");
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(
        response({ data: [catalogModel("etag-model")] }, 200, { etag: "catalog-v1" }),
      )
      .mockResolvedValueOnce(new Response(null, { status: 304, headers: { etag: "catalog-v1" } }));
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials, catalogs);

    await models.refresh({ provider: "openai", force: true });
    const checkedAt = (await catalogs.read("openai"))?.checkedAt;
    await models.refresh({ provider: "openai", force: true });

    expect(headers(fetch.mock.calls[0]?.[1]).get("if-none-match")).toBeNull();
    expect(headers(fetch.mock.calls[1]?.[1]).get("if-none-match")).toBe("catalog-v1");
    expect(models.getModel("openai", "etag-model")).toMatchObject({ id: "etag-model" });
    await expect(catalogs.read("openai")).resolves.toMatchObject({
      etag: "catalog-v1",
      checkedAt: expect.any(Number),
    });
    expect((await catalogs.read("openai"))?.checkedAt).toBeGreaterThanOrEqual(checkedAt ?? 0);
  });

  it("keeps the previous catalog and ETag after a transient catalog failure", async () => {
    const credentials = new MemoryCredentials();
    const catalogs = new MemoryCatalogs();
    await setApiKey(credentials, "openai");
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(
        response({ data: [catalogModel("cached-after-error")] }, 200, { etag: "catalog-v1" }),
      )
      .mockResolvedValueOnce(response({ error: "rate limited" }, 429));
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials, catalogs);

    await models.refresh({ provider: "openai", force: true });
    const result = await models.refresh({ provider: "openai", force: true });

    expect(result.errors.get("openai")?.message).toContain("HTTP 429");
    expect(models.getModel("openai", "cached-after-error")).toMatchObject({
      id: "cached-after-error",
    });
    await expect(catalogs.read("openai")).resolves.toMatchObject({
      etag: "catalog-v1",
      models: [expect.objectContaining({ id: "cached-after-error" })],
    });
  });

  it("restores a cached catalog without issuing a network request", async () => {
    const catalogs = new MemoryCatalogs();
    await catalogs.write("openai", {
      models: [cachedModel("openai", "offline-catalog-model")],
      checkedAt: 1,
      lastModified: Date.now(),
      etag: "cached-etag",
    });
    const fetch = vi.fn(async () => response({ data: [] }));
    vi.stubGlobal("fetch", fetch);
    const models = runtime(new MemoryCredentials(), catalogs);

    await models.refresh({ provider: "openai", allowNetwork: false });

    expect(fetch).not.toHaveBeenCalled();
    expect(models.getModel("openai", "offline-catalog-model")).toMatchObject({
      id: "offline-catalog-model",
    });
  });

  it("ignores a catalog cache older than the generated metadata", async () => {
    const catalogs = new MemoryCatalogs();
    await catalogs.write("openai", {
      models: [cachedModel("openai", "stale-catalog-model")],
      checkedAt: Date.now(),
      lastModified: 1,
    });
    const fetch = vi.fn(async () => response({ data: [] }));
    vi.stubGlobal("fetch", fetch);
    const models = runtime(new MemoryCredentials(), catalogs);

    await models.refresh({ provider: "openai", allowNetwork: false });

    expect(fetch).not.toHaveBeenCalled();
    expect(models.getModel("openai", "stale-catalog-model")).toBeUndefined();
    expect(models.getModel("openai", "gpt-5.4")).toMatchObject({ id: "gpt-5.4" });
  });

  it("accepts an empty successful catalog as the authoritative availability snapshot", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "openai");
    vi.stubGlobal("fetch", vi.fn(async () => response({ data: [] })));
    const models = runtime(credentials);

    await models.refresh({ provider: "openai", force: true });

    expect(models.getModels("openai")).toEqual([]);
  });

  it("sends provider authentication and isolates host headers by endpoint ownership", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "openai", "openai-secret");
    await setApiKey(credentials, "kimi-coding", "dimi-secret");
    const fetch = vi.fn(async (input: string | URL, init?: RequestInit) => {
      const url = String(input);
      if (url === "https://api.openai.com/v1/models") return response({ data: [] });
      if (url === "https://api.kimi.com/coding/v1/models") return response({ data: [] });
      throw new Error(`Unexpected catalog URL: ${url}`);
    });
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials, new MemoryCatalogs(), {
      "User-Agent": "dimi-cli/catalog-test",
      "X-Msh-Device-Id": "device-test",
    });

    await models.refresh({ provider: "openai", force: true });
    await models.refresh({ provider: "kimi-coding", force: true });

    const openaiHeaders = headers(fetch.mock.calls[0]?.[1]);
    const dimiHeaders = headers(fetch.mock.calls[1]?.[1]);
    expect(openaiHeaders.get("authorization")).toBe("Bearer openai-secret");
    expect(openaiHeaders.get("user-agent")).toBe("dimi-cli/catalog-test");
    expect(openaiHeaders.get("x-msh-device-id")).toBeNull();
    expect(dimiHeaders.get("authorization")).toBe("Bearer dimi-secret");
    expect(dimiHeaders.get("user-agent")).toBe("dimi-cli/catalog-test");
    expect(dimiHeaders.get("x-msh-device-id")).toBe("device-test");
  });

  it("parses Dimi and Codex catalog records with their provider contracts", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "kimi-coding", "dimi-secret");
    await credentials.modify("openai-codex", async () => ({
      type: "oauth",
      access: "codex-access",
      refresh: "codex-refresh",
      expires: Date.now() + 60 * 60_000,
    }));
    const fetch = vi.fn(async (input: string | URL) => {
      const url = String(input);
      if (url === "https://api.kimi.com/coding/v1/models") {
        return response({
          data: [
            catalogModel("dimi-catalog", {
              display_name: "Dimi Catalog",
              context_window: 262_144,
            }),
          ],
        });
      }
      if (url === "https://chatgpt.com/backend-api/codex/models") {
        return response({
          models: [
            catalogModel("codex-catalog", {
              slug: "codex-catalog",
              display_name: "Codex Catalog",
              context_window: 400_000,
              context_window_size: 400_000,
              max_output_tokens: 16_000,
            }),
          ],
        });
      }
      throw new Error(`Unexpected catalog URL: ${url}`);
    });
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials);

    await models.refresh({ provider: "kimi-coding", force: true });
    await models.refresh({ provider: "openai-codex", force: true });

    expect(models.getModel("kimi-coding", "dimi-catalog")).toMatchObject({
      api: "anthropic-messages",
      name: "Dimi Catalog",
      contextWindow: 262_144,
    });
    expect(models.getModel("openai-codex", "codex-catalog")).toMatchObject({
      api: "openai-codex-responses",
      name: "Codex Catalog",
      contextWindow: 400_000,
      maxTokens: 16_000,
      reasoning: true,
    });
  });

  it("persists dynamic catalogs under their own provider keys", async () => {
    const credentials = new MemoryCredentials();
    const catalogs = new MemoryCatalogs();
    await setApiKey(credentials, "openai", "openai-secret");
    await setApiKey(credentials, "xai", "xai-secret");
    const fetch = vi.fn(async (input: string | URL) => {
      const url = String(input);
      if (url === "https://api.openai.com/v1/models")
        return response({ data: [catalogModel("openai-catalog")] });
      if (url === "https://api.x.ai/v1/models")
        return response({ data: [catalogModel("xai-catalog")] });
      throw new Error(`Unexpected catalog URL: ${url}`);
    });
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials, catalogs);

    await models.refresh({ provider: "openai", force: true });
    await models.refresh({ provider: "xai", force: true });

    await expect(catalogs.read("openai")).resolves.toMatchObject({
      models: [expect.objectContaining({ provider: "openai", id: "openai-catalog" })],
    });
    await expect(catalogs.read("xai")).resolves.toMatchObject({
      models: [expect.objectContaining({ provider: "xai", id: "xai-catalog" })],
    });
  });
});
