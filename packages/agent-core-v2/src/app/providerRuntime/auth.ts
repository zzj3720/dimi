import {
  KIMI_CODE_FLOW_CONFIG,
  pollDeviceToken,
  refreshAccessToken,
  requestDeviceAuthorization,
} from "@moonshot-ai/kimi-code-oauth";

import type { ApiKeyAuth, OAuthAuth, OAuthCredential } from "./types";

export function envApiKeyAuth(name: string, envNames: readonly string[]): ApiKeyAuth {
  return {
    name,
    login: async (interaction) => ({
      type: "api_key",
      key: await interaction.prompt({
        type: "secret",
        message: `Enter ${name}:`,
      }),
    }),
    check: async ({ ctx, credential }) => {
      if (credential?.key?.trim()) return { type: "api_key", source: "Stored API key" };
      for (const env of envNames) {
        if ((await ctx.env(env))?.trim()) return { type: "api_key", source: env };
      }
      return undefined;
    },
    resolve: async ({ ctx, credential }) => {
      const stored = credential?.key?.trim();
      if (stored) return { auth: { apiKey: stored }, source: "Stored API key" };
      for (const env of envNames) {
        const value = (await ctx.env(env))?.trim();
        if (value) return { auth: { apiKey: value }, source: env };
      }
      return undefined;
    },
  };
}

export const kimiCodingOAuth: OAuthAuth = {
  name: "Kimi Code (subscription)",
  loginLabel: "Sign in with Kimi Code",
  login: async (interaction) => {
    const device = await requestDeviceAuthorization(KIMI_CODE_FLOW_CONFIG, {});
    interaction.notify({
      type: "device_code",
      userCode: device.userCode,
      verificationUri: device.verificationUriComplete || device.verificationUri,
      intervalSeconds: device.interval,
      expiresInSeconds: device.expiresIn ?? undefined,
    });
    const deadline = Date.now() + (device.expiresIn ?? 15 * 60) * 1_000;
    let interval = Math.max(1, device.interval) * 1_000;
    while (Date.now() < deadline) {
      interaction.signal?.throwIfAborted();
      await abortableSleep(interval, interaction.signal);
      const result = await pollDeviceToken(KIMI_CODE_FLOW_CONFIG, device.deviceCode, {});
      if (result.kind === "success") {
        return {
          type: "oauth",
          access: result.token.accessToken,
          refresh: result.token.refreshToken,
          expires: result.token.expiresAt * 1_000,
        };
      }
      if (result.kind === "expired") throw new Error("Kimi device code expired");
      if (result.kind === "denied") throw new Error(result.description || "Kimi login denied");
      if (result.errorCode === "slow_down") interval += 5_000;
    }
    throw new Error("Kimi device login timed out");
  },
  refresh: async (credential) => {
    const token = await refreshAccessToken(KIMI_CODE_FLOW_CONFIG, credential.refresh, {});
    return {
      type: "oauth",
      access: token.accessToken,
      refresh: token.refreshToken,
      expires: token.expiresAt * 1_000,
    };
  },
  toAuth: async (credential) => ({
    headers: { Authorization: `Bearer ${credential.access}` },
  }),
};

const XAI_CLIENT_ID = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_SCOPE = "openid profile email offline_access grok-cli:access api:access";
const XAI_DEVICE_CODE_URL = "https://auth.x.ai/oauth2/device/code";
const XAI_TOKEN_URL = "https://auth.x.ai/oauth2/token";

