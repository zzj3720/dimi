import { afterEach, describe, expect, it } from "vitest";

import { createKimiHarness, type Event, type KimiError } from "#/index";

import {
  createFakeProviderHarness,
  type FakeProviderHarness,
} from "../../../test/fixtures/fake-provider-harness";
import {
  createTestProviderRuntime,
  makeTempDir,
  removeTempDirs,
  waitForAgentWireEvent,
} from "./session-runtime-helpers";
import { TEST_IDENTITY } from "./test-identity";

const tempDirs: string[] = [];
let provider: FakeProviderHarness | undefined;

afterEach(async () => {
  await provider?.close();
  provider = undefined;
  await removeTempDirs(tempDirs);
});

describe("Session.steer", () => {
  it("starts a new turn when the session is idle", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-steer-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-steer-work-");
    provider = await createFakeProviderHarness();
    provider.route("POST", "/v1/chat/completions", async (_request, reply) => {
      await reply.sseJson(200, [
        completionChunk({ content: "idle steer response" }),
        completionChunk({}, "stop"),
      ]);
    });
    const harness = createKimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createTestProviderRuntime({
        providerId: "local",
        modelId: "fake-model",
        baseUrl: `${provider.baseUrl}/v1`,
      }),
    });

    try {
      await harness.setConfig({
        defaultProvider: "local",
        defaultModel: "fake-model",
      });
      const session = await harness.createSession({ id: "ses_steer_idle", workDir });
      const ended = waitForEvent(session, (event) => event.type === "turn.ended");

      await expect(session.steer("start from idle")).resolves.toBeUndefined();
      await ended;
      await expect(
        waitForAgentWireEvent(homeDir, session.id, "turn.prompt", (event) =>
          Array.isArray(event["input"]),
        ),
      ).resolves.toMatchObject({
        type: "turn.prompt",
        input: [{ type: "text", text: "start from idle" }],
      });
    } finally {
      await harness.close();
    }
  });

  it("sends turn.steer to the core session runtime", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-steer-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-steer-work-");
    provider = await createFakeProviderHarness();
    let markStarted!: () => void;
    let release!: () => void;
    const started = new Promise<void>((resolve) => {
      markStarted = resolve;
    });
    const blocked = new Promise<void>((resolve) => {
      release = resolve;
    });
    provider.route("POST", "/v1/chat/completions", async (_request, reply) => {
      markStarted();
      await blocked;
      await reply.sseJson(200, [
        completionChunk({ content: "steer response" }),
        completionChunk({}, "stop"),
      ]);
    });
    const harness = createKimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createTestProviderRuntime({
        providerId: "local",
        modelId: "fake-model",
        baseUrl: `${provider.baseUrl}/v1`,
      }),
    });

    try {
      await harness.setConfig({
        defaultProvider: "local",
        defaultModel: "fake-model",
      });
      const session = await harness.createSession({ id: "ses_steer_wire", workDir });
      const ended = waitForEvent(session, (event) => event.type === "turn.ended");
      await session.prompt("start the turn");
      await started;

      await session.steer("also do this");
      release();

      await expect(
        waitForAgentWireEvent(homeDir, session.id, "turn.steer", (event) =>
          Array.isArray(event["input"]),
        ),
      ).resolves.toMatchObject({
        type: "turn.steer",
        input: [{ type: "text", text: "also do this" }],
      });
      await ended;
    } finally {
      await harness.close();
    }
  });

  it("rejects empty steer input", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-steer-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-steer-work-");
    const harness = createKimiHarness({ homeDir, identity: TEST_IDENTITY });

    try {
      const session = await harness.createSession({ id: "ses_steer_empty", workDir });

      await expect(session.steer("   ")).rejects.toMatchObject({
        name: "KimiError",
        code: "request.prompt_input_empty",
      } satisfies Partial<KimiError>);
    } finally {
      await harness.close();
    }
  });

  it("rejects after the session is closed", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-steer-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-steer-work-");
    const harness = createKimiHarness({ homeDir, identity: TEST_IDENTITY });

    try {
      const session = await harness.createSession({ id: "ses_steer_closed", workDir });
      await session.close();

      await expect(session.steer("hello")).rejects.toMatchObject({
        name: "KimiError",
        code: "session.closed",
      } satisfies Partial<KimiError>);
    } finally {
      await harness.close();
    }
  });
});

function completionChunk(
  delta: Record<string, unknown>,
  finishReason: string | null = null,
): Record<string, unknown> {
  return {
    id: "chatcmpl-node-sdk-steer",
    object: "chat.completion.chunk",
    created: 1,
    model: "fake-model",
    choices: [{ index: 0, delta, finish_reason: finishReason }],
  };
}

function waitForEvent(
  session: { onEvent(listener: (event: Event) => void): () => void },
  predicate: (event: Event) => boolean,
): Promise<Event> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      unsubscribe();
      reject(new Error("Timed out waiting for session event"));
    }, 5_000);
    const unsubscribe = session.onEvent((event) => {
      if (!predicate(event)) return;
      clearTimeout(timeout);
      unsubscribe();
      resolve(event);
    });
  });
}
