/**
 * Provider CLI lifecycle e2e — launches the production `src/main.ts` entry
 * in child processes, with an isolated home and no real credentials/network.
 */
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

import { afterEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const appRoot = join(import.meta.dirname, "..", "..");
const tsxCli = require.resolve("tsx/cli");
const rawTextLoader = join(appRoot, "..", "..", "build", "register-raw-text-loader.mjs");
const main = join(appRoot, "src", "main.ts");
const tempHomes: string[] = [];
const CLI_PROCESS_TIMEOUT_MS = 30_000;

afterEach(async () => {
  for (const home of tempHomes.splice(0)) await rm(home, { recursive: true, force: true });
});

describe("provider CLI lifecycle", () => {
  it("lists, logs in, reads built-ins, logs out, and exits cleanly through the production entry point", async () => {
    const home = await mkdtemp(join(tmpdir(), "kimi-provider-e2e-"));
    tempHomes.push(home);

    const initial = await runCli(home, ["provider", "list", "--json"]);
    expect(initial).toMatchObject({ code: 0, signal: null, stderr: "" });
    const initialCatalog = JSON.parse(initial.stdout) as {
      providers: Array<{ id: string; configured: boolean; methods: Array<{ type: string }> }>;
    };
    for (const [id, method] of [
      ["kimi-coding", "oauth"],
      ["openai-codex", "oauth"],
      ["xai", "oauth"],
      ["anthropic", "api_key"],
    ]) {
      const provider = initialCatalog.providers.find((entry) => entry.id === id);
      expect(provider?.id).toBe(id);
      expect(provider?.methods.some((entry) => entry.type === method)).toBe(true);
    }

    const login = await runCli(
      home,
      ["login", "anthropic", "--method", "api-key"],
      "YOUR_API_KEY\n",
    );
    expect(login).toMatchObject({ code: 0, signal: null });
    expect(login.stderr).toContain("Connected to Anthropic.");
    for (const file of ["auth.json", "models-store.json", "device_id"]) {
      expect((await stat(join(home, file))).mode & 0o777).toBe(0o600);
    }
    await expect(readJson(join(home, "auth.json"))).resolves.toMatchObject({
      anthropic: { type: "api_key", key: "YOUR_API_KEY" },
    });

    const authenticated = await runCli(home, ["provider", "list", "--json"]);
    expect(authenticated).toMatchObject({ code: 0, signal: null, stderr: "" });
    const authenticatedCatalog = JSON.parse(authenticated.stdout) as {
      providers: Array<{ id: string; configured: boolean; credentialType?: string }>;
    };
    expect(
      authenticatedCatalog.providers.find((provider) => provider.id === "anthropic"),
    ).toMatchObject({
      id: "anthropic",
      configured: true,
      credentialType: "api_key",
    });

    const models = await runCli(home, ["provider", "models", "anthropic"]);
    expect(models).toMatchObject({ code: 0, signal: null, stderr: "" });
    expect(models.stdout).toContain("anthropic/claude-sonnet-4-6");

    const logout = await runCli(home, ["logout", "anthropic"]);
    expect(logout).toMatchObject({ code: 0, signal: null, stderr: "" });
    expect(logout.stdout).toBe("Disconnected from anthropic.\n");

    const disconnected = await runCli(home, ["provider", "list", "--json"]);
    expect(disconnected).toMatchObject({ code: 0, signal: null, stderr: "" });
    const disconnectedCatalog = JSON.parse(disconnected.stdout) as {
      providers: Array<{ id: string; configured: boolean; credentialType?: string }>;
    };
    const disconnectedAnthropic = disconnectedCatalog.providers.find(
      (provider) => provider.id === "anthropic",
    );
    expect(disconnectedAnthropic?.configured).toBe(false);
    expect(disconnectedAnthropic?.credentialType).toBeUndefined();
    await expect(readJson(join(home, "auth.json"))).resolves.toEqual({});

    await rm(home, { recursive: true, force: true });
    tempHomes.splice(tempHomes.indexOf(home), 1);
    expect(existsSync(home)).toBe(false);
  }, 120_000);
});

function runCli(
  home: string,
  args: readonly string[],
  input?: string,
): Promise<{
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
}> {
  const { ANTHROPIC_API_KEY: _anthropicApiKey, ...env } = process.env;
  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [
        tsxCli,
        "--tsconfig",
        join(appRoot, "tsconfig.dev.json"),
        "--import",
        rawTextLoader,
        main,
        ...args,
      ],
      {
        cwd: appRoot,
        env: { ...env, KIMI_CODE_HOME: home, KIMI_LOG_LEVEL: "off" },
        stdio: ["pipe", "pipe", "pipe"],
      },
    );
    let stdout = "";
    let stderr = "";
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`CLI timed out: ${args.join(" ")}`));
    }, CLI_PROCESS_TIMEOUT_MS);
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    child.once("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.once("close", (code, signal) => {
      clearTimeout(timeout);
      resolve({ code, signal, stdout, stderr });
    });
    child.stdin.end(input);
  });
}

async function readJson(path: string): Promise<unknown> {
  return JSON.parse(await readFile(path, "utf8")) as unknown;
}
