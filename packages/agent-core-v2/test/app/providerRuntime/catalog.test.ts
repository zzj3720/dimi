/**
 * Provider runtime catalog contracts — builtin provider refresh, cache reuse,
 * validators, authentication and host headers. Replaces network with fetch
 * fixtures; run with `vp test -- catalog.test.ts`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { ProviderModels } from "#/app/providerRuntime/models";
import { builtinProviders } from "#/app/providerRuntime/providers";
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

const defaultHeaders = { "User-Agent": "kimi-code-cli/catalog-test" };

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
  it("uses the catalog TTL before issuing a second network request", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "openai");
    const fetch = vi.fn(async () => response({ data: [{ id: "catalog-model" }] }));
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
      .mockResolvedValueOnce(response({ data: [{ id: "first-catalog-model" }] }))
      .mockResolvedValueOnce(response({ data: [{ id: "second-catalog-model" }] }));
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
        response({ data: [{ id: "etag-model" }] }, 200, { etag: "catalog-v1" }),
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
        response({ data: [{ id: "cached-after-error" }] }, 200, { etag: "catalog-v1" }),
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

  it("sends provider authentication and isolates host headers by endpoint ownership", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "openai", "openai-secret");
    await setApiKey(credentials, "kimi-coding", "kimi-secret");
    const fetch = vi.fn(async (input: string | URL, init?: RequestInit) => {
      const url = String(input);
      if (url === "https://api.openai.com/v1/models") return response({ data: [] });
      if (url === "https://api.kimi.com/coding/v1/models") return response({ data: [] });
      throw new Error(`Unexpected catalog URL: ${url}`);
    });
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials, new MemoryCatalogs(), {
      "User-Agent": "kimi-code-cli/catalog-test",
      "X-Msh-Device-Id": "device-test",
    });

    await models.refresh({ provider: "openai", force: true });
    await models.refresh({ provider: "kimi-coding", force: true });

    const openaiHeaders = headers(fetch.mock.calls[0]?.[1]);
    const kimiHeaders = headers(fetch.mock.calls[1]?.[1]);
    expect(openaiHeaders.get("authorization")).toBe("Bearer openai-secret");
    expect(openaiHeaders.get("user-agent")).toBe("kimi-code-cli/catalog-test");
    expect(openaiHeaders.get("x-msh-device-id")).toBeNull();
    expect(kimiHeaders.get("authorization")).toBe("Bearer kimi-secret");
    expect(kimiHeaders.get("user-agent")).toBe("kimi-code-cli/catalog-test");
    expect(kimiHeaders.get("x-msh-device-id")).toBe("device-test");
  });

  it("parses Kimi and Codex catalog records with their provider contracts", async () => {
    const credentials = new MemoryCredentials();
    await setApiKey(credentials, "kimi-coding", "kimi-secret");
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
          data: [{ id: "kimi-catalog", display_name: "Kimi Catalog", context_window: 262_144 }],
        });
      }
      if (url === "https://chatgpt.com/backend-api/codex/models") {
        return response({
          models: [
            {
              slug: "codex-catalog",
              display_name: "Codex Catalog",
              context_window_size: 400_000,
              max_output_tokens: 16_000,
            },
          ],
        });
      }
      throw new Error(`Unexpected catalog URL: ${url}`);
    });
    vi.stubGlobal("fetch", fetch);
    const models = runtime(credentials);

    await models.refresh({ provider: "kimi-coding", force: true });
    await models.refresh({ provider: "openai-codex", force: true });

    expect(models.getModel("kimi-coding", "kimi-catalog")).toMatchObject({
      api: "anthropic-messages",
      name: "Kimi Catalog",
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
        return response({ data: [{ id: "openai-catalog" }] });
      if (url === "https://api.x.ai/v1/models") return response({ data: [{ id: "xai-catalog" }] });
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
