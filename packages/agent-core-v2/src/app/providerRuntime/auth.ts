import {
  KIMI_CODE_FLOW_CONFIG,
  pollDeviceToken,
  refreshAccessToken,
  requestDeviceAuthorization,
} from "@moonshot-ai/kimi-code-oauth";
import { createServer, type Server, type ServerResponse } from "node:http";

import type { ApiKeyAuth, AuthInteraction, OAuthAuth, OAuthCredential } from "./types";

const ANTHROPIC_CLIENT_ID = "9d1c25aa-e6a1-44d9-88ed-594d41962f5e";
const ANTHROPIC_AUTHORIZE_URL = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL = "https://platform.claude.com/v1/oauth/token";
const ANTHROPIC_REDIRECT_URI = "http://localhost:53692/callback";
const ANTHROPIC_SCOPES =
  "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
const OPENROUTER_AUTHORIZE_URL = "https://openrouter.ai/auth";
const OPENROUTER_TOKEN_URL = "https://openrouter.ai/api/v1/auth/keys";
const RADIUS_CLIENT_ID = "pi-gateway";
const RADIUS_SCOPE = "gateway offline_access";
const LOOPBACK_LOGIN_TIMEOUT_MS = 5 * 60_000;
const COPILOT_CLIENT_ID = "Iv1.b507a08c87ecfe98";
const COPILOT_HEADERS = {
  "User-Agent": "GitHubCopilotChat/0.35.0",
  "Editor-Version": "vscode/1.107.0",
  "Editor-Plugin-Version": "copilot-chat/0.35.0",
  "Copilot-Integration-Id": "vscode-chat",
} as const;

/** Anthropic keeps gateway bearer auth distinct from OAuth/API-key authentication. */
export function anthropicApiKeyAuth(): ApiKeyAuth {
  return {
    name: "Anthropic API key",
    login: async (interaction) => ({
      type: "api_key",
      key: await interaction.prompt({ type: "secret", message: "Enter Anthropic API key:" }),
    }),
    resolve: async ({ ctx, credential }) => {
      if (credential?.key?.trim()) {
        return { auth: { apiKey: credential.key.trim() }, env: credential.env, source: "Stored API key" };
      }
      const gatewayToken = (await ctx.env("ANTHROPIC_AUTH_TOKEN"))?.trim();
      if (gatewayToken) {
        return { auth: { headers: { Authorization: `Bearer ${gatewayToken}` } as Record<string, string> }, source: "ANTHROPIC_AUTH_TOKEN" };
      }
      const oauthToken = (await ctx.env("ANTHROPIC_OAUTH_TOKEN"))?.trim();
      if (oauthToken) {
        return {
          auth: { headers: anthropicOAuthHeaders(oauthToken) },
          source: "ANTHROPIC_OAUTH_TOKEN",
        };
      }
      const apiKey = (await ctx.env("ANTHROPIC_API_KEY"))?.trim();
      return apiKey === undefined || apiKey === "" ? undefined : { auth: { apiKey }, source: "ANTHROPIC_API_KEY" };
    },
  };
}

