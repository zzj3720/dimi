import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { ProviderRuntimeErrors } from "#/app/providerRuntime/errors";
import { composeProvider, customProvider, parseCustomProviderDefinition } from "#/app/providerRuntime/customProviders";
import { envApiKeyAuth } from "#/app/providerRuntime/auth";
import { createModels } from "#/app/providerRuntime/models";
import { builtinProviders } from "#/app/providerRuntime/providers";
import { InMemoryCredentialStore } from "#/app/providerRuntime/storage";
import type { Provider } from "#/app/providerRuntime/types";

const definition = {
  id: "local",
  name: "Local",
  api: "openai-completions",
  baseUrl: "https://api.example.test/v1",
  models: [{
    id: "local-chat",
    reasoning: true,
    input: ["text", "image"],
    contextWindow: 65_536,
    maxTokens: 8_192,
    cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 0.2 },
  }],
} as const;

describe("custom providers", () => {
  const builtin: Provider = {
    id: "xai",
    name: "xAI",
    baseUrl: "https://api.x.ai/v1",
    auth: { apiKey: envApiKeyAuth("xAI API key", ["XAI_API_KEY"]) },
    getModels: () => [{
      id: "grok-4.5",
      name: "Grok 4.5",
      api: "openai-responses",
      provider: "xai",
      baseUrl: "https://api.x.ai/v1",
      reasoning: true,
      input: ["text"],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 500_000,
      maxTokens: 500_000,
      thinkingLevelMap: { off: null, low: "low", high: "high" },
    }],
    stream: async function* () {},
  };

  it("treats a models.json entry as a built-in overlay, not a conflicting replacement", () => {
    const provider = composeProvider({
      id: "xai",
      baseUrl: "https://gateway.example.test/v1",
      headers: { "x-gateway": "test" },
      modelOverrides: { "grok-4.5": { maxTokens: 16_384 } },
    }, builtin);

    expect(provider.name).toBe("xAI");
    expect(provider.getModels()).toEqual([expect.objectContaining({
      id: "grok-4.5",
      api: "openai-responses",
      baseUrl: "https://gateway.example.test/v1",
      contextWindow: 500_000,
      maxTokens: 16_384,
      headers: { "x-gateway": "test" },
    })]);
  });

  it("keeps Pi-style non-HTTP provider and model endpoint overlays for adapter-owned resolution", () => {
    const provider = composeProvider({
      id: "xai",
      baseUrl: "$XAI_GATEWAY",
      models: [{ id: "grok-private", baseUrl: "gateway://private", contextWindow: 300_000, maxTokens: 32_000 }],
    }, builtin);
    expect(provider.getModels()).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "grok-4.5", baseUrl: "$XAI_GATEWAY" }),
      expect.objectContaining({ id: "grok-private", baseUrl: "gateway://private" }),
    ]));
  });

  it("allows a new built-in-provider model only with explicit limits", () => {
    const provider = composeProvider({
      id: "xai",
      models: [{ id: "grok-private", contextWindow: 300_000, maxTokens: 32_000 }],
    }, builtin);
    expect(provider.getModels()).toContainEqual(expect.objectContaining({
      id: "grok-private",
      api: "openai-responses",
      contextWindow: 300_000,
      maxTokens: 32_000,
    }));
  });

  it("merges partial model override cost fields and preserves base tiers", () => {
    const provider = composeProvider({
      id: "xai",
      modelOverrides: { "grok-4.5": { cost: { output: 3 } } },
    }, {
      ...builtin,
      getModels: () => [{ ...builtin.getModels()[0]!, cost: {
        input: 1, output: 2, cacheRead: 0.5, cacheWrite: 0.75,
        tiers: [{ inputTokensAbove: 1_000, input: 4, output: 5, cacheRead: 2, cacheWrite: 3 }],
      } }],
    });
    expect(provider.getModels()[0]?.cost).toEqual({
      input: 1, output: 3, cacheRead: 0.5, cacheWrite: 0.75,
      tiers: [{ inputTokensAbove: 1_000, input: 4, output: 5, cacheRead: 2, cacheWrite: 3 }],
    });
  });

  it("ignores a stale model override after a dynamic catalog removes that id", () => {
    const provider = composeProvider({
      id: "xai",
      modelOverrides: { "retired-grok": { maxTokens: 1 } },
    }, builtin);
    expect(provider.getModels()).toEqual([expect.objectContaining({ id: "grok-4.5" })]);
  });

  it("keeps explicitly configured capability metadata without guessed limits", () => {
    const provider = customProvider(definition);
    expect(provider.getModels()).toEqual([
      expect.objectContaining({
        contextWindow: 65_536,
        maxTokens: 8_192,
        reasoning: true,
        input: ["text", "image"],
      }),
    ]);
  });

  it("rejects an unknown configured model without context or output limits when composed", () => {
    expect(() =>
      customProvider({
        ...definition,
        models: [{ id: "unknown" }],
      }),
    ).toThrow(expect.objectContaining({ code: ProviderRuntimeErrors.codes.PROVIDER_INVALID_DEFINITION }));
  });

  it("rejects a protocol without a registered stream adapter when materializing a custom model", () => {
    expect(() => customProvider({ ...definition, api: "unknown" })).toThrow(
      expect.objectContaining({ code: ProviderRuntimeErrors.codes.PROVIDER_INVALID_DEFINITION }),
    );
  });

  it("does not accept unimplemented input modalities", () => {
    expect(() =>
      parseCustomProviderDefinition({
        ...definition,
        models: [{ ...definition.models[0], input: ["text", "audio"] }],
      }),
    ).toThrow(expect.objectContaining({ code: ProviderRuntimeErrors.codes.PROVIDER_INVALID_DEFINITION }));
  });

  it("rejects unknown fields at every user-owned config layer with their field path", () => {
    expect(() => parseCustomProviderDefinition({ ...definition, typoProviderOption: true })).toThrow(
      "typoProviderOption",
    );
    expect(() => parseCustomProviderDefinition({
      ...definition,
      models: [{ ...definition.models[0], typoModelOption: true }],
    })).toThrow("models.0");
    expect(() => parseCustomProviderDefinition({
      ...definition,
      modelOverrides: { "local-chat": { cost: { input: 1, typoCostOption: 2 } } },
    })).toThrow("modelOverrides.local-chat.cost");
  });

  it("rejects unknown and API-inapplicable compat fields instead of silently ignoring them", () => {
    expect(() => parseCustomProviderDefinition({ ...definition, compat: { madeUpToggle: true } })).toThrow(
      expect.objectContaining({ code: ProviderRuntimeErrors.codes.PROVIDER_INVALID_DEFINITION }),
    );
    expect(() => customProvider({ ...definition, compat: { supportsTemperature: false } })).toThrow(
      "unsupported for openai-completions",
    );
    expect(() => customProvider({
      ...definition,
      api: "anthropic-messages",
      compat: { maxTokensField: "max_tokens" },
    })).toThrow("unsupported for anthropic-messages");
    expect(() => customProvider({
      ...definition,
      api: "openai-responses",
      compat: { openRouterRouting: { only: ["example"] } },
    })).toThrow("unsupported for openai-responses");
  });

  it("accepts only OpenAI compat fields that the wire adapter serializes", () => {
    expect(() => customProvider({
      ...definition,
      compat: {
        supportsStore: true,
        maxTokensField: "max_completion_tokens",
        thinkingFormat: "qwen-chat-template",
        sendSessionAffinityHeaders: true,
        sessionAffinityFormat: "openrouter",
      },
    })).not.toThrow();
  });

  it("resolves stored credentials before models.json keys, then config keys before inherited environments", async () => {
    const provider = composeProvider({ id: "xai", apiKey: "$CONFIG_KEY" }, builtin);
    const apiKey = provider.auth.apiKey;
    if (apiKey === undefined) throw new Error("missing API key auth");
    const ctx = {
      env: async (name: string) => ({ CONFIG_KEY: "config-key", XAI_API_KEY: "inherited-key" })[name],
      fileExists: async () => false,
    };

    await expect(apiKey.resolve({ ctx, credential: { type: "api_key", key: "stored-key" } })).resolves.toMatchObject({
      auth: { apiKey: "stored-key" },
    });
    await expect(apiKey.resolve({ ctx })).resolves.toMatchObject({ auth: { apiKey: "config-key" } });
  });

  it("does not execute configured key commands while discovering availability", async () => {
    const directory = await mkdtemp(join(tmpdir(), "provider-command-check-"));
    const marker = join(directory, "executed");
    try {
      const provider = customProvider({
        ...definition,
        apiKey: `!printf command-key | tee ${marker}`,
      });
      const runtime = createModels({
        providers: [provider],
        authContext: { env: async () => undefined, fileExists: async () => false },
      });

      await expect(runtime.checkAuth("local")).resolves.toEqual({
        type: "api_key",
        source: "models.json command",
      });
      await expect(runtime.getAvailable("local")).resolves.toHaveLength(1);
      await expect(access(marker)).rejects.toMatchObject({ code: "ENOENT" });

      await expect(runtime.getAuth("local")).resolves.toMatchObject({ auth: { apiKey: "command-key" } });
      await expect(readFile(marker, "utf8")).resolves.toBe("command-key");
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });

  it("keeps an ordinary missing configured key unavailable", async () => {
    const unavailable = createModels({
      providers: [customProvider({ ...definition, apiKey: "$MISSING_KEY" })],
      authContext: { env: async () => undefined, fileExists: async () => false },
    });
    await expect(unavailable.getAvailable("local")).resolves.toEqual([]);
  });

  it.each(["$MISSING_KEY", "!exit 1"])(
    "reports authHeader failure for %s at request time",
    async (apiKey) => {
      const required = createModels({
        providers: [customProvider({ ...definition, apiKey, authHeader: true })],
        authContext: { env: async () => undefined, fileExists: async () => false },
      });
      await expect(required.getAuth("local")).rejects.toThrow("authHeader requires a resolved API key");
    },
  );

  it("resolves models.json env templates and commands in provider and model request headers", async () => {
    const provider = customProvider({
      ...definition,
      apiKey: "$CUSTOM_KEY",
      headers: { "x-provider": "provider-${HEADER_VALUE}", "x-command": "!printf command-value" },
      models: [{ ...definition.models[0], headers: { "x-model": "$MODEL_HEADER" } }],
    });
    const runtime = createModels({
      providers: [provider],
      authContext: {
        env: async (name) => ({ CUSTOM_KEY: "key", HEADER_VALUE: "header", MODEL_HEADER: "model" })[name],
        fileExists: async () => false,
      },
    });
    const model = runtime.getModel("local", "local-chat");
    if (model === undefined) throw new Error("missing local model");
    await expect(runtime.getAuth(model)).resolves.toMatchObject({
      auth: { apiKey: "key", headers: { "x-provider": "provider-header", "x-model": "model", "x-command": "command-value" } },
    });
  });

  it("supports Pi-compatible Radius OAuth custom gateways", () => {
    const provider = customProvider({
      id: "radius-dev",
      name: "Radius dev",
      oauth: "radius",
      baseUrl: "https://radius.example.test",
      api: "pi-messages",
      models: [{ id: "gateway-model", contextWindow: 128_000, maxTokens: 8_192 }],
    });
    expect(provider.auth.oauth?.name).toBe("Radius dev");
    expect(provider.auth.apiKey).toBeUndefined();
    expect(() => customProvider({
      id: "radius-invalid",
      oauth: "radius",
      api: "pi-messages",
      models: [{ id: "model", contextWindow: 1, maxTokens: 1 }],
    })).toThrow("baseUrl");
  });

  it("keeps Cloudflare account and gateway state in credentials while models.json adds a gateway model", async () => {
    const base = builtinProviders().find((provider) => provider.id === "cloudflare-ai-gateway");
    if (base === undefined) throw new Error("missing Cloudflare AI Gateway provider");
    expect(base.baseUrl).toBe("https://gateway.ai.cloudflare.com/v1");
    const credentials = new InMemoryCredentialStore();
    await credentials.modify("cloudflare-ai-gateway", async () => ({
      type: "api_key",
      key: "example-key",
      env: { CLOUDFLARE_ACCOUNT_ID: "example-account", CLOUDFLARE_GATEWAY_ID: "example-gateway" },
    }));
    const provider = composeProvider({
      id: "cloudflare-ai-gateway",
      models: [{ id: "workers-ai/example-model", api: "openai-completions", contextWindow: 128_000, maxTokens: 8_192 }],
    }, base);
    const models = createModels({
      providers: [provider],
      credentials,
      authContext: { env: async () => undefined, fileExists: async () => false },
    });
    const model = models.getModel("cloudflare-ai-gateway", "workers-ai/example-model");
    if (model === undefined) throw new Error("missing configured Cloudflare model");
    await expect(models.getAuth(model)).resolves.toMatchObject({
      auth: {
        baseUrl: "https://gateway.ai.cloudflare.com/v1/example-account/example-gateway/compat",
        headers: { "cf-aig-authorization": "Bearer example-key" },
      },
    });
  });

  it("rejects authHeader when OAuth does not resolve an API key", async () => {
    const provider = composeProvider({ id: "oauth", authHeader: true, models: [{ id: "m", api: "pi-messages", baseUrl: "https://example.test", contextWindow: 1, maxTokens: 1 }] }, {
      id: "oauth", name: "OAuth", auth: {
        oauth: {
          name: "OAuth",
          login: async () => ({ type: "oauth", access: "a", refresh: "r", expires: 1 }),
          refresh: async (credential) => credential,
          toAuth: async () => ({ headers: { "x-token": "present" } }),
        },
      },
      getModels: () => [],
      stream: async function* () {},
    });
    await expect(provider.auth.oauth?.toAuth({ type: "oauth", access: "a", refresh: "r", expires: 1 })).rejects.toThrow("authHeader");
  });

  it("applies model header templates after API-key provider headers, case-insensitively", async () => {
    const provider = customProvider({
      ...definition,
      apiKey: "$CUSTOM_KEY",
      headers: { Authorization: "Bearer provider", "x-remove": "provider" },
      models: [{ ...definition.models[0], headers: { authorization: "Bearer $MODEL_TOKEN", "X-Remove": "model" } }],
    });
    const runtime = createModels({
      providers: [provider],
      authContext: {
        env: async (name) => ({ CUSTOM_KEY: "key", MODEL_TOKEN: "model-token" })[name],
        fileExists: async () => false,
      },
    });
    const model = runtime.getModel("local", "local-chat")!;
    await expect(runtime.getAuth(model)).resolves.toMatchObject({
      auth: { headers: { authorization: "Bearer model-token", "X-Remove": "model" } },
    });
  });

  it("applies model headers after OAuth provider headers and retains null deletions", async () => {
    const credentials = new InMemoryCredentialStore();
    await credentials.modify("oauth", async () => ({
      type: "oauth", access: "a", refresh: "r", expires: Date.now() + 60 * 60_000,
      env: { PROVIDER_TOKEN: "provider-from-oauth", MODEL_TOKEN: "model-from-oauth" },
    }));
    const provider = composeProvider({
      id: "oauth", headers: { Authorization: "Bearer $PROVIDER_TOKEN", "x-provider": "$PROVIDER_TOKEN", "x-delete": "present" },
      models: [{ id: "m", api: "pi-messages", headers: { authorization: "$MODEL_TOKEN", "X-Delete": null }, contextWindow: 1, maxTokens: 1 }],
    }, {
      id: "oauth", name: "OAuth", baseUrl: "https://example.test", auth: { oauth: {
        name: "OAuth", login: async () => ({ type: "oauth", access: "a", refresh: "r", expires: Date.now() + 60_000 }),
        refresh: async (credential) => credential,
        toAuth: async () => ({ headers: { authorization: "Bearer oauth", "x-delete": "oauth" } }),
      } },
      getModels: () => [], stream: async function* () {},
    });
    const runtime = createModels({ providers: [provider], credentials, authContext: {
      env: async () => undefined,
      fileExists: async () => false,
    } });
    const model = runtime.getModel("oauth", "m")!;
    await expect(runtime.getAuth(model)).resolves.toMatchObject({
      env: { PROVIDER_TOKEN: "provider-from-oauth", MODEL_TOKEN: "model-from-oauth" },
      auth: { headers: { authorization: "model-from-oauth", "x-provider": "provider-from-oauth", "X-Delete": null } },
    });
  });
});
