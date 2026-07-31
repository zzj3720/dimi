/**
 * Provider runtime OAuth contracts — Kimi, xAI, and OpenAI Codex device-code
 * login, refresh, cancellation and request-auth projection. HTTP and time are
 * local fixtures; run with `vp test -- auth.test.ts`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { kimiCodingOAuth, openaiCodexOAuth, xaiOAuth } from "#/app/providerRuntime/auth";
import type { AuthEvent, AuthInteraction, OAuthCredential } from "#/app/providerRuntime/types";

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

function kimiToken(
  access = "kimi-access",
  refresh = "kimi-refresh",
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

describe("Kimi Coding OAuth", () => {
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
        return pollCount === 1 ? json({ error: "authorization_pending" }, 400) : json(kimiToken());
      }
      throw new Error(`Unexpected request: ${url}`);
    });
    vi.stubGlobal("fetch", fetch);
    const events: AuthEvent[] = [];

    const login = kimiCodingOAuth.login(interaction(events));
    await advance(1_000);
    await advance(1_000);

    await expect(login).resolves.toMatchObject({
      type: "oauth",
      access: "kimi-access",
      refresh: "kimi-refresh",
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
      kimiCodingOAuth.toAuth({
        type: "oauth",
        access: "kimi-access",
        refresh: "kimi-refresh",
        expires: 1,
      }),
    ).resolves.toEqual({ headers: { Authorization: "Bearer kimi-access" } });
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

    const login = kimiCodingOAuth.login(interaction([]));
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

    const login = kimiCodingOAuth.login(interaction([]));
    const rejection = expect(login).rejects.toThrow("Kimi device login timed out");
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
          : json(kimiToken("fresh-access", "fresh-refresh"));
      }),
    );

    const refresh = kimiCodingOAuth.refresh({
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
      kimiCodingOAuth.refresh({
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
          expect(form(init).get("referrer")).toBe("kimi-code");
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