/** AWS SDK credential-chain discovery is owned by the request adapter; this owns the persisted selection. */
export const bedrockAuth: ApiKeyAuth = {
  name: "AWS credentials or bearer token",
  login: async (interaction) => {
    const method = await interaction.prompt({
      type: "select",
      message: "Select Amazon Bedrock authentication method:",
      options: [
        { id: "bearer-token", label: "Bearer token" },
        { id: "aws-profile", label: "AWS profile" },
        { id: "credential-chain", label: "Existing AWS credential chain" },
      ],
    });
    if (method === "bearer-token") {
      return { type: "api_key", key: await interaction.prompt({ type: "secret", message: "Enter Amazon Bedrock bearer token:" }) };
    }
    if (method === "aws-profile") {
      return { type: "api_key", env: { AWS_PROFILE: await interaction.prompt({ type: "text", message: "Enter AWS profile name:" }) } };
    }
    if (method !== "credential-chain") throw new Error(`Unknown Amazon Bedrock auth method: ${method}`);
    interaction.notify({ type: "info", message: "Amazon Bedrock uses the AWS default credential chain (profile, IAM keys, ECS/EKS role, or web identity)." });
    return { type: "api_key" };
  },
  resolve: async ({ ctx, credential }) => {
    if (credential?.key?.trim()) return { auth: { apiKey: credential.key.trim() }, env: credential.env, source: "Stored bearer token" };
    const bearer = (await ctx.env("AWS_BEARER_TOKEN_BEDROCK"))?.trim();
    if (bearer) return { auth: { headers: { Authorization: `Bearer ${bearer}` } as Record<string, string> }, env: credential?.env, source: "AWS_BEARER_TOKEN_BEDROCK" };
    const profile = credential?.env?.["AWS_PROFILE"] ?? (await ctx.env("AWS_PROFILE"));
    if (profile?.trim()) return { auth: {}, env: { ...credential?.env, AWS_PROFILE: profile.trim() }, source: credential?.env?.["AWS_PROFILE"] ? "Stored AWS profile" : "AWS_PROFILE" };
    if ((await ctx.env("AWS_ACCESS_KEY_ID"))?.trim() && (await ctx.env("AWS_SECRET_ACCESS_KEY"))?.trim()) return { auth: {}, source: "AWS access keys" };
    for (const variable of ["AWS_CONTAINER_CREDENTIALS_RELATIVE_URI", "AWS_CONTAINER_CREDENTIALS_FULL_URI", "AWS_WEB_IDENTITY_TOKEN_FILE"]) {
      if ((await ctx.env(variable))?.trim()) return { auth: {}, source: variable };
    }
    if (credential !== undefined) return { auth: {}, env: credential.env, source: "Stored AWS credential-chain selection" };
    if (await ctx.fileExists("~/.aws/credentials")) return { auth: {}, source: "~/.aws/credentials" };
    if (await ctx.fileExists("~/.aws/config")) return { auth: {}, source: "~/.aws/config" };
    return undefined;
  },
};

export const googleVertexAuth: ApiKeyAuth = {
  name: "Google Cloud credentials",
  login: async (interaction) => {
    const method = await interaction.prompt({
      type: "select",
      message: "Select Google Vertex AI authentication method:",
      options: [
        { id: "api-key", label: "Google Cloud API key" },
        { id: "adc", label: "Application Default Credentials" },
        { id: "service-account", label: "Service account credentials file" },
      ],
    });
    if (method === "api-key") return { type: "api_key", key: await interaction.prompt({ type: "secret", message: "Enter Google Cloud API key:" }) };
    if (method !== "adc" && method !== "service-account") throw new Error(`Unknown Google Vertex AI auth method: ${method}`);
    const project = await interaction.prompt({ type: "text", message: "Enter Google Cloud project ID:" });
    const location = await interaction.prompt({ type: "text", message: "Enter Google Cloud location:" });
    const credentialsPath = method === "service-account" ? await interaction.prompt({ type: "text", message: "Enter service account credentials file path:" }) : undefined;
    return { type: "api_key", env: compactEnv({ GOOGLE_CLOUD_PROJECT: project, GOOGLE_CLOUD_LOCATION: location, GOOGLE_APPLICATION_CREDENTIALS: credentialsPath }) };
  },
  resolve: async ({ ctx, credential }) => {
    if (credential?.key?.trim()) return { auth: { apiKey: credential.key.trim() }, env: credential.env, source: "Stored API key" };
    const apiKey = (await ctx.env("GOOGLE_CLOUD_API_KEY"))?.trim();
    if (apiKey) return { auth: { apiKey }, source: "GOOGLE_CLOUD_API_KEY" };
    const env = compactEnv({
      GOOGLE_CLOUD_PROJECT: credential?.env?.["GOOGLE_CLOUD_PROJECT"] ?? await ctx.env("GOOGLE_CLOUD_PROJECT") ?? await ctx.env("GCLOUD_PROJECT"),
      GOOGLE_CLOUD_LOCATION: credential?.env?.["GOOGLE_CLOUD_LOCATION"] ?? await ctx.env("GOOGLE_CLOUD_LOCATION"),
      GOOGLE_APPLICATION_CREDENTIALS: credential?.env?.["GOOGLE_APPLICATION_CREDENTIALS"] ?? await ctx.env("GOOGLE_APPLICATION_CREDENTIALS"),
    });
    const credentialPath = env["GOOGLE_APPLICATION_CREDENTIALS"] ?? "~/.config/gcloud/application_default_credentials.json";
    if (env["GOOGLE_CLOUD_PROJECT"] && env["GOOGLE_CLOUD_LOCATION"] && await ctx.fileExists(credentialPath)) {
      return { auth: {}, env, source: env["GOOGLE_APPLICATION_CREDENTIALS"] ? "Google service account credentials" : "Google Application Default Credentials" };
    }
    return undefined;
  },
};

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

