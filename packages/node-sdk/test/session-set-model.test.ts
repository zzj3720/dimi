import { afterEach, describe, expect, it } from "vitest";

import { createDimiHarness, type DimiError, type DimiHarness } from "#/index";
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

describe("Session.setModel", () => {
  it("updates the runtime model and sends config.update with the resolved model", async () => {
    const homeDir = await makeTempDir(tempDirs, "dimi-sdk-model-home-");
    const workDir = await makeTempDir(tempDirs, "dimi-sdk-model-work-");
    const harness = createLocalHarness(homeDir);

    try {
      await configureLocalProvider(harness);
      const session = await harness.createSession({
        id: "ses_model_wire",
        workDir,
        model: "local/initial-model",
      });

      await session.setModel("local/next-model");

      await expect(session.getStatus()).resolves.toMatchObject({ model: "local/next-model" });
      const configEvent = await waitForAgentWireEvent(
        homeDir,
        session.id,
        "config.update",
        (event) => event["modelAlias"] === "local/next-model",
      );
      expect(configEvent).toMatchObject({
        type: "config.update",
        modelAlias: "local/next-model",
      });
      expect(configEvent).not.toHaveProperty("provider");
    } finally {
      await harness.close();
    }
  });

  it("updates canonical references within an OAuth-capable provider", async () => {
    const homeDir = await makeTempDir(tempDirs, "dimi-sdk-model-home-");
    const workDir = await makeTempDir(tempDirs, "dimi-sdk-model-work-");
    const harness = createDimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createTestProviderRuntime({
        providerId: "kimi-coding",
        modelIds: ["initial", "kimi-for-coding"],
      }),
    });

    try {
      await harness.setConfig({
        defaultProvider: "kimi-coding",
        defaultModel: "initial",
      });
      const session = await harness.createSession({
        id: "ses_model_oauth_wire",
        workDir,
        model: "kimi-coding/initial",
      });

      await session.setModel("kimi-coding/kimi-for-coding");

      await expect(session.getStatus()).resolves.toMatchObject({
        model: "kimi-coding/kimi-for-coding",
      });
      const configEvent = await waitForAgentWireEvent(
        homeDir,
        session.id,
        "config.update",
        (event) => event["modelAlias"] === "kimi-coding/kimi-for-coding",
      );
      expect(configEvent).toMatchObject({
        type: "config.update",
        modelAlias: "kimi-coding/kimi-for-coding",
      });
      expect(configEvent).not.toHaveProperty("provider");
    } finally {
      await harness.close();
    }
  });

  it("rejects empty model names", async () => {
    const homeDir = await makeTempDir(tempDirs, "dimi-sdk-model-home-");
    const workDir = await makeTempDir(tempDirs, "dimi-sdk-model-work-");
    const harness = createLocalHarness(homeDir);

    try {
      await configureLocalProvider(harness);
      const session = await harness.createSession({ id: "ses_model_empty", workDir });

      await expect(session.setModel("   ")).rejects.toMatchObject({
        code: "request.invalid",
      } satisfies Partial<DimiError>);
    } finally {
      await harness.close();
    }
  });

  it("rejects after the session is closed", async () => {
    const homeDir = await makeTempDir(tempDirs, "dimi-sdk-model-home-");
    const workDir = await makeTempDir(tempDirs, "dimi-sdk-model-work-");
    const harness = createLocalHarness(homeDir);

    try {
      await configureLocalProvider(harness);
      const session = await harness.createSession({ id: "ses_model_closed", workDir });
      await session.close();

      await expect(session.setModel("next-model")).rejects.toMatchObject({
        code: "session.closed",
      } satisfies Partial<DimiError>);
    } finally {
      await harness.close();
    }
  });
});

async function configureLocalProvider(harness: DimiHarness): Promise<void> {
  await harness.setConfig({
    defaultProvider: "local",
    defaultModel: "initial-model",
  });
}

function createLocalHarness(homeDir: string): DimiHarness {
  return createDimiHarness({
    homeDir,
    identity: TEST_IDENTITY,
    providerRuntime: createTestProviderRuntime({
      providerId: "local",
      modelIds: ["initial-model", "next-model"],
    }),
  });
}