export const xaiOAuth: OAuthAuth = {
  name: "xAI (Grok/X subscription)",
  loginLabel: "Sign in with SuperGrok or X Premium",
  login: async (interaction) => {
    const deviceResponse = await postForm(
      XAI_DEVICE_CODE_URL,
      { client_id: XAI_CLIENT_ID, scope: XAI_SCOPE, referrer: "kimi-code" },
      interaction.signal,
    );
    if (!deviceResponse.ok) throw oauthFailure("xAI device authorization", deviceResponse);
    const deviceCode = requiredString(deviceResponse.body, "device_code");
    const userCode = requiredString(deviceResponse.body, "user_code");
    const verificationUri = trustedHttps(requiredString(deviceResponse.body, "verification_uri"));
    const complete =
      typeof deviceResponse.body["verification_uri_complete"] === "string"
        ? trustedHttps(deviceResponse.body["verification_uri_complete"])
        : verificationUri;
    const expiresIn = positiveNumber(deviceResponse.body["expires_in"]) ?? 900;
    let interval = (positiveNumber(deviceResponse.body["interval"]) ?? 5) * 1_000;
    interaction.notify({
      type: "device_code",
      userCode,
      verificationUri: complete,
      intervalSeconds: interval / 1_000,
      expiresInSeconds: expiresIn,
    });
    const deadline = Date.now() + expiresIn * 1_000;
    while (Date.now() < deadline) {
      await abortableSleep(interval, interaction.signal);
      const token = await postForm(
        XAI_TOKEN_URL,
        {
          grant_type: "urn:ietf:params:oauth:grant-type:device_code",
          client_id: XAI_CLIENT_ID,
          device_code: deviceCode,
        },
        interaction.signal,
      );
      if (token.ok) return oauthCredential(token.body);
      const code = token.body["error"];
      if (code === "authorization_pending") continue;
      if (code === "slow_down") {
        interval += 5_000;
        continue;
      }
      throw oauthFailure("xAI token polling", token);
    }
    throw new Error("xAI device login timed out");
  },
  refresh: async (credential, signal) => {
    const response = await postForm(
      XAI_TOKEN_URL,
      {
        grant_type: "refresh_token",
        client_id: XAI_CLIENT_ID,
        refresh_token: credential.refresh,
      },
      signal,
    );
    if (!response.ok) throw oauthFailure("xAI token refresh", response);
    return oauthCredential(response.body, credential.refresh);
  },
  toAuth: async (credential) => ({ apiKey: credential.access }),
};

const OPENAI_CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTH_BASE = "https://auth.openai.com";
const OPENAI_DEVICE_CODE_TIMEOUT_SECONDS = 15 * 60;

export const openaiCodexOAuth: OAuthAuth = {
  name: "OpenAI (ChatGPT Plus/Pro)",
  loginLabel: "Sign in with ChatGPT",
  login: async (interaction) => {
    const start = await fetch(`${OPENAI_AUTH_BASE}/api/accounts/deviceauth/usercode`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ client_id: OPENAI_CLIENT_ID }),
      signal: interaction.signal,
    });
    const device = await jsonObject(start);
    if (!start.ok) throw new Error(`OpenAI device authorization failed (HTTP ${start.status})`);
    const deviceAuthId = requiredString(device, "device_auth_id");
    const userCode = requiredString(device, "user_code");
    const interval = positiveNumber(device["interval"]) ?? 5;
    interaction.notify({
      type: "device_code",
      userCode,
      verificationUri: `${OPENAI_AUTH_BASE}/codex/device`,
      intervalSeconds: interval,
      expiresInSeconds: OPENAI_DEVICE_CODE_TIMEOUT_SECONDS,
    });
    const deadline = Date.now() + OPENAI_DEVICE_CODE_TIMEOUT_SECONDS * 1_000;
    while (Date.now() < deadline) {
      await abortableSleep(interval * 1_000, interaction.signal);
      const poll = await fetch(`${OPENAI_AUTH_BASE}/api/accounts/deviceauth/token`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          device_auth_id: deviceAuthId,
          user_code: userCode,
        }),
        signal: interaction.signal,
      });
      if (poll.status === 403 || poll.status === 404) continue;
      const data = await jsonObject(poll);
      if (!poll.ok) {
        const code = errorCode(data);
        if (code === "deviceauth_authorization_pending") continue;
        if (code === "slow_down") continue;
        throw new Error(`OpenAI device authorization failed (HTTP ${poll.status})`);
      }
      const authorizationCode = requiredString(data, "authorization_code");
      const codeVerifier = requiredString(data, "code_verifier");
      return exchangeOpenAI(
        authorizationCode,
        codeVerifier,
        `${OPENAI_AUTH_BASE}/deviceauth/callback`,
        interaction.signal,
      );
    }
    throw new Error("OpenAI Codex device login timed out");
  },
  refresh: async (credential, signal) => {
    const response = await postForm(
      `${OPENAI_AUTH_BASE}/oauth/token`,
      {
        grant_type: "refresh_token",
        refresh_token: credential.refresh,
        client_id: OPENAI_CLIENT_ID,
      },
      signal,
    );
    if (!response.ok) throw oauthFailure("OpenAI Codex token refresh", response);
    return oauthCredential(response.body, credential.refresh, true);
  },
  toAuth: async (credential) => ({ apiKey: credential.access }),
};