const CLOUDFLARE_API_KEY = "CLOUDFLARE_API_KEY";
const CLOUDFLARE_ACCOUNT_ID = "CLOUDFLARE_ACCOUNT_ID";
const CLOUDFLARE_GATEWAY_ID = "CLOUDFLARE_GATEWAY_ID";
const CLOUDFLARE_WORKERS_AI_BASE_URL =
  "https://api.cloudflare.com/client/v4/accounts/{CLOUDFLARE_ACCOUNT_ID}/ai/v1";
const CLOUDFLARE_AI_GATEWAY_COMPAT_BASE_URL =
  "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/compat";

/**
 * Cloudflare's API token is insufficient on its own: each endpoint is scoped
 * to an account (and AI Gateway additionally to a gateway).  Keep those
 * values in the existing credential `env` bag, just like Pi, rather than
 * adding provider-specific fields to models.json.
 */
export function cloudflareWorkersAIAuth(): ApiKeyAuth {
  return cloudflareAuth("workers-ai");
}

export function cloudflareAIGatewayAuth(): ApiKeyAuth {
  return cloudflareAuth("ai-gateway");
}

function cloudflareAuth(kind: "workers-ai" | "ai-gateway"): ApiKeyAuth {
  const gateway = kind === "ai-gateway";
  return {
    name: "Cloudflare API key",
    login: async (interaction) => {
      const key = await interaction.prompt({ type: "secret", message: "Enter Cloudflare API key:" });
      const accountId = await interaction.prompt({ type: "text", message: "Enter Cloudflare account ID:" });
      const gatewayId = gateway
        ? await interaction.prompt({ type: "text", message: "Enter Cloudflare AI Gateway ID:" })
        : undefined;
      return {
        type: "api_key",
        key,
        env: compactEnv({
          [CLOUDFLARE_ACCOUNT_ID]: accountId,
          [CLOUDFLARE_GATEWAY_ID]: gatewayId,
        }),
      };
    },
    resolve: async ({ ctx, credential }) => {
      const key = credential?.key?.trim() || (await ctx.env(CLOUDFLARE_API_KEY))?.trim();
      const accountId = credential?.env?.[CLOUDFLARE_ACCOUNT_ID] || await ctx.env(CLOUDFLARE_ACCOUNT_ID);
      const gatewayId = gateway
        ? credential?.env?.[CLOUDFLARE_GATEWAY_ID] || await ctx.env(CLOUDFLARE_GATEWAY_ID)
        : undefined;
      if (!key || !accountId || (gateway && !gatewayId)) return undefined;
      const env = compactEnv({
        [CLOUDFLARE_ACCOUNT_ID]: accountId,
        [CLOUDFLARE_GATEWAY_ID]: gatewayId,
      });
      return gateway
        ? {
            auth: {
              baseUrl: materializeCloudflareUrl(CLOUDFLARE_AI_GATEWAY_COMPAT_BASE_URL, env),
              headers: {
                "cf-aig-authorization": `Bearer ${key}`,
                Authorization: null,
                "x-api-key": null,
              },
            },
            env,
            source: credential === undefined ? CLOUDFLARE_API_KEY : "Stored Cloudflare API key",
          }
        : {
            auth: {
              apiKey: key,
              baseUrl: materializeCloudflareUrl(CLOUDFLARE_WORKERS_AI_BASE_URL, env),
            },
            env,
            source: credential === undefined ? CLOUDFLARE_API_KEY : "Stored Cloudflare API key",
          };
    },
  };
}

