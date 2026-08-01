import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { createDimiHarness } from "@dimi-agent/dimi-sdk";

import { smokeIdentityFromEnv } from "./runtime-smoke-helpers";

async function main(): Promise<void> {
  const homeDir = await mkdtemp(join(tmpdir(), "dimi-harness-config-home-"));
  const harness = createDimiHarness({ homeDir, identity: smokeIdentityFromEnv() });
  await harness.setConfig({
    defaultProvider: "kimi-coding",
    defaultModel: "kimi-for-coding",
    thinking: { enabled: true },
    services: {
      moonshotSearch: {
        baseUrl: "https://api.kimi.com/coding/v1/search",
        apiKey: "",
      },
    },
  });

  const configPath = join(homeDir, "config.toml");
  const text = await readFile(configPath, "utf-8");
  for (const expected of [
    'default_provider = "kimi-coding"',
    'default_model = "kimi-for-coding"',
    "[services.moonshot_search]",
  ]) {
    if (!text.includes(expected)) throw new Error(`missing ${expected} in written config`);
  }
  process.stdout.write(`config: ${configPath}\nok\n`);
  await harness.close();
}

await main();
