/**
 * Provider runtime model/auth contracts — model enumeration, catalog refresh,
 * persisted catalog recovery, and credential lifecycle. Uses in-memory stores
 * and provider boundaries; run with `vp test -- models.test.ts`.
 */
import { describe, expect, it, vi } from "vitest";

import { ProviderRuntimeErrors } from "#/app/providerRuntime/errors";
import { ProviderModels } from "#/app/providerRuntime/models";
import { APIStatusError } from "#/llmProtocol/errors";
import type {
  ApiKeyAuth,
  AuthContext,
  AuthInteraction,
  Credential,
  CredentialInfo,
  CredentialStore,
  Model,
  ModelsStore,
  ModelsStoreEntry,
  OAuthAuth,
  Provider,
} from "#/app/providerRuntime/types";

const authContext: AuthContext = {
  env: async (name) => (name === "EXAMPLE_API_KEY" ? "environment-key" : undefined),
  fileExists: async () => false,
};

const interaction: AuthInteraction = {
  prompt: async () => "entered-key",
  notify: () => {},
};

function model(provider: string, id: string): Model {
  return {
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
  };
}

class MemoryCredentials implements CredentialStore {
  private readonly values = new Map<string, Credential>();
  private pending: Promise<void> = Promise.resolve();

  read(providerId: string): Promise<Credential | undefined> {
    return Promise.resolve(this.values.get(providerId));
  }

  list(): Promise<readonly CredentialInfo[]> {
    return Promise.resolve(
      [...this.values].map(([providerId, credential]) => ({ providerId, type: credential.type })),
    );
  }