function materializeCloudflareUrl(template: string, env: Record<string, string>): string {
  return template
    .replaceAll(`{${CLOUDFLARE_ACCOUNT_ID}}`, env[CLOUDFLARE_ACCOUNT_ID]!)
    .replaceAll(`{${CLOUDFLARE_GATEWAY_ID}}`, env[CLOUDFLARE_GATEWAY_ID] ?? `{${CLOUDFLARE_GATEWAY_ID}}`);
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

export const anthropicOAuth: OAuthAuth = {
  name: "Anthropic (Claude Pro/Max)",
  loginLabel: "Sign in with Claude",
  login: async (interaction) => {
    const verifier = await pkceVerifier();
    const authorization = new URL(ANTHROPIC_AUTHORIZE_URL);
    authorization.search = new URLSearchParams({
      code: "true",
      client_id: ANTHROPIC_CLIENT_ID,
      response_type: "code",
      redirect_uri: ANTHROPIC_REDIRECT_URI,
      scope: ANTHROPIC_SCOPES,
      code_challenge: await pkceChallenge(verifier),
      code_challenge_method: "S256",
      state: verifier,
    }).toString();
    interaction.notify({ type: "auth_url", url: authorization.toString(), instructions: "Complete login in a browser, then paste the final redirect URL or authorization code." });
    const { code, state } = parseAuthorizationInput(await interaction.prompt({ type: "manual_code", message: "Paste the Claude authorization code or redirect URL:", placeholder: ANTHROPIC_REDIRECT_URI }));
    if (!code) throw new Error("Anthropic OAuth returned no authorization code");
    if (state !== undefined && state !== verifier) throw new Error("Anthropic OAuth state mismatch");
    return anthropicToken({ grant_type: "authorization_code", client_id: ANTHROPIC_CLIENT_ID, code, state: verifier, redirect_uri: ANTHROPIC_REDIRECT_URI, code_verifier: verifier }, interaction.signal);
  },
  refresh: (credential, signal) => anthropicToken({ grant_type: "refresh_token", client_id: ANTHROPIC_CLIENT_ID, refresh_token: credential.refresh }, signal),
  toAuth: async (credential) => ({ headers: anthropicOAuthHeaders(credential.access) }),
};

export const openRouterOAuth: OAuthAuth = {
  name: "OpenRouter OAuth",
  loginLabel: "Sign in with OpenRouter",
  login: async (interaction) => {
    const verifier = await pkceVerifier();
    const callback = await startLoopbackCodeCallback(`/oauth/openrouter/${crypto.randomUUID()}`, interaction.signal);
    const authorization = new URL(OPENROUTER_AUTHORIZE_URL);
    authorization.searchParams.set("callback_url", callback.url);
    authorization.searchParams.set("code_challenge", await pkceChallenge(verifier));
    authorization.searchParams.set("code_challenge_method", "S256");
    interaction.notify({ type: "progress", message: `Listening for OpenRouter OAuth callback on ${callback.url}` });
    interaction.notify({ type: "auth_url", url: authorization.toString(), instructions: "Complete sign-in in your browser. If the browser is remote, paste the final redirect URL or authorization code." });
    const manualAbort = new AbortController();
    const manual = interaction.prompt({
      type: "manual_code",
      message: "Complete sign-in in your browser, or paste the authorization code / redirect URL:",
      placeholder: callback.url,
      signal: manualAbort.signal,
    }).then(
      (input) => ({ source: "manual" as const, code: parseAuthorizationInput(input).code }),
      (error) => ({ source: "manual-error" as const, error }),
    );
    try {
      const result = await Promise.race([
        callback.wait().then((code) => ({ source: "callback" as const, code })),
        manual,
      ]);
      if (result.source === "manual-error") throw result.error;
      if (!result.code) throw new Error("OpenRouter OAuth returned no authorization code");
      const response = await fetch(OPENROUTER_TOKEN_URL, {
        method: "POST",
        headers: { accept: "application/json", "content-type": "application/json" },
        body: JSON.stringify({ code: result.code, code_verifier: verifier, code_challenge_method: "S256" }),
        signal: interaction.signal,
      });
      const body = await jsonObject(response);
      if (!response.ok) throw oauthFailure("OpenRouter OAuth key exchange", { ok: false, status: response.status, body });
      const key = requiredString(body, "key");
      return { type: "oauth", access: key, refresh: "", expires: Number.MAX_SAFE_INTEGER };
    } finally {
      manualAbort.abort();
      callback.close();
    }
  },
  refresh: async (credential) => credential,
  toAuth: async (credential) => ({ apiKey: credential.access }),
};

export const githubCopilotOAuth: OAuthAuth = {
  name: "GitHub Copilot",
  loginLabel: "Sign in with GitHub Copilot",
  login: async (interaction) => {
    const device = await postForm("https://github.com/login/device/code", { client_id: COPILOT_CLIENT_ID, scope: "read:user" }, interaction.signal, { "User-Agent": COPILOT_HEADERS["User-Agent"] });
    if (!device.ok) throw oauthFailure("GitHub device authorization", device);
    const deviceCode = requiredString(device.body, "device_code");
    const userCode = requiredString(device.body, "user_code");
    const verificationUri = trustedHttps(requiredString(device.body, "verification_uri"));
    const expiresIn = positiveNumber(device.body["expires_in"]) ?? 900;
    let interval = (positiveNumber(device.body["interval"]) ?? 5) * 1_000;
    interaction.notify({ type: "device_code", userCode, verificationUri, intervalSeconds: interval / 1_000, expiresInSeconds: expiresIn });
    const deadline = Date.now() + expiresIn * 1_000;
    while (Date.now() < deadline) {
      await abortableSleep(interval, interaction.signal);
      const token = await postForm("https://github.com/login/oauth/access_token", { client_id: COPILOT_CLIENT_ID, device_code: deviceCode, grant_type: "urn:ietf:params:oauth:grant-type:device_code" }, interaction.signal, { "User-Agent": COPILOT_HEADERS["User-Agent"] });
      if (token.ok && typeof token.body["access_token"] === "string") return githubCopilotToken(token.body["access_token"]);
      if (token.body["error"] === "authorization_pending") continue;
      if (token.body["error"] === "slow_down") { interval += 5_000; continue; }
      throw oauthFailure("GitHub device token", token);
    }
    throw new Error("GitHub Copilot device login timed out");
  },
  refresh: async (credential) => githubCopilotToken(credential.refresh, copilotEnterpriseDomain(credential)),
  toAuth: async (credential) => ({ apiKey: credential.access, baseUrl: copilotApiBaseUrl(credential.access, copilotEnterpriseDomain(credential)) }),
};

/** Radius uses the gateway's documented device-code endpoint; gateway URLs are persisted on the OAuth credential. */
export function createRadiusOAuth(gateway = "https://radius.pi.dev", name = "Radius"): OAuthAuth {
  const baseUrl = normalizeGateway(gateway);
  return {
    name,
    loginLabel: `Sign in with ${name}`,
    login: async (interaction) => {
      const method = await interaction.prompt({
        type: "select",
        message: `Sign in to ${name}:`,
        options: [
          { id: "browser", label: "Sign in with browser", description: "Recommended on this machine." },
          { id: "device-code", label: "Sign in with device code", description: "Use when the browser is on another device." },
        ],
      });
      if (method === "browser") return radiusBrowserLogin(baseUrl, name, interaction);
      if (method !== "device-code") throw new Error(`Unknown ${name} sign-in method: ${method}`);
      const response = await postForm(`${baseUrl}/v1/oauth/device`, { client_id: RADIUS_CLIENT_ID, scope: RADIUS_SCOPE }, interaction.signal);
      if (!response.ok) throw oauthFailure(`${name} device authorization`, response);
      const deviceCode = requiredString(response.body, "device_code");
      const userCode = requiredString(response.body, "user_code");
      const verificationUri = trustedHttps(requiredString(response.body, "verification_uri"));
      const expiresIn = positiveNumber(response.body["expires_in"]) ?? 900;
      let interval = (positiveNumber(response.body["interval"]) ?? 5) * 1_000;
      interaction.notify({ type: "device_code", userCode, verificationUri, intervalSeconds: interval / 1_000, expiresInSeconds: expiresIn });
      const deadline = Date.now() + expiresIn * 1_000;
      while (Date.now() < deadline) {
        await abortableSleep(interval, interaction.signal);
        const token = await postForm(`${baseUrl}/v1/oauth/token`, { grant_type: "urn:ietf:params:oauth:grant-type:device_code", client_id: RADIUS_CLIENT_ID, device_code: deviceCode }, interaction.signal);
        if (token.ok) return radiusToken(token.body, baseUrl);
        if (token.body["error"] === "authorization_pending") continue;
        if (token.body["error"] === "slow_down") { interval += 5_000; continue; }
        throw oauthFailure(`${name} device token`, token);
      }
      throw new Error(`${name} device login timed out`);
    },
    refresh: async (credential, signal) => {
      const response = await postForm(`${baseUrl}/v1/oauth/token`, { grant_type: "refresh_token", client_id: RADIUS_CLIENT_ID, refresh_token: credential.refresh }, signal);
      if (!response.ok) throw oauthFailure(`${name} token refresh`, response);
      return radiusToken(response.body, baseUrl);
    },
    toAuth: async (credential) => ({ apiKey: credential.access }),
  };
}

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
  extraHeaders?: Record<string, string>,
): Promise<FormResponse> {
  const response = await fetch(url, {
    method: "POST",
    headers: { accept: "application/json", "content-type": "application/x-www-form-urlencoded", ...extraHeaders },
    body: new URLSearchParams(fields),
    signal,
  });
  return { ok: response.ok, status: response.status, body: await jsonObject(response) };
}

