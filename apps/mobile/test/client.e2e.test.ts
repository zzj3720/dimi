import { randomBytes, randomUUID } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { ISessionLifecycleService, IProviderRuntime } from "@dimi-agent/agent-core-v2";
import { startServer } from "@dimi-agent/kap-server";
import type { StoredRemote } from "@dimi-agent/remote";
import { startRemoteBridge } from "@dimi-agent/remote/bridge";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WebSocket } from "ws";

import { startRelay } from "../../../packages/remote/test/server";
import {
  createFakeProviderHarness,
  type FakeProviderHarness,
} from "../../../test/fixtures/fake-provider-harness";
import { MobileRuntime } from "../src/runtime";

const stored = vi.hoisted(() => ({ value: undefined as StoredRemote | undefined }));

vi.mock("react-native", () => ({ Platform: { OS: "android" } }));
vi.mock("expo-crypto", () => ({
  getRandomBytes: (length: number) => new Uint8Array(randomBytes(length)),
  randomUUID,
}));
vi.mock("../src/storage", () => ({
  clearStoredRemote: vi.fn(async () => {
    stored.value = undefined;
  }),
  loadStoredRemote: vi.fn(async () => stored.value),
  saveStoredRemote: vi.fn(async (value: StoredRemote) => {
    stored.value = value;
  }),
}));

const cleanups: (() => Promise<void>)[] = [];

beforeEach(() => {
  stored.value = undefined;
  Object.defineProperty(globalThis, "__DEV__", { configurable: true, value: true });
  Object.defineProperty(globalThis, "WebSocket", { configurable: true, value: WebSocket });
});

afterEach(async () => {
  for (const cleanup of cleanups.splice(0).toReversed()) await cleanup();
  Reflect.deleteProperty(globalThis, "__DEV__");
});

describe("Android client real remote path", () => {
  it("pairs with a real Kap server, completes a prompt, and resyncs after bridge restart", async () => {
    const provider = await createFakeProviderHarness();
    cleanups.push(() => provider.close());
    provider.route("POST", "/v1/chat/completions", async (_request, reply) => {
      if (provider.requests.length === 1) {
        // First round: the model streams its text reply.
        await reply.sseJson(200, [
          completionChunk({ content: "hello from the real mobile path" }),
          completionChunk({}, "stop"),
        ]);
        return;
      }
      // The intentional-completion protocol requires a sole AllDone call to
      // end the turn after a tool-free response.
      await reply.sseJson(200, [
        completionToolChunk("call_done", "AllDone", "{}"),
        completionChunk({}, "tool_calls"),
      ]);
    });

    const home = await mkdtemp(join(tmpdir(), "mobile-client-e2e-"));
    cleanups.push(() => rm(home, { recursive: true, force: true }));
    await writeFile(
      join(home, "config.toml"),
      ['default_provider = "local"', 'default_model = "fake-model"', ""].join("\n"),
    );
    await writeFile(
      join(home, "models.json"),
      JSON.stringify({
        providers: {
          local: {
            id: "local",
            api: "openai-completions",
            baseUrl: `${provider.baseUrl}/v1`,
            apiKey: "test-only",
            models: [
              { id: "fake-model", input: ["text"], contextWindow: 262144, maxTokens: 16000 },
            ],
          },
        },
      }),
    );

    const server = await startServer({
      host: "127.0.0.1",
      port: 0,
      homeDir: home,
      logLevel: "silent",
    });
    cleanups.push(async () => {
      server.connectionRegistry.closeAll("test cleanup");
      server.app.server.closeAllConnections();
      await server.close();
    });
    const serverOrigin = `http://127.0.0.1:${server.port}`;
    // The provider runtime syncs custom models.json providers lazily — force a
    // refresh so the fake provider's model is resolvable before the session
    // is created.
    await server.core.accessor.get(IProviderRuntime).refreshProviderDefinitions();
    const session = await server.core.accessor.get(ISessionLifecycleService).create({
      workDir: home,
      mainAgentBinding: { profile: "agent", model: "fake-model" },
    });
    const sessionId = session.id;

    const relay = await startRelay();
    cleanups.push(() => relay.close());
    const relayUrl = `ws://${relay.host}:${relay.port}`;
    const statePath = join(home, "remote-bridge.json");
    const bridgeOptions = {
      relayUrl,
      localOrigin: serverOrigin,
      localToken: server.authTokenService.getToken(),
      runtimeName: "E2E runtime",
      statePath,
    };
    let bridge = await startRemoteBridge(bridgeOptions);
    cleanups.push(async () => {
      await bridge.close();
    });

    const runtime = new MobileRuntime();
    cleanups.push(async () => {
      runtime.dispose();
    });
    await runtime.pair(bridge.pairingUri);
    await eventually(() => runtime.state.connection === "online");
    await eventually(() => runtime.state.sessions.some((session) => session.id === sessionId));
    await runtime.selectSession(sessionId);
    await runtime.sendPrompt("Say hello from the mobile client.");

    await eventually(() => provider.requests.length === 1);
    await eventually(
      () => runtime.state.sessions.some((session) => session.id === sessionId && !session.busy),
      10_000,
    );
    const transcriptResponse = await fetch(
      `${serverOrigin}/api/v1/sessions/${sessionId}/transcript?agent_id=main&page_size=100`,
      {
        headers: { authorization: `Bearer ${server.authTokenService.getToken()}` },
      },
    );
    expect(JSON.stringify(await transcriptResponse.json())).toContain(
      "hello from the real mobile path",
    );
    await eventually(
      () =>
        JSON.stringify(runtime.state.transcript).includes("hello from the real mobile path"),
      10_000,
    );

    await bridge.close();
    await eventually(() => runtime.state.connection === "disconnected");
    bridge = await startRemoteBridge(bridgeOptions);
    await eventually(() => runtime.state.connection === "online", 10_000);
    await eventually(() =>
      JSON.stringify(runtime.state.transcript).includes("hello from the real mobile path"),
    );
  }, 30_000);
});

function completionChunk(
  delta: Record<string, unknown>,
  finishReason: string | null = null,
): Record<string, unknown> {
  return {
    id: "chatcmpl-mobile-e2e",
    object: "chat.completion.chunk",
    created: 1,
    model: "fake-model",
    choices: [{ index: 0, delta, finish_reason: finishReason }],
  };
}

function completionToolChunk(
  id: string,
  name: string,
  args: string,
): Record<string, unknown> {
  return {
    id: `chatcmpl-mobile-${id}`,
    object: "chat.completion.chunk",
    created: 1,
    model: "fake-model",
    choices: [
      {
        index: 0,
        delta: {
          tool_calls: [
            {
              index: 0,
              id,
              type: "function",
              function: { name, arguments: args },
            },
          ],
        },
        finish_reason: null,
      },
    ],
  };
}

async function eventually(predicate: () => boolean, timeoutMs = 5_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error("Timed out waiting for condition.");
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}
