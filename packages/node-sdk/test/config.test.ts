import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { createDimiConfigRpc, createDimiHarness, DimiError } from "#/index";

import { TEST_IDENTITY } from "./test-identity";

// node-sdk/agent-core normalize paths to forward slashes (pathe). Mirror that
// in path assertions so they hold on Windows, where node:path produces
// backslashes.
const toPosix = (p: string): string => p.replaceAll("\\", "/");

const tempDirs: string[] = [];

afterEach(async () => {
  vi.unstubAllEnvs();
  for (const dir of tempDirs.splice(0)) {
    await rm(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 20 });
  }
});

async function makeTempDir(): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), "dimi-sdk-config-"));
  tempDirs.push(dir);
  return dir;
}

const COMPLETE_TOML = `
default_provider = "kimi-coding"
default_model = "kimi-for-coding"
default_permission_mode = "auto"
skip_afk_prompt_injection = false
default_plan_mode = false
default_editor = ""
theme = "dark"
show_thinking_stream = true
merge_all_available_skills = true
extra_skill_dirs = ["~/team-skills", ".agents/team-skills"]

[loop_control]
max_retries_per_step = 3
max_ralph_iterations = 0
reserved_context_size = 50000
compaction_trigger_ratio = 0.85

[task]
max_running_tasks = 4
kill_grace_period_ms = 2000
print_wait_ceiling_s = 3600

[services.moonshot_search]
base_url = "https://api.kimi.com/coding/v1/search"
api_key = "sk-search"
custom_headers = { "X-Search" = "1" }

[services.moonshot_fetch]
base_url = "https://api.kimi.com/coding/v1/fetch"
api_key = "sk-fetch"

[notifications]
claim_stale_after_ms = 15000

[thinking]
enabled = true
effort = "high"
`;

describe("SDK config TOML", () => {
  it("resolves config paths through the config RPC wrapper", async () => {
    const dir = await makeTempDir();
    const rpc = createDimiConfigRpc();

    await expect(rpc.resolveConfigPath({ homeDir: dir })).resolves.toBe(
      toPosix(join(dir, "config.toml")),
    );
  });

  it("returns structured validation issues through the config RPC wrapper", async () => {
    const rpc = createDimiConfigRpc();

    await expect(
      rpc.validateConfigToml({
        text: `
[model_catalog]
refresh_interval_ms = "often"
`,
        filePath: "broken.toml",
      }),
    ).rejects.toMatchObject({
      details: {
        validationIssues: [
          {
            path: ["refreshIntervalMs"],
          },
        ],
      },
    });
  });
});

describe("DimiHarness config API", () => {
  it("loads default config when missing and deep-merges setConfig patches from disk", async () => {
    const homeDir = await makeTempDir();
    const configPath = join(homeDir, "config.toml");
    await writeFile(configPath, COMPLETE_TOML, "utf-8");

    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });

    await harness.setConfig({
      services: {
        moonshotSearch: {
          apiKey: "sk-search-updated",
        },
      },
    });

    const config = await harness.getConfig({ reload: true });
    expect(config.services?.moonshotSearch?.apiKey).toBe("sk-search-updated");
    const text = await readFile(configPath, "utf-8");
    expect(text).toContain('theme = "dark"');
    expect(text).toContain("claim_stale_after_ms = 15000");
  });

  it("does not write invalid config patches", async () => {
    const homeDir = await makeTempDir();
    const configPath = join(homeDir, "config.toml");
    await writeFile(configPath, COMPLETE_TOML, "utf-8");
    const before = await readFile(configPath, "utf-8");

    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });

    const setInvalidConfig = harness.setConfig({
      thinking: { enabled: "yes" },
    } as never);

    await expect(setInvalidConfig).rejects.toBeInstanceOf(DimiError);
    await expect(setInvalidConfig).rejects.toMatchObject({
      code: "config.invalid",
    } satisfies Partial<DimiError>);

    await expect(readFile(configPath, "utf-8")).resolves.toBe(before);
  });

  it("uses default config when the config file is absent", async () => {
    const homeDir = await makeTempDir();
    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });

    await expect(harness.getConfig()).resolves.toMatchObject({
      services: {},
    });
  });

  it("returns experimental feature metadata through the harness", async () => {
    vi.stubEnv("DIMI_CODE_EXPERIMENTAL_FLAG", "0");
    const homeDir = await makeTempDir();
    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });

    const features = await harness.getExperimentalFeatures();
    expect(features.map((feature) => feature.id)).toEqual(
      expect.arrayContaining(["tool-select", "secondary-model"]),
    );
    expect(features.every((feature) => typeof feature.enabled === "boolean")).toBe(true);
  });

  it("can create the default config scaffold without selecting a model", async () => {
    const homeDir = await makeTempDir();
    const configPath = join(homeDir, "config.toml");
    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });

    await harness.ensureConfigFile();

    const text = await readFile(configPath, "utf-8");
    expect(text).toContain("Dimi runtime settings.");
    expect(text).not.toMatch(/^default_thinking =/m);
    expect(text).not.toMatch(/^default_model =/m);

    const config = await harness.getConfig({ reload: true });
    expect(config.defaultModel).toBeUndefined();
    expect(config.thinking?.enabled).toBeUndefined();
  });

  it("reloads an active session without closing the SDK session wrapper", async () => {
    const homeDir = await makeTempDir();
    const workDir = join(homeDir, "work");
    const configPath = join(homeDir, "config.toml");
    await writeFile(configPath, COMPLETE_TOML, "utf-8");
    await mkdir(workDir, { recursive: true });
    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });
    const session = await harness.createSession({
      id: "session-sdk-reload",
      workDir,
      model: "kimi-for-coding",
    });

    expect(session.getResumeState()).toBeUndefined();

    const reloaded = await harness.reloadSession({ id: session.id });

    expect(reloaded).toBe(session);
    expect(harness.getSession(session.id)).toBe(session);
    expect(session.getResumeState()?.agents["main"]).toBeDefined();
    await expect(session.getStatus()).resolves.toMatchObject({ model: "kimi-for-coding" });
  });

  it("forwards forcePluginSessionStartReminder to the active session reload", async () => {
    const homeDir = await makeTempDir();
    const workDir = join(homeDir, "work");
    const configPath = join(homeDir, "config.toml");
    await writeFile(configPath, COMPLETE_TOML, "utf-8");
    await mkdir(workDir, { recursive: true });
    const harness = createDimiHarness({ homeDir, identity: TEST_IDENTITY });
    const session = await harness.createSession({
      id: "session-sdk-reload-forward",
      workDir,
      model: "kimi-for-coding",
    });

    const reloadSpy = vi.spyOn(session, "reloadSession").mockResolvedValue({} as never);

    await harness.reloadSession({ id: session.id, forcePluginSessionStartReminder: true });

    expect(reloadSpy).toHaveBeenCalledWith({ forcePluginSessionStartReminder: true });
  });
});