async function anthropicToken(fields: Record<string, string>, signal?: AbortSignal): Promise<OAuthCredential> {
  const response = await postJson(ANTHROPIC_TOKEN_URL, fields, signal);
  if (!response.ok) throw oauthFailure("Anthropic OAuth token request", response);
  return oauthCredential(response.body, fields["refresh_token"], true);
}

async function postJson(url: string, body: Record<string, string>, signal?: AbortSignal): Promise<FormResponse> {
  const response = await fetch(url, {
    method: "POST",
    headers: { accept: "application/json", "content-type": "application/json" },
    body: JSON.stringify(body),
    signal,
  });
  return { ok: response.ok, status: response.status, body: await jsonObject(response) };
}

async function radiusBrowserLogin(
  gateway: string,
  name: string,
  interaction: AuthInteraction,
): Promise<OAuthCredential> {
  const discovery = await fetch(`${gateway}/v1/oauth`, {
    headers: { accept: "application/json" },
    signal: interaction.signal,
  });
  const discoveryBody = await jsonObject(discovery);
  if (!discovery.ok) throw oauthFailure(`${name} OAuth discovery`, { ok: false, status: discovery.status, body: discoveryBody });
  const endpoint = requiredString(discoveryBody, "authorizationEndpoint");
  const verifier = await pkceVerifier();
  const state = crypto.randomUUID();
  const callback = await startLoopbackCodeCallback(`/oauth/radius/${crypto.randomUUID()}`, interaction.signal, state);
  const authorization = new URL(endpoint);
  authorization.search = new URLSearchParams({
    response_type: "code",
    client_id: RADIUS_CLIENT_ID,
    redirect_uri: callback.url,
    scope: RADIUS_SCOPE,
    code_challenge: await pkceChallenge(verifier),
    code_challenge_method: "S256",
    handoff: "url",
    state,
  }).toString();
  interaction.notify({ type: "progress", message: `Listening for ${name} OAuth callback on ${callback.url}` });
  interaction.notify({ type: "auth_url", url: authorization.toString(), instructions: "Continue sign-in in your browser." });
  try {
    const code = await callback.wait();
    const token = await postForm(`${gateway}/v1/oauth/token`, {
      grant_type: "authorization_code",
      client_id: RADIUS_CLIENT_ID,
      redirect_uri: callback.url,
      code,
      code_verifier: verifier,
    }, interaction.signal);
    if (!token.ok) throw oauthFailure(`${name} OAuth token`, token);
    return radiusToken(token.body, gateway);
  } finally {
    callback.close();
  }
}

