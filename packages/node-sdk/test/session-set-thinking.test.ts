import { afterEach, describe, expect, it } from "vitest";

import { createDimiHarness, type DimiError } from "#/index";

import { createFakeProviderHarness } from "../../../test/fixtures/fake-provider-harness";
import {
  createTestProviderRuntime,
  makeTempDir,
  removeTempDirs,
  waitForAgentWireEvent,
  waitForSDKEvent,
} from "./session-runtime-helpers";
import { TEST_IDENTITY } from "./test-identity";

const tempDirs: string[] = [];

afterEach(async () => {
  await removeTempDirs(tempDirs);
});

describe("Session.setThinking", () => {
  it("sends config.update with the new thinking effort", async () => {
    const homeDir = await makeTempDir(tempDirs, "dimi-sdk-thinking-home-");
    const workDir = await makeTempDir(tempDirs, "dimi-sdk-thinking-work-");
    const harness = createDimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createTestProviderRuntime({
        model: { reasoning: true, thinkingLevelMap: { off: null, low: "low" } },
      }),
    });

    try {
      await harness.setConfig({
        defaultProvider: "kimi-coding",
        defaultModel: "kimi-for-coding",
      });
      const session = await harness.createSession({ id: "ses_thinking_wire", workDir });

      await session.setThinking("low");

      await expect(
        waitForAgentWireEvent(
          homeDir,
          session.id,
          "config.update",
          (event) => event["thinkingEffort"] === "low",
        ),
      ).resolves.toMatchObject({
        type: "config.update",
        thinkingEffort: "low",
      });
    } finally {
      await harness.close();
    }
  });

  it("sends DeepSeek V4 Flash max effort through the provider request", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-thinking-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-thinking-work-");
    const provider = await createFakeProviderHarness();
    provider.route("POST", "/v1/chat/completions", async (_request, reply) => {
      await reply.sseJson(200, [
        {
          id: "chatcmpl-deepseek-thinking",
          object: "chat.completion.chunk",
          created: 1,
          model: "deepseek-v4-flash",
          choices: [
            { index: 0, delta: { content: "done" }, finish_reason: null },
          ],
        },
        {
          id: "chatcmpl-deepseek-thinking",
          object: "chat.completion.chunk",
          created: 1,
          model: "deepseek-v4-flash",
          choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
        },
      ]);
    });
    const harness = createKimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createTestProviderRuntime({
        providerId: "deepseek",
        modelId: "deepseek-v4-flash",
        baseUrl: `${provider.baseUrl}/v1`,
        model: {
          reasoning: true,
          thinkingLevelMap: { low: "low", high: "high", max: "max" },
          defaultThinkingLevel: "high",
          compat: {
            requiresReasoningContentOnAssistantMessages: true,
            supportsReasoningEffort: true,
            thinkingFormat: "deepseek",
          },
        },
      }),
    });

    try {
      await harness.setConfig({
        defaultProvider: "deepseek",
        defaultModel: "deepseek-v4-flash",
      });
      const session = await harness.createSession({ id: "ses_deepseek_max_thinking", workDir });
      await session.setThinking("max");
      const ended = waitForSDKEvent(session, (event) => event.type === "turn.ended", 10_000);

      await session.prompt("finish the request");
      await ended;

      expect(provider.requests).toHaveLength(1);
      expect(provider.requests[0]?.bodyJson).toMatchObject({
        model: "deepseek-v4-flash",
        thinking: { type: "enabled" },
        reasoning_effort: "max",
      });
    } finally {
      await harness.close();
      await provider.close();
    }
  });

  it("rejects empty thinking efforts", async () => {
    const homeDir = await makeTempDir(tempDirs, "dimi-sdk-thinking-home-");
    const workDir = await makeTempDir(tempDirs, "dimi-sdk-thinking-work-");
    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });

    try {
      const session = await harness.createSession({ id: "ses_thinking_empty", workDir });

      await expect(session.setThinking("   ")).rejects.toMatchObject({
        code: "request.invalid",
      } satisfies Partial<DimiError>);
    } finally {
      await harness.close();
    }
  });

  it("rejects after the session is closed", async () => {
    const homeDir = await makeTempDir(tempDirs, "dimi-sdk-thinking-home-");
    const workDir = await makeTempDir(tempDirs, "dimi-sdk-thinking-work-");
    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });

    try {
      const session = await harness.createSession({ id: "ses_thinking_closed", workDir });
      await session.close();

      await expect(session.setThinking("high")).rejects.toMatchObject({
        code: "session.closed",
      } satisfies Partial<DimiError>);
    } finally {
      await harness.close();
    }
  });
});
