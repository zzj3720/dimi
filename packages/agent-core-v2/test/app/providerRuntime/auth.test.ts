/**
 * Provider runtime OAuth contracts — Dimi, xAI, and OpenAI Codex device-code
 * login, refresh, cancellation and request-auth projection. HTTP and time are
 * local fixtures; run with `vp test -- auth.test.ts`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  anthropicApiKeyAuth,
  anthropicOAuth,
  bedrockAuth,
  cloudflareAIGatewayAuth,
  cloudflareWorkersAIAuth,
  createRadiusOAuth,
  githubCopilotOAuth,
  googleVertexAuth,
  dimiCodingOAuth,
  openRouterOAuth,
  openaiCodexOAuth,
  xaiOAuth,
} from "#/app/providerRuntime/auth";
import type { AuthEvent, AuthInteraction, OAuthCredential } from "#/app/providerRuntime/types";
import { streamProvider } from "#/app/providerRuntime/stream";

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function requestUrl(input: string | URL | Request): string {
  return typeof input === "string"
    ? input
    : input instanceof Request
      ? input.url
      : input.toString();
}

function form(init: RequestInit | undefined): URLSearchParams {
  const body = init?.body;
  if (typeof body === "string" || body instanceof URLSearchParams) {
    return new URLSearchParams(body);
  }
  throw new TypeError("Expected a form request body");
}

function jsonBody(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

function interaction(events: AuthEvent[], signal?: AbortSignal): AuthInteraction {
  return {
    signal,
    prompt: async () => {
      throw new Error("Unexpected prompt");
    },
    notify: (event) => events.push(event),
  };
}

function dimiToken(
  access = "dimi-access",
  refresh = "dimi-refresh",
  expiresIn = 3_600,
): Record<string, unknown> {
  return { access_token: access, refresh_token: refresh, expires_in: expiresIn };
}

function xaiToken(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    access_token: "xai-access",
    refresh_token: "xai-refresh",
    expires_in: 21_600,
    ...overrides,
  };
}

function codexToken(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    access_token: "codex-access",
    refresh_token: "codex-refresh",
    expires_in: 3_600,
    ...overrides,
  };
}

async function advance(ms: number): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("Dimi Coding OAuth", () => {
  it("notifies a device code, tolerates pending polling, and projects the token as a bearer header", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    let pollCount = 0;
    const fetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = requestUrl(input);
      if (url.endsWith("/device_authorization")) {
        expect(form(init).get("grant_type")).toBeNull();
        return json({
          user_code: "KIMI-CODE",
          device_code: "device-code",
          verification_uri: "https://auth.example.test/device",
          verification_uri_complete: "https://auth.example.test/device?code=KIMI-CODE",
          expires_in: 60,
          interval: 1,
        });
      }
      if (url.endsWith("/token")) {
        pollCount += 1;
        expect(form(init).get("grant_type")).toBe("urn:ietf:params:oauth:grant-type:device_code");
        return pollCount === 1 ? json({ error: "authorization_pending" }, 400) : json(dimiToken());
      }
      throw new Error(`Unexpected request: ${url}`);
    });
    vi.stubGlobal("fetch", fetch);
    const events: AuthEvent[] = [];

    const login = dimiCodingOAuth.login(interaction(events));
    await advance(1_000);
    await advance(1_000);

    await expect(login).resolves.toMatchObject({
      type: "oauth",
      access: "dimi-access",
      refresh: "dimi-refresh",
      expires: Date.parse("2026-01-01T00:00:00Z") + 3_602_000,
    });
    expect(events).toEqual([
      {
        type: "device_code",
        userCode: "KIMI-CODE",
        verificationUri: "https://auth.example.test/device?code=KIMI-CODE",
        intervalSeconds: 1,
        expiresInSeconds: 60,
      },
    ]);
    await expect(
      dimiCodingOAuth.toAuth({
        type: "oauth",
        access: "dimi-access",
        refresh: "dimi-refresh",
        expires: 1,
      }),
    ).resolves.toEqual({ headers: { Authorization: "Bearer dimi-access" } });
  });

  it("fails when device authorization is denied", async () => {
    vi.useFakeTimers();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) =>
        requestUrl(input).endsWith("/device_authorization")
          ? json({
              user_code: "KIMI-CODE",
              device_code: "device-code",
              verification_uri_complete: "https://auth.example.test/device",
              expires_in: 60,
              interval: 1,
            })
          : json({ error: "access_denied", error_description: "User rejected sign-in" }, 400),
      ),
    );

    const login = dimiCodingOAuth.login(interaction([]));
    const rejection = expect(login).rejects.toThrow("User rejected sign-in");
    await advance(1_000);

    await rejection;
  });

  it("times out when the device code remains pending", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) =>
        requestUrl(input).endsWith("/device_authorization")
          ? json({
              user_code: "KIMI-CODE",
              device_code: "device-code",
              verification_uri_complete: "https://auth.example.test/device",
              expires_in: 1,
              interval: 1,
            })
          : json({ error: "authorization_pending" }, 400),
      ),
    );

    const login = dimiCodingOAuth.login(interaction([]));
    const rejection = expect(login).rejects.toThrow("Dimi device login timed out");
    await advance(1_000);

    await rejection;
  });

  it("retries a rate-limited refresh and rejects invalid grants", async () => {
    vi.useFakeTimers();
    let attempts = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL | Request, init?: RequestInit) => {
        expect(form(init).get("grant_type")).toBe("refresh_token");
        attempts += 1;
        return attempts === 1
          ? json({ error: "rate_limited" }, 429)
          : json(dimiToken("fresh-access", "fresh-refresh"));
      }),
    );

    const refresh = dimiCodingOAuth.refresh({
      type: "oauth",
      access: "old-access",
      refresh: "old-refresh",
      expires: 0,
    });
    await advance(1_000);

    await expect(refresh).resolves.toMatchObject({
      access: "fresh-access",
      refresh: "fresh-refresh",
    });

    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        json({ error: "invalid_grant", error_description: "expired refresh" }, 400),
      ),
    );
    await expect(
      dimiCodingOAuth.refresh({
        type: "oauth",
        access: "old-access",
        refresh: "old-refresh",
        expires: 0,
      }),
    ).rejects.toThrow("expired refresh");
  });
});

describe("xAI OAuth", () => {
  it("handles pending and slow_down polling before returning a device-flow credential", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    let polls = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
        const url = requestUrl(input);
        if (url.endsWith("/device/code")) {
          expect(form(init).get("referrer")).toBe("dimi");
          return json({
            device_code: "device-code",
            user_code: "XAI-CODE",
            verification_uri: "https://accounts.x.ai/oauth2/device",
            expires_in: 60,
            interval: 1,
          });
        }
        polls += 1;
        return polls === 1
          ? json({ error: "authorization_pending" }, 400)
          : polls === 2
            ? json({ error: "slow_down" }, 400)
            : json(xaiToken());
      }),
    );
    const events: AuthEvent[] = [];

    const login = xaiOAuth.login(interaction(events));
    await advance(1_000);
    await advance(1_000);
    await advance(6_000);

    await expect(login).resolves.toMatchObject({ access: "xai-access", refresh: "xai-refresh" });
    expect(events).toEqual([
      {
        type: "device_code",
        userCode: "XAI-CODE",
        verificationUri: "https://accounts.x.ai/oauth2/device",
        intervalSeconds: 1,
        expiresInSeconds: 60,
      },
    ]);
  });

  it("uses the five-second default when a device response omits interval", async () => {
    vi.useFakeTimers();
    let polls = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) => {
        if (requestUrl(input).endsWith("/device/code")) {
          return json({
            device_code: "device-code",
            user_code: "XAI-CODE",
            verification_uri: "https://accounts.x.ai/oauth2/device",
            expires_in: 60,
          });
        }
        polls += 1;
        return json(xaiToken());
      }),
    );

    const login = xaiOAuth.login(interaction([]));
    await advance(4_999);
    expect(polls).toBe(0);
    await advance(1);

    await expect(login).resolves.toMatchObject({ access: "xai-access" });
    expect(polls).toBe(1);
  });

  it("rejects an untrusted verification URL before polling", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        json({
          device_code: "device-code",
          user_code: "XAI-CODE",
          verification_uri: "http://accounts.x.ai/oauth2/device",
        }),
      ),
    );

    await expect(xaiOAuth.login(interaction([]))).rejects.toThrow("untrusted verification URL");
  });

  it("cancels while waiting for device authorization polling", async () => {
    vi.useFakeTimers();
    const controller = new AbortController();
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        json({
          device_code: "device-code",
          user_code: "XAI-CODE",
          verification_uri: "https://accounts.x.ai/oauth2/device",
          expires_in: 60,
          interval: 5,
        }),
      ),
    );

    const login = xaiOAuth.login(interaction([], controller.signal));
    await advance(0);
    controller.abort();

    await expect(login).rejects.toMatchObject({ name: "AbortError" });
  });

  it("preserves an unrotated refresh token and surfaces structured refresh errors", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => json(xaiToken({ refresh_token: undefined }))),
    );

    await expect(
      xaiOAuth.refresh({ type: "oauth", access: "old-access", refresh: "old-refresh", expires: 0 }),
    ).resolves.toMatchObject({ access: "xai-access", refresh: "old-refresh" });
    await expect(
      xaiOAuth.toAuth({ type: "oauth", access: "xai-access", refresh: "old-refresh", expires: 0 }),
    ).resolves.toEqual({ apiKey: "xai-access" });

    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        json({ error: "invalid_client", error_description: "client not accepted" }, 400),
      ),
    );
    await expect(
      xaiOAuth.refresh({ type: "oauth", access: "old-access", refresh: "old-refresh", expires: 0 }),
    ).rejects.toThrow("invalid_client: client not accepted");
  });
});

describe("OpenAI Codex OAuth", () => {
  it("accepts 403, 404 and pending polls before exchanging a device authorization code", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    let polls = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
        const url = requestUrl(input);
        if (url.endsWith("/usercode")) {
          expect(jsonBody(init)).toMatchObject({ client_id: expect.any(String) });
          return json({ device_auth_id: "device-auth-id", user_code: "CODEX-CODE", interval: 1 });
        }
        if (url.endsWith("/deviceauth/token")) {
          polls += 1;
          if (polls === 1) return json({}, 403);
          if (polls === 2) return json({}, 404);
          if (polls === 3)
            return json({ error: { code: "deviceauth_authorization_pending" } }, 400);
          return json({ authorization_code: "authorization-code", code_verifier: "code-verifier" });
        }
        if (url.endsWith("/oauth/token")) {
          const body = form(init);
          expect(body.get("grant_type")).toBe("authorization_code");
          expect(body.get("code")).toBe("authorization-code");
          expect(body.get("code_verifier")).toBe("code-verifier");
          return json(codexToken());
        }
        throw new Error(`Unexpected request: ${url}`);
      }),
    );
    const events: AuthEvent[] = [];

    const login = openaiCodexOAuth.login(interaction(events));
    await advance(1_000);
    await advance(1_000);
    await advance(1_000);
    await advance(1_000);

    await expect(login).resolves.toMatchObject({
      access: "codex-access",
      refresh: "codex-refresh",
    });
    expect(events).toEqual([
      {
        type: "device_code",
        userCode: "CODEX-CODE",
        verificationUri: "https://auth.openai.com/codex/device",
        intervalSeconds: 1,
        expiresInSeconds: 900,
      },
    ]);
    await expect(
      openaiCodexOAuth.toAuth({
        type: "oauth",
        access: "codex-access",
        refresh: "codex-refresh",
        expires: 0,
      }),
    ).resolves.toEqual({ apiKey: "codex-access" });
  });

  it("cancels while waiting for the next Codex device poll", async () => {
    vi.useFakeTimers();
    const controller = new AbortController();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) =>
        requestUrl(input).endsWith("/usercode")
          ? json({ device_auth_id: "device-auth-id", user_code: "CODEX-CODE", interval: 5 })
          : json({}, 403),
      ),
    );

    const login = openaiCodexOAuth.login(interaction([], controller.signal));
    await advance(0);
    controller.abort();

    await expect(login).rejects.toMatchObject({ name: "AbortError" });
  });

  it("times out after the device-code lifetime when authorization remains pending", async () => {
    vi.useFakeTimers();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) =>
        requestUrl(input).endsWith("/usercode")
          ? json({ device_auth_id: "device-auth-id", user_code: "CODEX-CODE", interval: 900 })
          : json({}, 403),
      ),
    );

    const login = openaiCodexOAuth.login(interaction([]));
    const rejection = expect(login).rejects.toThrow("OpenAI Codex device login timed out");
    await advance(900_000);

    await rejection;
  });

  it("reports refresh failures without writing to stderr", async () => {
    const stderr = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => json({ error: "invalid_grant" }, 400)),
    );

    await expect(
      openaiCodexOAuth.refresh({
        type: "oauth",
        access: "old-access",
        refresh: "old-refresh",
        expires: 0,
      }),
    ).rejects.toThrow("invalid_grant");

    expect(stderr).not.toHaveBeenCalled();
  });
});

describe("loopback OAuth login", () => {
  it("accepts an OpenRouter browser callback without requiring a pasted redirect URL", async () => {
    const nativeFetch = globalThis.fetch;
    const events: AuthEvent[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
        expect(requestUrl(input)).toBe("https://openrouter.ai/api/v1/auth/keys");
        expect(jsonBody(init)).toMatchObject({ code: "browser-code", code_challenge_method: "S256" });
        return json({ key: "openrouter-key" });
      }),
    );
    const login = openRouterOAuth.login({
      prompt: async (prompt) => new Promise<string>((_resolve, reject) => {
        prompt.signal?.addEventListener("abort", () => reject(new DOMException("cancelled", "AbortError")), { once: true });
      }),
      notify: (event) => events.push(event),
    });

    await vi.waitFor(() => expect(events.find((event) => event.type === "auth_url")).toBeDefined());
    const event = events.find((entry): entry is Extract<AuthEvent, { type: "auth_url" }> => entry.type === "auth_url")!;
    const callback = new URL(new URL(event.url).searchParams.get("callback_url")!);
    callback.searchParams.set("code", "browser-code");
    const response = await nativeFetch(callback);
    expect(await response.text()).toContain("Signed in");

    await expect(login).resolves.toMatchObject({ type: "oauth", access: "openrouter-key" });
  });

  it("cancels the OpenRouter loopback listener after manual code entry wins the race", async () => {
    const nativeFetch = globalThis.fetch;
    const events: AuthEvent[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) => {
        expect(requestUrl(input)).toBe("https://openrouter.ai/api/v1/auth/keys");
        return json({ key: "manual-openrouter-key" });
      }),
    );
    await expect(
      openRouterOAuth.login({
        prompt: async () => "manual-code",
        notify: (event) => events.push(event),
      }),
    ).resolves.toMatchObject({ access: "manual-openrouter-key" });
    const event = events.find((entry): entry is Extract<AuthEvent, { type: "auth_url" }> => entry.type === "auth_url")!;
    const callback = new URL(new URL(event.url).searchParams.get("callback_url")!);
    await expect(nativeFetch(callback)).rejects.toThrow();
  });

  it("discovers Radius browser OAuth and exchanges its state-bound loopback callback", async () => {
    const nativeFetch = globalThis.fetch;
    const events: AuthEvent[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
        const url = requestUrl(input);
        if (url === "https://radius.example.test/v1/oauth") {
          return json({ authorizationEndpoint: "https://auth.radius.example.test/authorize" });
        }
        if (url === "https://radius.example.test/v1/oauth/token") {
          expect(form(init).get("grant_type")).toBe("authorization_code");
          expect(form(init).get("code")).toBe("radius-code");
          return json({ access_token: "radius-access", refresh_token: "radius-refresh", expires_in: 3600 });
        }
        throw new Error(`Unexpected URL: ${url}`);
      }),
    );
    const radius = createRadiusOAuth("https://radius.example.test", "Example Radius");
    const login = radius.login({
      prompt: async (prompt) => prompt.type === "select" ? "browser" : "",
      notify: (event) => events.push(event),
    });

    await vi.waitFor(() => expect(events.find((event) => event.type === "auth_url")).toBeDefined());
    const event = events.find((entry): entry is Extract<AuthEvent, { type: "auth_url" }> => entry.type === "auth_url")!;
    const authorize = new URL(event.url);
    const callback = new URL(authorize.searchParams.get("redirect_uri")!);
    expect(authorize.searchParams.get("state")).not.toBeNull();
    callback.searchParams.set("state", authorize.searchParams.get("state")!);
    callback.searchParams.set("code", "radius-code");
    const response = await nativeFetch(callback);
    expect(await response.text()).toContain("Signed in");

    await expect(login).resolves.toMatchObject({ type: "oauth", access: "radius-access", refresh: "radius-refresh" });
  });
});

describe("provider-native authentication", () => {
  it("requires and materializes Cloudflare Workers AI account credentials", async () => {
    const auth = cloudflareWorkersAIAuth();
    const missing = await auth.resolve({
      ctx: { env: async (name) => name === "CLOUDFLARE_API_KEY" ? "example-key" : undefined, fileExists: async () => false },
    });
    expect(missing).toBeUndefined();

    await expect(auth.resolve({
      ctx: {
        env: async (name) => ({ CLOUDFLARE_API_KEY: "example-key", CLOUDFLARE_ACCOUNT_ID: "example-account" })[name],
        fileExists: async () => false,
      },
    })).resolves.toEqual({
      auth: {
        apiKey: "example-key",
        baseUrl: "https://api.cloudflare.com/client/v4/accounts/example-account/ai/v1",
      },
      env: { CLOUDFLARE_ACCOUNT_ID: "example-account" },
      source: "CLOUDFLARE_API_KEY",
    });
  });

  it("uses Cloudflare AI Gateway's scoped endpoint and gateway auth header", async () => {
    const auth = cloudflareAIGatewayAuth();
    await expect(auth.resolve({
      credential: {
        type: "api_key",
        key: "example-key",
        env: { CLOUDFLARE_ACCOUNT_ID: "example-account", CLOUDFLARE_GATEWAY_ID: "example-gateway" },
      },
      ctx: { env: async () => undefined, fileExists: async () => false },
    })).resolves.toEqual({
      auth: {
        baseUrl: "https://gateway.ai.cloudflare.com/v1/example-account/example-gateway/compat",
        headers: {
          "cf-aig-authorization": "Bearer example-key",
          Authorization: null,
          "x-api-key": null,
        },
      },
      env: { CLOUDFLARE_ACCOUNT_ID: "example-account", CLOUDFLARE_GATEWAY_ID: "example-gateway" },
      source: "Stored Cloudflare API key",
    });
  });

  it("sends Cloudflare AI Gateway credentials without an upstream Authorization header", async () => {
    let url: string | undefined;
    let headers: Headers | undefined;
    vi.stubGlobal("fetch", vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      url = requestUrl(input);
      headers = new Headers(init?.headers);
      return new Response("data: [DONE]\\n\\n", { status: 200 });
    }));
    const model = {
      id: "workers-ai/example-model",
      name: "Example model",
      api: "openai-completions" as const,
      provider: "cloudflare-ai-gateway",
      baseUrl: "https://gateway.ai.cloudflare.com/v1",
      reasoning: false,
      input: ["text"] as const,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 128_000,
      maxTokens: 8_192,
    };
    const resolved = await cloudflareAIGatewayAuth().resolve({
      credential: {
        type: "api_key",
        key: "example-key",
        env: { CLOUDFLARE_ACCOUNT_ID: "example-account", CLOUDFLARE_GATEWAY_ID: "example-gateway" },
      },
      ctx: { env: async () => undefined, fileExists: async () => false },
    });
    if (resolved === undefined) throw new Error("expected Cloudflare authentication");
    for await (const _event of streamProvider(model, { messages: [] }, resolved)) {
      // The request URL and headers are the public wire contract.
    }
    expect(url).toBe("https://gateway.ai.cloudflare.com/v1/example-account/example-gateway/compat/chat/completions");
    expect(headers?.get("cf-aig-authorization")).toBe("Bearer example-key");
    expect(headers?.get("authorization")).toBeNull();
    expect(headers?.get("x-api-key")).toBeNull();
  });

  it("resolves the Anthropic gateway token ahead of OAuth and API-key environment values", async () => {
    const auth = await anthropicApiKeyAuth().resolve({
      ctx: {
        env: async (name) => ({
          ANTHROPIC_AUTH_TOKEN: "gateway-token",
          ANTHROPIC_OAUTH_TOKEN: "oauth-token",
          ANTHROPIC_API_KEY: "api-key",
        })[name],
        fileExists: async () => false,
      },
    });

    expect(auth).toEqual({
      auth: { headers: { Authorization: "Bearer gateway-token" } },
      source: "ANTHROPIC_AUTH_TOKEN",
    });
  });

  it("projects Anthropic subscription credentials with the OAuth beta protocol header", async () => {
    await expect(
      anthropicOAuth.toAuth({ type: "oauth", access: "oauth-access", refresh: "refresh", expires: 1 }),
    ).resolves.toEqual({
      headers: {
        Authorization: "Bearer oauth-access",
        "anthropic-beta": "claude-code-20250219,oauth-2025-04-20",
        "user-agent": "dimi",
        "x-app": "cli",
      },
    });
  });

  it("sends an Anthropic OAuth credential as bearer auth rather than x-api-key", async () => {
    let headers: Headers | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_input: string | URL | Request, init?: RequestInit) => {
      headers = new Headers(init?.headers);
      return new Response("data: {\"type\":\"message_stop\"}\n\n", { status: 200 });
    }));
    const model = {
      id: "claude-test",
      name: "Claude test",
      api: "anthropic-messages" as const,
      provider: "anthropic",
      baseUrl: "https://anthropic.example.test/v1",
      reasoning: false,
      input: ["text"] as const,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 200_000,
      maxTokens: 8_192,
    };

    for await (const _event of streamProvider(model, { messages: [] }, await anthropicOAuth.toAuth({ type: "oauth", access: "oauth-access", refresh: "refresh", expires: 1 }).then((auth) => ({ auth })))) {
      // The request headers are the observable contract.
    }
    expect(headers?.get("authorization")).toBe("Bearer oauth-access");
    expect(headers?.get("x-api-key")).toBeNull();
    expect(headers?.get("anthropic-beta")).toContain("oauth-2025-04-20");
    expect(headers?.get("user-agent")).toBe("dimi");
    expect(headers?.get("x-app")).toBe("cli");
  });

  it("does not add the Anthropic OAuth identity headers to a normal API key", async () => {
    await expect(
      anthropicApiKeyAuth().resolve({
        credential: { type: "api_key", key: "sk-ant-api" },
        ctx: { env: async () => undefined, fileExists: async () => false },
      }),
    ).resolves.toEqual({ auth: { apiKey: "sk-ant-api" }, env: undefined, source: "Stored API key" });
  });

  it("selects a persisted AWS profile without treating it as an API key", async () => {
    const auth = await bedrockAuth.resolve({
      credential: { type: "api_key", env: { AWS_PROFILE: "engineering" } },
      ctx: { env: async () => undefined, fileExists: async () => false },
    });

    expect(auth).toEqual({ auth: {}, env: { AWS_PROFILE: "engineering" }, source: "Stored AWS profile" });
  });

  it("keeps a saved AWS credential-chain selection available for request-time resolution", async () => {
    await expect(
      bedrockAuth.resolve({
        credential: { type: "api_key" },
        ctx: { env: async () => undefined, fileExists: async () => false },
      }),
    ).resolves.toEqual({ auth: {}, env: undefined, source: "Stored AWS credential-chain selection" });
  });

  it("recognizes the shared AWS credentials file without probing the network", async () => {
    await expect(
      bedrockAuth.resolve({
        ctx: {
          env: async () => undefined,
          fileExists: async (path) => path === "~/.aws/credentials",
        },
      }),
    ).resolves.toEqual({ auth: {}, source: "~/.aws/credentials" });
  });

  it("resolves the standard gcloud ADC path through the authority supplied file lookup", async () => {
    await expect(
      googleVertexAuth.resolve({
        ctx: {
          env: async (name) => ({ GOOGLE_CLOUD_PROJECT: "example-project", GOOGLE_CLOUD_LOCATION: "us-central1" })[name],
          fileExists: async (path) => path === "~/.config/gcloud/application_default_credentials.json",
        },
      }),
    ).resolves.toEqual({
      auth: {},
      env: { GOOGLE_CLOUD_PROJECT: "example-project", GOOGLE_CLOUD_LOCATION: "us-central1" },
      source: "Google Application Default Credentials",
    });
  });

  it("keeps OpenRouter keys issued by OAuth permanently usable without a fake refresh", async () => {
    const credential = { type: "oauth" as const, access: "sk-or-issued", refresh: "", expires: Number.MAX_SAFE_INTEGER };
    await expect(openRouterOAuth.refresh(credential)).resolves.toEqual(credential);
    await expect(openRouterOAuth.toAuth(credential)).resolves.toEqual({ apiKey: "sk-or-issued" });
  });

  it("uses the Copilot access-token endpoint and preserves its discovered proxy endpoint", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) => {
        const url = requestUrl(input);
        if (url === "https://copilot-api.example.test/copilot_internal/v2/token") {
          return json({ token: "tid=x;proxy-ep=proxy.example.test;", expires_at: 1_800_000_000 });
        }
        expect(url).toBe("https://api.example.test/models");
        return json({ data: [{ id: "claude", model_picker_enabled: true, policy: { state: "enabled" }, capabilities: { supports: { tool_calls: true } } }] });
      }),
    );

    const credential = await githubCopilotOAuth.refresh({
      type: "oauth",
      access: "old",
      refresh: "refresh-token",
      expires: 0,
      enterpriseUrl: "https://example.test",
    });

    await expect(githubCopilotOAuth.toAuth(credential)).resolves.toEqual({
      apiKey: "tid=x;proxy-ep=proxy.example.test;",
      baseUrl: "https://api.example.test",
    });
  });

  it("builds Radius OAuth auth against the configured gateway rather than an API-key surrogate", async () => {
    const radius = createRadiusOAuth("https://radius.example.test", "Example Radius");
    await expect(
      radius.toAuth({ type: "oauth", access: "radius-access", refresh: "radius-refresh", expires: 1 }),
    ).resolves.toEqual({ apiKey: "radius-access" });
    expect(radius.loginLabel).toBe("Sign in with Example Radius");
  });
});