interface LoopbackCodeCallback {
  readonly url: string;
  wait(): Promise<string>;
  close(): void;
}

async function startLoopbackCodeCallback(
  path: string,
  signal?: AbortSignal,
  expectedState?: string,
): Promise<LoopbackCodeCallback> {
  if (signal?.aborted) throw new DOMException("Login cancelled", "AbortError");
  let server: Server | undefined;
  let settle: ((result: { code?: string; error?: Error }) => void) | undefined;
  let settled = false;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const result = new Promise<{ code?: string; error?: Error }>((resolve) => {
    settle = resolve;
  });
  const finish = (next: { code?: string; error?: Error }) => {
    if (settled) return;
    settled = true;
    if (timeout !== undefined) clearTimeout(timeout);
    signal?.removeEventListener("abort", onAbort);
    server?.close();
    settle?.(next);
  };
  const onAbort = () => finish({ error: new DOMException("Login cancelled", "AbortError") });
  signal?.addEventListener("abort", onAbort, { once: true });
  server = createServer((request, response) => {
    const requestUrl = new URL(request.url ?? "/", "http://127.0.0.1");
    if (request.method !== "GET" || requestUrl.pathname !== path) {
      sendCallbackHtml(response, 404, "OAuth callback route not found.");
      return;
    }
    if (expectedState !== undefined && requestUrl.searchParams.get("state") !== expectedState) {
      sendCallbackHtml(response, 400, "OAuth state mismatch.");
      return;
    }
    const error = requestUrl.searchParams.get("error");
    if (error !== null) {
      sendCallbackHtml(response, 400, requestUrl.searchParams.get("error_description") ?? error);
      finish({ error: new Error(`OAuth authorization failed: ${error}`) });
      return;
    }
    const code = requestUrl.searchParams.get("code");
    if (!code) {
      sendCallbackHtml(response, 400, "OAuth returned no authorization code.");
      return;
    }
    sendCallbackHtml(response, 200, "Signed in. You may close this page.");
    finish({ code });
  });
  await new Promise<void>((resolve, reject) => {
    server!.once("error", reject);
    server!.listen(0, "127.0.0.1", () => {
      server!.off("error", reject);
      resolve();
    });
  });
  const address = server.address();
  if (address === null || typeof address === "string") {
    server.close();
    throw new Error("Could not determine OAuth callback port");
  }
  timeout = setTimeout(
    () => finish({ error: new Error("OAuth login timed out waiting for the browser callback") }),
    LOOPBACK_LOGIN_TIMEOUT_MS,
  );
  return {
    url: `http://127.0.0.1:${String(address.port)}${path}`,
    wait: async () => {
      const next = await result;
      if (next.error !== undefined) throw next.error;
      if (next.code === undefined) throw new Error("OAuth callback did not return an authorization code");
      return next.code;
    },
    close: () => finish({ error: new Error("OAuth callback cancelled") }),
  };
}