  modify(
    providerId: string,
    fn: (current: Credential | undefined) => Promise<Credential | undefined>,
  ): Promise<Credential | undefined> {
    const operation = this.pending.then(async () => {
      const current = this.values.get(providerId);
      const next = await fn(current);
      if (next !== undefined) this.values.set(providerId, next);
      return next ?? current;
    });
    this.pending = operation.then(
      () => undefined,
      () => undefined,
    );
    return operation;
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

function apiKeyAuth(value?: string): ApiKeyAuth {
  return {
    name: "API key",
    login: async () => ({ type: "api_key", key: "logged-in-key" }),
    resolve: async ({ ctx, credential }) => {
      const key = credential?.key ?? value ?? (await ctx.env("EXAMPLE_API_KEY"));
      return key === undefined
        ? undefined
        : { auth: { apiKey: key }, source: credential === undefined ? "Environment" : "Stored" };
    },
  };
}

function unavailableApiKeyAuth(): ApiKeyAuth {
  return {
    name: "Unavailable API key",
    resolve: async () => undefined,
  };
}

function provider(input: {
  id: string;
  models?: readonly Model[];
  auth?: Provider["auth"];
  refreshModels?: Provider["refreshModels"];
}): Provider {
  const models = input.models ?? [model(input.id, `${input.id}-model`)];
  return {
    id: input.id,
    name: input.id,
    baseUrl: "https://api.example.test/v1",
    auth: input.auth ?? { apiKey: apiKeyAuth() },
    getModels: () => models,
    refreshModels: input.refreshModels,
    stream: async function* () {},
  };
}

function runtime(
  providers: readonly Provider[],
  credentials: CredentialStore = new MemoryCredentials(),
  catalogs: ModelsStore = new MemoryCatalogs(),
): ProviderModels {
  return new ProviderModels(providers, credentials, catalogs, authContext);
}

describe("ProviderModels model and authentication contracts", () => {
  it("enumerates all provider models and finds a model within its provider", () => {
    const models = runtime([
      provider({ id: "first", models: [model("first", "a"), model("first", "b")] }),
      provider({ id: "second", models: [model("second", "c")] }),
    ]);

    expect(models.getModels().map((entry) => entry.id)).toEqual(["a", "b", "c"]);
    expect(models.getModels("first").map((entry) => entry.id)).toEqual(["a", "b"]);
    expect(models.getModels("missing")).toEqual([]);
    expect(models.getModel("second", "c")).toEqual(model("second", "c"));
    expect(models.getModel("second", "missing")).toBeUndefined();
  });

  it("isolates a provider whose model source throws", () => {
    const broken = provider({ id: "broken" });
    broken.getModels = () => {
      throw new Error("model source failed");
    };
    const models = runtime([
      broken,
      provider({ id: "healthy", models: [model("healthy", "available")] }),
    ]);

    expect(models.getModels("broken")).toEqual([]);
    expect(models.getModels().map((entry) => entry.id)).toEqual(["available"]);
  });

  it("returns only authenticated provider models without refreshing OAuth", async () => {
    const credentials = new MemoryCredentials();
    const refresh = vi.fn<OAuthAuth["refresh"]>(async (credential) => credential);
    await credentials.modify("subscription", async () => ({
      type: "oauth",
      access: "expired-access",
      refresh: "refresh-token",
      expires: 0,
    }));
    const models = runtime(
      [
        provider({ id: "environment" }),
        provider({ id: "missing", auth: { apiKey: unavailableApiKeyAuth() } }),
        provider({
          id: "subscription",
          auth: {
            oauth: {
              name: "Subscription",
              login: async () => {
                throw new Error("not used");
              },
              refresh,
              toAuth: async (credential) => ({ apiKey: credential.access }),
            },
          },
        }),
      ],
      credentials,
    );

    await expect(models.checkAuth("subscription")).resolves.toEqual({
      type: "oauth",
      source: "OAuth",
    });
    await expect(models.getAvailable()).resolves.toEqual([
      model("environment", "environment-model"),
      model("subscription", "subscription-model"),
    ]);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("refreshes OAuth once and replays an unauthorized request before emitting its start", async () => {
    const credentials = new MemoryCredentials();
    await credentials.modify("oauth", async () => ({
      type: "oauth",
      access: "stale",
      refresh: "refresh-token",
      expires: Date.now() + 60 * 60_000,
    }));
    const refresh = vi.fn<OAuthAuth["refresh"]>(async (credential) => ({
      ...credential,
      access: "fresh",
      expires: Date.now() + 60 * 60_000,
    }));
    const target = model("oauth", "oauth-model");
    const stream = vi.fn<Provider["stream"]>(async function* (_model, _context, resolved) {
      const message = {
        role: "assistant" as const,
        content: [],
        api: target.api,
        provider: target.provider,
        model: target.id,
        usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
        stopReason: "pending" as const,
        timestamp: 1,
      };
      yield { type: "start" as const, partial: message };
      if (resolved.auth.apiKey === "stale") {
        yield {
          type: "error" as const,
          reason: "error" as const,
          error: { ...message, stopReason: "error" as const, errorMessage: "unauthorized" },
          cause: new APIStatusError(401, "unauthorized"),
        };
        return;
      }
      yield { type: "done" as const, reason: "stop" as const, message: { ...message, stopReason: "stop" as const } };
    });
    const providerEntry = provider({
      id: "oauth",
      models: [target],
      auth: {
        oauth: {
          name: "OAuth",
          login: async () => {
            throw new Error("not used");
          },
          refresh,
          toAuth: async (credential) => ({ apiKey: credential.access }),
        },
      },
    });
    providerEntry.stream = stream;
    const models = runtime([providerEntry], credentials);
    const events = [];
    for await (const event of models.streamSimple(target, { messages: [] })) events.push(event);

    expect(refresh).toHaveBeenCalledOnce();
    expect(stream).toHaveBeenCalledTimes(2);
    expect(events.map((event) => event.type)).toEqual(["start", "done"]);
  });

  it("surfaces a second OAuth 401 without a third request or refresh", async () => {
    const credentials = new MemoryCredentials();
    await credentials.modify("oauth", async () => ({ type: "oauth", access: "stale", refresh: "refresh", expires: Date.now() + 60 * 60_000 }));
    const refresh = vi.fn<OAuthAuth["refresh"]>(async (credential) => ({ ...credential, access: "fresh" }));
    const target = model("oauth", "oauth-model");
    const entry = provider({
      id: "oauth",
      models: [target],
      auth: { oauth: { name: "OAuth", login: async () => { throw new Error("not used"); }, refresh, toAuth: async (credential) => ({ apiKey: credential.access }) } },
    });
    const stream = vi.fn<Provider["stream"]>(async function* () {
      const message = { role: "assistant" as const, content: [], api: target.api, provider: target.provider, model: target.id, usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "pending" as const, timestamp: 1 };
      yield { type: "start" as const, partial: message };
      yield { type: "error" as const, reason: "error" as const, error: { ...message, stopReason: "error" as const, errorMessage: "unauthorized" }, cause: new APIStatusError(401, "unauthorized") };
    });
    entry.stream = stream;
    const events = [];
    for await (const event of runtime([entry], credentials).streamSimple(target, { messages: [] })) events.push(event);
    expect(refresh).toHaveBeenCalledOnce();
    expect(stream).toHaveBeenCalledTimes(2);
    expect(events.map((event) => event.type)).toEqual(["start", "error"]);
  });

  it("does not replay an OAuth request after provider content has been emitted", async () => {
    const credentials = new MemoryCredentials();
    await credentials.modify("oauth", async () => ({ type: "oauth", access: "stale", refresh: "refresh", expires: Date.now() + 60 * 60_000 }));
    const refresh = vi.fn<OAuthAuth["refresh"]>(async (credential) => ({ ...credential, access: "fresh" }));
    const target = model("oauth", "oauth-model");
    const entry = provider({ id: "oauth", models: [target], auth: { oauth: { name: "OAuth", login: async () => { throw new Error("not used"); }, refresh, toAuth: async (credential) => ({ apiKey: credential.access }) } } });
    const stream = vi.fn<Provider["stream"]>(async function* () {
      const message = { role: "assistant" as const, content: [], api: target.api, provider: target.provider, model: target.id, usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "pending" as const, timestamp: 1 };
      yield { type: "start" as const, partial: message };
      yield { type: "text_delta" as const, delta: "partial", partial: message };
      yield { type: "error" as const, reason: "error" as const, error: { ...message, stopReason: "error" as const, errorMessage: "unauthorized" }, cause: new APIStatusError(401, "unauthorized") };
    });
    entry.stream = stream;
    const events = [];
    for await (const event of runtime([entry], credentials).streamSimple(target, { messages: [] })) events.push(event);
    expect(refresh).not.toHaveBeenCalled();
    expect(stream).toHaveBeenCalledOnce();
    expect(events.map((event) => event.type)).toEqual(["start", "text_delta", "error"]);
  });

  it("does not refresh or replay an API-key 401", async () => {
    const target = model("api", "api-model");
    const entry = provider({ id: "api", models: [target] });
    const stream = vi.fn<Provider["stream"]>(async function* () {
      const message = { role: "assistant" as const, content: [], api: target.api, provider: target.provider, model: target.id, usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "pending" as const, timestamp: 1 };
      yield { type: "start" as const, partial: message };
      yield { type: "error" as const, reason: "error" as const, error: { ...message, stopReason: "error" as const, errorMessage: "unauthorized" }, cause: new APIStatusError(401, "unauthorized") };
    });
    entry.stream = stream;
    const events = [];
    for await (const event of runtime([entry]).streamSimple(target, { messages: [] })) events.push(event);
    expect(stream).toHaveBeenCalledOnce();
    expect(events.map((event) => event.type)).toEqual(["start", "error"]);
  });

  it("filters available models with the provider credential", async () => {
    const credentials = new MemoryCredentials();
    await credentials.modify("tiered", async () => ({ type: "api_key", key: "premium-key" }));
    const tiered = provider({
      id: "tiered",
      models: [model("tiered", "standard"), model("tiered", "premium")],
    });
    tiered.filterModels = (models, credential) =>
      credential?.type === "api_key" && credential.key === "premium-key"
        ? models.filter((entry) => entry.id === "premium")
        : models.filter((entry) => entry.id === "standard");
    const models = runtime([tiered], credentials);

    await expect(models.getAvailable("tiered")).resolves.toEqual([model("tiered", "premium")]);
  });

  it("refreshes every authenticated dynamic provider when no provider is selected", async () => {
    const first = vi.fn<NonNullable<Provider["refreshModels"]>>(async () => {});
    const second = vi.fn<NonNullable<Provider["refreshModels"]>>(async () => {});
    const models = runtime([
      provider({ id: "first", refreshModels: first }),
      provider({ id: "second", refreshModels: second }),
    ]);

    await expect(models.refresh({ allowNetwork: true })).resolves.toMatchObject({
      aborted: false,
      errors: new Map(),
    });
    expect(first).toHaveBeenCalledOnce();
    expect(second).toHaveBeenCalledOnce();
  });

  it("refreshes only the selected dynamic provider", async () => {
    const first = vi.fn<NonNullable<Provider["refreshModels"]>>(async () => {});
    const second = vi.fn<NonNullable<Provider["refreshModels"]>>(async () => {});
    const models = runtime([
      provider({ id: "first", refreshModels: first }),
      provider({ id: "second", refreshModels: second }),
    ]);

    await models.refresh({ provider: "second", allowNetwork: true, force: true });

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledOnce();
    expect(second.mock.calls[0]?.[0].force).toBe(true);
  });

  it("does not invoke an unauthenticated provider during network catalog refresh", async () => {
    const refresh = vi.fn<NonNullable<Provider["refreshModels"]>>(async () => {});
    const models = runtime([
      provider({
        id: "unconfigured",
        auth: { apiKey: unavailableApiKeyAuth() },
        refreshModels: refresh,
      }),
    ]);

    await models.refresh({ allowNetwork: true });

    expect(refresh).not.toHaveBeenCalled();
  });

  it("reports cancellation without recording a provider refresh error", async () => {
    const controller = new AbortController();
    const models = runtime([
      provider({
        id: "cancelled",
        refreshModels: async () => {
          controller.abort();
        },
      }),
    ]);

    await expect(models.refresh({ signal: controller.signal })).resolves.toMatchObject({
      aborted: true,
      errors: new Map(),
    });
  });

  it("restores a cached catalog without network authentication", async () => {
    const catalogs = new MemoryCatalogs();
    await catalogs.write("dynamic", {
      models: [model("dynamic", "cached-model")],
      checkedAt: 1,
    });
    let dynamic: readonly Model[] = [];
    const refresh = vi.fn<NonNullable<Provider["refreshModels"]>>(async (context) => {
      dynamic = (await context.store.read())?.models ?? [];
      if (context.allowNetwork) throw new Error("network must stay disabled");
    });
    const dynamicProvider = provider({
      id: "dynamic",
      models: dynamic,
      auth: { apiKey: unavailableApiKeyAuth() },
      refreshModels: refresh,
    });
    dynamicProvider.getModels = () => dynamic;
    const models = runtime([dynamicProvider], new MemoryCredentials(), catalogs);

    await models.refresh({ allowNetwork: false });

    expect(refresh).toHaveBeenCalledOnce();
    expect(models.getModel("dynamic", "cached-model")).toEqual(model("dynamic", "cached-model"));
  });

  it("prefers a stored credential and blocks ambient fallback for an unsupported credential type", async () => {
    const credentials = new MemoryCredentials();
    const models = runtime(
      [
        provider({
          id: "priority",
          auth: {
            apiKey: apiKeyAuth(),
            oauth: {
              name: "Subscription",
              login: async () => {
                throw new Error("not used");
              },
              refresh: async (credential) => credential,
              toAuth: async (credential) => ({ apiKey: credential.access }),
            },
          },
        }),
        provider({ id: "api-only", auth: { apiKey: apiKeyAuth() } }),
      ],
      credentials,
    );

    await credentials.modify("priority", async () => ({
      type: "oauth",
      access: "subscription-token",
      refresh: "refresh-token",
      expires: Date.now() + 60_000,
    }));
    await expect(models.getAuth("priority")).resolves.toMatchObject({
      auth: { apiKey: "subscription-token" },
      source: "OAuth",
    });

    await credentials.modify("priority", async () => ({ type: "api_key", key: "stored-key" }));
    await expect(models.getAuth("priority")).resolves.toMatchObject({
      auth: { apiKey: "stored-key" },
      source: "Stored",
    });

    await credentials.modify("api-only", async () => ({
      type: "oauth",
      access: "stale-subscription",
      refresh: "refresh-token",
      expires: 0,
    }));
    await expect(models.getAuth("api-only")).resolves.toBeUndefined();
  });

  it("persists login credentials and removes them on logout", async () => {
    const credentials = new MemoryCredentials();
    const models = runtime([provider({ id: "login" })], credentials);

    await expect(models.login("login", "api_key", interaction)).resolves.toEqual({
      type: "api_key",
      key: "logged-in-key",
    });
    await expect(credentials.read("login")).resolves.toEqual({
      type: "api_key",
      key: "logged-in-key",
    });

    await models.logout("login");

    await expect(credentials.read("login")).resolves.toBeUndefined();
  });

  it("refreshes an OAuth credential with less than five minutes remaining", async () => {
    const credentials = new MemoryCredentials();
    const refresh = vi.fn<OAuthAuth["refresh"]>(async (credential) => ({
      ...credential,
      access: "fresh-access",
      expires: Date.now() + 60 * 60_000,
    }));
    await credentials.modify("subscription", async () => ({
      type: "oauth",
      access: "soon-expiring-access",
      refresh: "refresh-token",
      expires: Date.now() + 4 * 60_000,
    }));
    const models = runtime(
      [
        provider({
          id: "subscription",
          auth: {
            oauth: {
              name: "Subscription",
              login: async () => {
                throw new Error("not used");
              },
              refresh,
              toAuth: async (credential) => ({ apiKey: credential.access }),
            },
          },
        }),
      ],
      credentials,
    );

    await expect(models.getAuth("subscription")).resolves.toEqual({
      auth: { apiKey: "fresh-access" },
      source: "OAuth",
    });
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("does not modify an OAuth credential with more than five minutes remaining", async () => {
    const credentials = new MemoryCredentials();
    const modify = vi.spyOn(credentials, "modify");
    await credentials.modify("subscription", async () => ({
      type: "oauth",
      access: "valid-access",
      refresh: "refresh-token",
      expires: Date.now() + 6 * 60_000,
    }));
    modify.mockClear();
    const refresh = vi.fn<OAuthAuth["refresh"]>(async (credential) => credential);
    const models = runtime(
      [
        provider({
          id: "subscription",
          auth: {
            oauth: {
              name: "Subscription",
              login: async () => {
                throw new Error("not used");
              },
              refresh,
              toAuth: async (credential) => ({ apiKey: credential.access }),
            },
          },
        }),
      ],
      credentials,
    );

    await expect(models.getAuth("subscription")).resolves.toEqual({
      auth: { apiKey: "valid-access" },
      source: "OAuth",
    });
    expect(modify).not.toHaveBeenCalled();
    expect(refresh).not.toHaveBeenCalled();
  });

  it("shares one expired OAuth refresh between concurrent callers", async () => {
    const credentials = new MemoryCredentials();
    await credentials.modify("subscription", async () => ({
      type: "oauth",
      access: "expired-access",
      refresh: "refresh-token",
      expires: 0,
    }));
    let release!: () => void;
    const waitForRefresh = new Promise<void>((resolve) => {
      release = resolve;
    });
    let markRefreshStarted!: () => void;
    const refreshStarted = new Promise<void>((resolve) => {
      markRefreshStarted = resolve;
    });
    const refresh = vi.fn<OAuthAuth["refresh"]>(async () => {
      markRefreshStarted();
      await waitForRefresh;
      return {
        type: "oauth",
        access: "fresh-access",
        refresh: "rotated-refresh",
        expires: Date.now() + 60 * 60_000,
      };
    });
    const models = runtime(
      [
        provider({
          id: "subscription",
          auth: {
            oauth: {
              name: "Subscription",
              login: async () => {
                throw new Error("not used");
              },
              refresh,
              toAuth: async (credential) => ({ apiKey: credential.access }),
            },
          },
        }),
      ],
      credentials,
    );

    const first = models.getAuth("subscription");
    const second = models.getAuth("subscription");
    await refreshStarted;
    expect(refresh).toHaveBeenCalledOnce();
    release();

    await expect(Promise.all([first, second])).resolves.toEqual([
      { auth: { apiKey: "fresh-access" }, source: "OAuth" },
      { auth: { apiKey: "fresh-access" }, source: "OAuth" },
    ]);
    await expect(credentials.read("subscription")).resolves.toMatchObject({
      access: "fresh-access",
      refresh: "rotated-refresh",
    });
  });

  it("preserves an expired OAuth credential when refresh fails", async () => {
    const credentials = new MemoryCredentials();
    const expired: Credential = {
      type: "oauth",
      access: "expired-access",
      refresh: "refresh-token",
      expires: 0,
    };
    await credentials.modify("subscription", async () => expired);
    const models = runtime(
      [
        provider({
          id: "subscription",
          auth: {
            oauth: {
              name: "Subscription",
              login: async () => {
                throw new Error("not used");
              },
              refresh: async () => {
                throw new Error("refresh rejected");
              },
              toAuth: async (credential) => ({ apiKey: credential.access }),
            },
          },
        }),
      ],
      credentials,
    );

    await expect(models.getAuth("subscription")).rejects.toMatchObject({
      code: ProviderRuntimeErrors.codes.PROVIDER_AUTH_ERROR,
      message: "OAuth refresh failed for subscription: refresh rejected",
      cause: expect.objectContaining({ message: "refresh rejected" }),
    });
    await expect(credentials.read("subscription")).resolves.toEqual(expired);
  });

  it("classifies credential store and API-key resolution failures as authentication errors", async () => {
    const readFailure: CredentialStore = {
      read: async () => {
        throw new Error("disk unavailable");
      },
      list: async () => [],
      modify: async () => undefined,
      delete: async () => {},
    };
    const readModels = runtime([provider({ id: "read-failure" })], readFailure);

    await expect(readModels.getAuth("read-failure")).rejects.toMatchObject({
      code: ProviderRuntimeErrors.codes.PROVIDER_AUTH_ERROR,
      message: "Credential read failed for read-failure: disk unavailable",
    });

    const authFailure: ApiKeyAuth = {
      name: "Broken API key",
      resolve: async () => {
        throw new Error("resolver unavailable");
      },
    };
    const authModels = runtime([provider({ id: "auth-failure", auth: { apiKey: authFailure } })]);

    await expect(authModels.getAuth("auth-failure")).rejects.toMatchObject({
      code: ProviderRuntimeErrors.codes.PROVIDER_AUTH_ERROR,
      message: "API key authentication failed for auth-failure: resolver unavailable",
    });
    await expect(authModels.checkAuth("auth-failure")).rejects.toMatchObject({
      code: ProviderRuntimeErrors.codes.PROVIDER_AUTH_ERROR,
    });
  });
});
