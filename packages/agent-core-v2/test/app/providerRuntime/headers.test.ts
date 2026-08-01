/**
 * Provider runtime request-header contracts — host identity isolation, model
 * and auth-header precedence, and Codex request identity. Replaces global
 * fetch at the actual request boundary; run with `vp test -- headers.test.ts`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { HostRequestHeaders } from "#/app/providerRuntime/hostRequestHeaders";
import { ProviderModels } from "#/app/providerRuntime/models";
import { builtinProviders } from "#/app/providerRuntime/providers";
import { streamProvider } from "#/app/providerRuntime/stream";
import type {
  AuthResult,
  Context,
  Credential,
  CredentialInfo,
  CredentialStore,
  Model,
  ModelsStore,
  ModelsStoreEntry,
} from "#/app/providerRuntime/types";

const hostHeaders = {
  "User-Agent": "dimi-cli/1.2.3",
  "X-Msh-Platform": "kimi_code_cli",
  "X-Msh-Device-Id": "device-test",
};

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
  read(_providerId: string): Promise<ModelsStoreEntry | undefined> {
    return Promise.resolve(undefined);
  }

  write(_providerId: string, _entry: ModelsStoreEntry): Promise<void> {
    return Promise.resolve();
  }

  delete(_providerId: string): Promise<void> {
    return Promise.resolve();
  }
}

function headers(init: RequestInit | undefined): Headers {
  return new Headers(init?.headers);
}

function json(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json" },
  });
}

function sse(): Response {
  return new Response("", { headers: { "content-type": "text/event-stream" } });
}

async function consume(stream: AsyncIterable<unknown>): Promise<void> {
  for await (const _event of stream) {
  }
}

function model(provider: string, headers?: Model["headers"]): Model {
  return {
    id: "header-model",
    name: "Header model",
    api: "openai-completions",
    provider,
    baseUrl: "https://provider.example.test/v1",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 32_000,
    headers,
  };
}

function auth(overrides: Partial<AuthResult["auth"]> = {}): AuthResult {
  return {
    auth: {
      apiKey: "provider-key",
      ...overrides,
    },
  };
}

function modelFor(providerId: string): Model {
  const entry = builtinProviders(hostHeaders).find((provider) => provider.id === providerId);
  const selected = entry?.getModels()[0];
  if (selected === undefined) throw new Error(`Missing builtin model for ${providerId}`);
  return selected;
}

function jwtWithAccount(accountId: string): string {
  const payload = Buffer.from(
    JSON.stringify({ "https://api.openai.com/auth": { chatgpt_account_id: accountId } }),
  ).toString("base64url");
  return `header.${payload}.signature`;
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("provider runtime request headers", () => {
  it("keeps the host-provided Dimi CLI user agent as the runtime identity source", () => {
    const host = new HostRequestHeaders(hostHeaders);

    expect(host.headers["User-Agent"]).toBe("dimi-cli/1.2.3");
    expect(host.headers["User-Agent"]).not.toContain("pi/");
  });

  it("isolates catalog host headers to first-party Dimi and Moonshot endpoints", async () => {
    const credentials = new MemoryCredentials();
    await credentials.modify("kimi-coding", async () => ({ type: "api_key", key: "dimi-key" }));
    await credentials.modify("moonshotai", async () => ({ type: "api_key", key: "moonshot-key" }));
    await credentials.modify("openai", async () => ({ type: "api_key", key: "openai-key" }));
    const captured = new Map<string, Headers>();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL, init?: RequestInit) => {
        captured.set(String(input), headers(init));
        return json({ data: [] });
      }),
    );
    const models = new ProviderModels(
      builtinProviders(hostHeaders),
      credentials,
      new MemoryCatalogs(),
    );

    await models.refresh({ provider: "kimi-coding", force: true });
    await models.refresh({ provider: "moonshotai", force: true });
    await models.refresh({ provider: "openai", force: true });

    const dimi = captured.get("https://api.kimi.com/coding/v1/models")!;
    const moonshot = captured.get("https://api.moonshot.ai/v1/models")!;
    const openai = captured.get("https://api.openai.com/v1/models")!;
    for (const firstParty of [dimi, moonshot]) {
      expect(firstParty.get("user-agent")).toBe("dimi-cli/1.2.3");
      expect(firstParty.get("x-msh-platform")).toBe("kimi_code_cli");
      expect(firstParty.get("x-msh-device-id")).toBe("device-test");
    }
    expect(openai.get("user-agent")).toBe("dimi-cli/1.2.3");
    expect(openai.get("x-msh-platform")).toBeNull();
    expect(openai.get("x-msh-device-id")).toBeNull();
  });

  it("applies the same host-header isolation to Dimi, Moonshot and third-party streams", async () => {
    const captured = new Map<string, Headers>();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL, init?: RequestInit) => {
        captured.set(String(input), headers(init));
        return sse();
      }),
    );
    const context: Context = { messages: [] };

    await consume(
      streamProvider(modelFor("kimi-coding"), context, {
        auth: { headers: { Authorization: "Bearer dimi-token" } },
      }),
    );
    await consume(streamProvider(modelFor("moonshotai"), context, auth()));
    await consume(streamProvider(modelFor("openai"), context, auth()));

    const dimi = captured.get("https://api.kimi.com/coding/v1/messages")!;
    const moonshot = captured.get("https://api.moonshot.ai/v1/chat/completions")!;
    const openai = captured.get("https://api.openai.com/v1/responses")!;
    for (const firstParty of [dimi, moonshot]) {
      expect(firstParty.get("user-agent")).toBe("dimi-cli/1.2.3");
      expect(firstParty.get("x-msh-platform")).toBe("kimi_code_cli");
      expect(firstParty.get("x-msh-device-id")).toBe("device-test");
    }
    expect(openai.get("user-agent")).toBe("dimi-cli/1.2.3");
    expect(openai.get("x-msh-platform")).toBeNull();
    expect(openai.get("x-msh-device-id")).toBeNull();
  });

  it("lets auth headers override model headers and honor explicit header removal", async () => {
    let captured: Headers | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL, init?: RequestInit) => {
        captured = headers(init);
        return sse();
      }),
    );

    await consume(
      streamProvider(
        model("custom-provider", {
          Authorization: "Bearer model-token",
          "x-priority": "model",
          "x-remove": "model-value",
        }),
        { messages: [] },
        auth({
          headers: {
            authorization: "Bearer auth-token",
            "x-priority": "auth",
            "x-remove": null,
            "x-auth": "auth-only",
          },
        }),
      ),
    );

    expect(captured?.get("authorization")).toBe("Bearer auth-token");
    expect(captured?.get("x-priority")).toBe("auth");
    expect(captured?.get("x-remove")).toBeNull();
    expect(captured?.get("x-auth")).toBe("auth-only");
  });

  it("uses Codex account, originator and session headers on the actual global fetch boundary", async () => {
    let captured: Headers | undefined;
    const fetch = vi.fn(async (_input: string | URL, init?: RequestInit) => {
      captured = headers(init);
      return sse();
    });
    vi.stubGlobal("fetch", fetch);
    const token = jwtWithAccount("account-test");

    await consume(
      streamProvider(modelFor("openai-codex"), { messages: [] }, auth({ apiKey: token }), {
        sessionId: "session-test",
      }),
    );

    expect(fetch).toHaveBeenCalledOnce();
    expect(captured?.get("authorization")).toBe(`Bearer ${token}`);
    expect(captured?.get("chatgpt-account-id")).toBe("account-test");
    expect(captured?.get("originator")).toBe("dimi");
    expect(captured?.get("openai-beta")).toBe("responses=experimental");
    expect(captured?.get("session-id")).toBe("session-test");
    expect(captured?.get("x-client-request-id")).toBe("session-test");
  });

  it("projects Copilot's editor, intent, continuation and vision headers for every compatible API", async () => {
    const captured: Headers[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL, init?: RequestInit) => {
        captured.push(headers(init));
        return sse();
      }),
    );
    const context: Context = {
      messages: [
        {
          role: "user",
          content: [{ type: "image", mimeType: "image/png", url: "https://image.example.test/input.png" }],
          timestamp: 1,
        },
        {
          role: "toolResult",
          toolCallId: "call-1",
          toolName: "inspect",
          content: [{ type: "image", mimeType: "image/png", data: "AA==" }],
          isError: false,
          timestamp: 2,
        },
      ],
    };

    for (const api of ["openai-completions", "openai-responses", "anthropic-messages"] as const) {
      await consume(streamProvider({ ...model("github-copilot"), api }, context, auth()));
    }

    expect(captured).toHaveLength(3);
    for (const request of captured) {
      expect(request.get("editor-version")).toBe("vscode/1.107.0");
      expect(request.get("editor-plugin-version")).toBe("copilot-chat/0.35.0");
      expect(request.get("copilot-integration-id")).toBe("vscode-chat");
      expect(request.get("openai-intent")).toBe("conversation-edits");
      expect(request.get("x-initiator")).toBe("agent");
      expect(request.get("copilot-vision-request")).toBe("true");
    }
  });
});