function sendCallbackHtml(response: ServerResponse, status: number, message: string): void {
  response.statusCode = status;
  response.setHeader("content-type", "text/html; charset=utf-8");
  response.setHeader("cache-control", "no-store");
  response.end(`<!doctype html><title>OAuth</title><p>${escapeHtml(message)}</p>`);
}

function escapeHtml(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function parseAuthorizationInput(input: string): { code?: string; state?: string } {
  const value = input.trim();
  if (value === "") return {};
  try {
    const url = new URL(value);
    return { code: url.searchParams.get("code") ?? undefined, state: url.searchParams.get("state") ?? undefined };
  } catch {
    const params = value.includes("code=") ? new URLSearchParams(value) : undefined;
    return params === undefined ? { code: value } : { code: params.get("code") ?? undefined, state: params.get("state") ?? undefined };
  }
}

async function pkceVerifier(): Promise<string> {
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  return base64Url(bytes);
}

async function pkceChallenge(verifier: string): Promise<string> {
  return base64Url(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
}

function base64Url(value: Uint8Array): string {
  return Buffer.from(value).toString("base64url");
}

function compactEnv(values: Record<string, string | undefined>): Record<string, string> {
  return Object.fromEntries(Object.entries(values).filter((entry): entry is [string, string] => entry[1]?.trim() !== ""));
}

function anthropicOAuthHeaders(access: string): Record<string, string> {
  return {
    Authorization: `Bearer ${access}`,
    "anthropic-beta": "claude-code-20250219,oauth-2025-04-20",
    "user-agent": "kimi-code",
    "x-app": "cli",
  };
}

function normalizeGateway(value: string): string {
  const url = new URL(/^https?:\/\//u.test(value) ? value : `https://${value}`);
  if (url.protocol !== "https:" && url.protocol !== "http:") throw new Error("Radius gateway must use HTTP(S)");
  return url.toString().replace(/\/+$/u, "");
}

function copilotEnterpriseDomain(credential: OAuthCredential): string | undefined {
  const value = credential["enterpriseUrl"];
  if (typeof value !== "string" || value.trim() === "") return undefined;
  try { return new URL(value.includes("://") ? value : `https://${value}`).hostname; } catch { return undefined; }
}

function copilotApiBaseUrl(token: string, enterpriseDomain?: string): string {
  const proxy = /(?:^|;)proxy-ep=([^;]+)/u.exec(token)?.[1];
  if (proxy) return `https://${proxy.replace(/^proxy\./u, "api.")}`;
  return enterpriseDomain === undefined ? "https://api.individual.githubcopilot.com" : `https://copilot-api.${enterpriseDomain}`;
}

async function githubCopilotToken(refreshToken: string, enterpriseDomain?: string): Promise<OAuthCredential> {
  const response = await fetch(`${copilotApiBaseUrl("", enterpriseDomain)}/copilot_internal/v2/token`, {
    headers: { accept: "application/json", Authorization: `Bearer ${refreshToken}`, ...COPILOT_HEADERS },
  });
  const body = await jsonObject(response);
  if (!response.ok) throw oauthFailure("GitHub Copilot token", { ok: false, status: response.status, body });
  const access = requiredString(body, "token");
  const expiresAt = positiveNumber(body["expires_at"]);
  if (expiresAt === undefined) throw new Error("GitHub Copilot token response missing expires_at");
  const modelResponse = await fetch(`${copilotApiBaseUrl(access, enterpriseDomain)}/models`, {
    headers: { accept: "application/json", Authorization: `Bearer ${access}`, ...COPILOT_HEADERS, "X-GitHub-Api-Version": "2026-06-01" },
  });
  const modelBody = await jsonObject(modelResponse);
  if (!modelResponse.ok) throw oauthFailure("GitHub Copilot model catalog", { ok: false, status: modelResponse.status, body: modelBody });
  const data = modelBody["data"];
  if (!Array.isArray(data)) throw new Error("GitHub Copilot model catalog is invalid");
  const availableModelIds = data.flatMap((item) => {
    if (typeof item !== "object" || item === null || Array.isArray(item)) return [];
    const record = item as Record<string, unknown>;
    const policy = record["policy"] as Record<string, unknown> | undefined;
    const capabilities = record["capabilities"] as Record<string, unknown> | undefined;
    const supports = capabilities?.["supports"] as Record<string, unknown> | undefined;
    return typeof record["id"] === "string" && record["model_picker_enabled"] === true && policy?.["state"] !== "disabled" && supports?.["tool_calls"] !== false
      ? [record["id"]]
      : [];
  });
  return { type: "oauth", access, refresh: refreshToken, expires: expiresAt * 1_000 - 5 * 60_000, enterpriseUrl: enterpriseDomain, availableModelIds };
}

function radiusToken(body: Record<string, unknown>, gateway: string): OAuthCredential {
  const access = requiredString(body, "access_token");
  const refresh = requiredString(body, "refresh_token");
  return { type: "oauth", access, refresh, expires: Date.now() + (positiveNumber(body["expires_in"]) ?? 3_600) * 1_000 - 60_000, gateway };
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
