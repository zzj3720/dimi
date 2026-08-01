import { afterEach, describe, expect, it } from "vitest";

import { createDimiHarness, type DimiError } from "#/index";

import {
  createTestProviderRuntime,
  makeTempDir,
  removeTempDirs,
  waitForAgentWireEvent,
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
