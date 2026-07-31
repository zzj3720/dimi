import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { createKimiHarness, type AuthInteraction } from "@moonshot-ai/kimi-code-sdk";

import { smokeIdentityFromEnv, runPromptToEnd } from "./runtime-smoke-helpers";

const PROVIDER = "kimi-coding";

async function main(): Promise<void> {
  const homeDir = await mkdtemp(join(tmpdir(), "kimi-sdk-auth-smoke-home-"));
  const workDir = await mkdtemp(join(tmpdir(), "kimi-sdk-auth-smoke-work-"));
  const harness = createKimiHarness({ homeDir, identity: smokeIdentityFromEnv() });
  const interaction: AuthInteraction = {
    prompt: async () => {
      throw new Error("Kimi OAuth must not prompt for terminal input");
    },
    notify: (event) => {
      if (event.type === "device_code") {
        process.stdout.write(`Open ${event.verificationUri} and enter ${event.userCode}\n`);
      }
    },
  };

  try {
    const login = await harness.auth.login(PROVIDER, "oauth", interaction);
    const first = login.models[0];
    if (first === undefined) throw new Error("login returned no available model");
    await harness.setConfig({ defaultProvider: first.provider, defaultModel: first.id });
    const session = await harness.createSession({
      workDir,
      model: `${first.provider}/${first.id}`,
    });
    const ended = await runPromptToEnd(session, "Reply with exactly: Kimi SDK auth smoke ok");
    if (ended.type !== "turn.ended" || ended.reason !== "completed") {
      throw new Error(`Expected completed turn, got ${ended.type}`);
    }
    process.stdout.write(`auth smoke passed: ${session.id}\n`);
  } finally {
    await harness.auth.logout(PROVIDER).catch(() => {});
    await harness.close();
    await rm(homeDir, { recursive: true, force: true });
    await rm(workDir, { recursive: true, force: true });
  }
}

await main();