async function exchangeOpenAI(
  code: string,
  verifier: string,
  redirectUri: string,
  signal?: AbortSignal,
): Promise<OAuthCredential> {
  const response = await postForm(
    `${OPENAI_AUTH_BASE}/oauth/token`,
    {
      grant_type: "authorization_code",
      client_id: OPENAI_CLIENT_ID,
      code,
      code_verifier: verifier,
      redirect_uri: redirectUri,
    },
    signal,
  );
  if (!response.ok) throw oauthFailure("OpenAI Codex token exchange", response);
  return oauthCredential(response.body, undefined, true);
}

interface FormResponse {
  ok: boolean;
  status: number;
  body: Record<string, unknown>;
}

async function postForm(
  url: string,
  fields: Record<string, string>,
  signal?: AbortSignal,
): Promise<FormResponse> {
  const response = await fetch(url, {
    method: "POST",
    headers: { accept: "application/json", "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams(fields),
    signal,
  });
  return { ok: response.ok, status: response.status, body: await jsonObject(response) };
}

async function jsonObject(response: Response): Promise<Record<string, unknown>> {
  const value: unknown = await response.json().catch(() => ({}));
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function oauthCredential(
  body: Record<string, unknown>,
  previousRefresh?: string,
  requireRefresh = false,
): OAuthCredential {
  const access = requiredString(body, "access_token");
  const refresh =
    typeof body["refresh_token"] === "string" ? body["refresh_token"] : previousRefresh;
  if (requireRefresh && refresh === undefined) {
    throw new Error("OAuth response missing refresh_token");
  }
  return {
    type: "oauth",
    access,
    refresh: refresh ?? "",
    expires: Date.now() + (positiveNumber(body["expires_in"]) ?? 3_600) * 1_000 - 60_000,
  };
}

function requiredString(body: Record<string, unknown>, field: string): string {
  const value = body[field];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`OAuth response missing ${field}`);
  }
  return value;
}

function positiveNumber(value: unknown): number | undefined {
  const parsed = typeof value === "string" ? Number(value) : value;
  return typeof parsed === "number" && Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
}

function trustedHttps(raw: string): string {
  const url = new URL(raw);
  if (url.protocol !== "https:") throw new Error("OAuth returned an untrusted verification URL");
  return url.href;
}

function oauthFailure(action: string, response: FormResponse): Error {
  const code = typeof response.body["error"] === "string" ? response.body["error"] : undefined;
  const detail =
    typeof response.body["error_description"] === "string"
      ? response.body["error_description"]
      : undefined;
  return new Error(
    `${action} failed (HTTP ${response.status})${code || detail ? `: ${[code, detail].filter(Boolean).join(": ")}` : ""}`,
  );
}

function errorCode(body: Record<string, unknown>): string | undefined {
  const error = body["error"];
  if (typeof error === "string") return error;
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = (error as { code?: unknown }).code;
    return typeof code === "string" ? code : undefined;
  }
  return undefined;
}

function abortableSleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException("Login cancelled", "AbortError"));
      return;
    }
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        reject(new DOMException("Login cancelled", "AbortError"));
      },
      { once: true },
    );
  });
}
