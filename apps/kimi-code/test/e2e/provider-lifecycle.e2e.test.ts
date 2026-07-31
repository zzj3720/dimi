/**
 * Provider CLI lifecycle e2e — launches the production `src/main.ts` entry
 * in child processes, with an isolated home and no real credentials/network.
 */
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { createServer } from "node:http";

import { afterEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const appRoot = join(import.meta.dirname, "..", "..");
const tsxCli = require.resolve("tsx/cli");
const rawTextLoader = join(appRoot, "..", "..", "build", "register-raw-text-loader.mjs");
const main = join(appRoot, "src", "main.ts");
const tempHomes: string[] = [];
const CLI_PROCESS_TIMEOUT_MS = 30_000;
const PTY_BRIDGE = `
import os, pty, select, sys
pid, master = pty.fork()
if pid == 0:
    os.execvpe(sys.argv[1], sys.argv[1:], os.environ)
while True:
    readable, _, _ = select.select([master, sys.stdin.fileno()], [], [])
    if master in readable:
        try: data = os.read(master, 4096)
        except OSError: break
        if not data: break
        os.write(sys.stdout.fileno(), data)
    if sys.stdin.fileno() in readable:
        data = os.read(sys.stdin.fileno(), 4096)
        if data: os.write(master, data)
_, status = os.waitpid(pid, 0)
sys.exit(os.waitstatus_to_exitcode(status))
`;

afterEach(async () => {
  for (const home of tempHomes.splice(0)) await rm(home, { recursive: true, force: true });
});

describe("provider CLI lifecycle", () => {
  it("keeps the production built-in catalog usable and reports malformed models.json", async () => {
    const home = await mkdtemp(join(tmpdir(), "kimi-malformed-models-cli-e2e-"));
    tempHomes.push(home);
    const malformed = '{ "providers": ';
    await writeFile(join(home, "models.json"), malformed, "utf8");

    const result = await runCli(home, ["provider", "list", "--json"]);
    expect(result).toMatchObject({ code: 0 });
    expect(result.stderr).toContain("Failed to load models.json");
    expect(JSON.parse(result.stdout).providers).toContainEqual(expect.objectContaining({ id: "xai" }));
    await expect(readFile(join(home, "models.json"), "utf8")).resolves.toBe(malformed);

    // A Pi-compatible commented replacement is picked up on the following
    // product invocation; it is not necessary to repair/restart the catalog.
    await writeFile(join(home, "models.json"), `{
      // a user-owned endpoint override
      "providers": { "xai": { "baseUrl": "https://gateway.example.test/v1", }, },
    }`, "utf8");
    const reloaded = await runCli(home, ["provider", "list", "--json"]);
    expect(reloaded).toMatchObject({ code: 0, stderr: "" });
    expect(JSON.parse(reloaded.stdout).providers).toContainEqual(expect.objectContaining({ id: "xai" }));
  }, 120_000);

  it("runs Grok 4.5 through the production CLI with its declared context and thinking wire shape after restart", async () => {
    const home = await mkdtemp(join(tmpdir(), "kimi-grok-cli-e2e-"));
    tempHomes.push(home);
    const seen: unknown[] = [];
    const server = createServer((request, response) => {
      void (async () => {
        if (request.url !== "/v1/responses") {
          response.writeHead(404).end();
          return;
        }
        const chunks: Buffer[] = [];
        for await (const chunk of request) chunks.push(Buffer.from(chunk));
        seen.push(JSON.parse(Buffer.concat(chunks).toString("utf8")));
        response.writeHead(200, { "content-type": "text/event-stream" });
        response.end([
          "data: {\"type\":\"response.output_text.delta\",\"delta\":\"local grok reply\"}",
          "",
          "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-local\",\"model\":\"grok-4.5\",\"status\":\"completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}",
          "",
        ].join("\n"));
      })().catch((error: unknown) => response.destroy(error instanceof Error ? error : new Error(String(error))));
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("fake provider did not bind a TCP port");
    const baseUrl = `http://127.0.0.1:${String(address.port)}/v1`;

    try {
      await writeFile(join(home, "models.json"), JSON.stringify({
        providers: { xai: { baseUrl } },
      }), "utf8");
      await writeFile(join(home, "config.toml"), [
        'default_provider = "xai"',
        'default_model = "grok-4.5"',
        "",
        "[thinking]",
        "enabled = true",
        'effort = "high"',
      ].join("\n"), "utf8");

      const environment = { XAI_API_KEY: "test-xai-key" };
      const listed = await runCli(home, ["provider", "models", "xai"], undefined, environment);
      expect(listed).toMatchObject({ code: 0, stderr: "" });
      expect(listed.stdout).toContain("xai/grok-4.5\tcontext=500000\treasoning");

      // This is a fresh production process after the catalog inspection. It
      // reproduces the user's restart path instead of sharing an in-memory
      // model registry from setup.
      const run = await runCli(home, ["-p", "hello from e2e"], undefined, environment);
      expect(run).toMatchObject({ code: 0 });
      expect(run.stdout).toContain("local grok reply");
      expect(seen).toHaveLength(1);
      expect(seen[0]).toMatchObject({
        model: "grok-4.5",
        reasoning: { effort: "high", summary: "auto" },
      });
    } finally {
      await new Promise<void>((resolve, reject) => server.close((error) => error === undefined ? resolve() : reject(error)));
    }
  }, 120_000);

  it("persists a localhost dynamic catalog through provider refresh and a fresh CLI process", async () => {
    const home = await mkdtemp(join(tmpdir(), "kimi-dynamic-catalog-cli-e2e-"));
    tempHomes.push(home);
    let modelRequests = 0;
    const server = createServer((request, response) => {
      if (request.url !== "/v1/models") {
        response.writeHead(404).end();
        return;
      }
      modelRequests += 1;
      expect(request.headers.authorization).toBe("Bearer dynamic-xai-key");
      response.writeHead(200, {
        "content-type": "application/json",
        etag: "catalog-v1",
        // Cache eligibility is compared to the generated catalog timestamp.
        // A real upstream supplies this freshness fact; make it unambiguously
        // newer so the second process proves cache restoration.
        "last-modified": "Wed, 31 Dec 2030 00:00:00 GMT",
      });
      response.end(JSON.stringify({ data: [{
        id: "grok-dynamic", display_name: "Dynamic Grok", context_window: 333_000,
        max_output_tokens: 12_345, reasoning: true, input_modalities: ["text", "image"],
        pricing: { input: 1, output: 2, cache_read: 0.5, cache_write: 0.75 },
        reasoning_efforts: ["high"], default_reasoning_effort: "high",
      }] }));
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("fake catalog did not bind a TCP port");
    const baseUrl = `http://127.0.0.1:${String(address.port)}/v1`;
    try {
      await writeFile(join(home, "models.json"), JSON.stringify({ providers: { xai: { baseUrl } } }), "utf8");
      const environment = { XAI_API_KEY: "dynamic-xai-key" };

      const refreshed = await runCli(home, ["provider", "refresh"], undefined, environment);
      expect(refreshed).toMatchObject({
        code: 0,
        stderr: "",
        stdout: "Provider model catalogs refreshed.\n",
      });
      expect(modelRequests).toBe(1);

      // A distinct product process reads the last-known-good catalog rather
      // than sharing the refresh process's closure or talking to the network.
      const listed = await runCli(home, ["provider", "models", "xai"], undefined, environment);
      expect(listed).toMatchObject({ code: 0, stderr: "" });
      expect(listed.stdout).toContain("xai/grok-dynamic\tcontext=333000\treasoning,image");
      expect(modelRequests).toBe(1);
      await expect(readJson(join(home, "models-store.json"))).resolves.toMatchObject({
        xai: { models: [expect.objectContaining({ id: "grok-dynamic", contextWindow: 333_000, maxTokens: 12_345, thinkingLevelMap: { high: "high" }, defaultThinkingLevel: "high" })] },
      });
    } finally {
      await new Promise<void>((resolve, reject) => server.close((error) => error === undefined ? resolve() : reject(error)));
    }
  }, 120_000);

  it("removes the final custom model from a built-in overlay without removing its built-in catalog", async () => {
    const home = await mkdtemp(join(tmpdir(), "kimi-overlay-model-remove-cli-e2e-"));
    tempHomes.push(home);
    await writeFile(join(home, "models.json"), JSON.stringify({
      providers: { xai: { models: [{ id: "overlay-only", contextWindow: 128_000, maxTokens: 8_192 }] } },
    }), "utf8");

    await expect(runCli(home, ["provider", "model", "remove", "xai", "overlay-only"])).resolves.toMatchObject({
      code: 0,
      stdout: "Removed model xai/overlay-only.\n",
    });
    await expect(readJson(join(home, "models.json"))).resolves.toEqual({ providers: { xai: { models: [] } } });
    const models = await runCli(home, ["provider", "models", "xai"], undefined, { XAI_API_KEY: "overlay-key" });
    expect(models).toMatchObject({ code: 0, stderr: "" });
    expect(models.stdout).toContain("xai/grok-4.5");
    expect(models.stdout).not.toContain("overlay-only");
  }, 120_000);

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

    const definitionPath = join(home, "local-provider.json");
    await writeFile(
      definitionPath,
      JSON.stringify({
        providers: {
          local: {
            name: "Local protocol stub",
            api: "openai-completions",
            baseUrl: "https://api.example.test/v1",
            apiKey: "$LOCAL_PROVIDER_KEY",
            headers: { "x-client": "kimi" },
            models: [{
              id: "local-chat",
              reasoning: true,
              input: ["text", "image"],
              contextWindow: 32_000,
              maxTokens: 4_096,
              cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0 },
              thinkingLevelMap: { low: "low", high: "high" },
            }],
          },
        },
      }),
      "utf8",
    );
    await expect(runCli(home, ["provider", "add", "local", "--from", definitionPath])).resolves.toMatchObject({
      code: 0,
      stdout: "Added provider local.\n",
    });
    await expect(readJson(join(home, "models.json"))).resolves.toMatchObject({
      providers: { local: { headers: { "x-client": "kimi" }, models: [expect.objectContaining({ contextWindow: 32_000 })] } },
    });
    const customCatalog = await runCli(home, ["provider", "list", "--json"]);
    expect(JSON.parse(customCatalog.stdout).providers).toContainEqual(
      expect.objectContaining({ id: "local", custom: true, configured: false }),
    );
    const customLogin = await runCli(home, ["login", "local", "--method", "api-key"], "LOCAL_TEST_KEY\n");
    expect(customLogin.code, customLogin.stderr).toBe(0);
    const customModels = await runCli(home, ["provider", "models", "local"]);
    expect(customModels.stdout).toContain("local/local-chat\tcontext=32000\treasoning,image");
    await expect(runCli(home, ["provider", "update", "local", "--model", "local-chat", "--max-tokens", "8192"])).resolves.toMatchObject({
      code: 0,
      stdout: "Updated provider local.\n",
    });
    await expect(readJson(join(home, "models.json"))).resolves.toMatchObject({
      providers: {
        local: {
          headers: { "x-client": "kimi" },
          models: [expect.objectContaining({ id: "local-chat", contextWindow: 32_000, maxTokens: 8_192, cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0 }, thinkingLevelMap: { low: "low", high: "high" } })],
        },
      },
    });
    await expect(runCli(home, ["provider", "model", "add", "local", "local-extra", "--context-window", "64000", "--max-tokens", "4096"])).resolves.toMatchObject({
      code: 0,
      stdout: "Saved model local/local-extra.\n",
    });
    await expect(runCli(home, ["provider", "model", "update", "local", "local-extra", "--image", "--thinking"])).resolves.toMatchObject({
      code: 0,
      stdout: "Saved model local/local-extra.\n",
    });
    await expect(readJson(join(home, "models.json"))).resolves.toMatchObject({
      providers: { local: { models: expect.arrayContaining([
        expect.objectContaining({ id: "local-extra", contextWindow: 64_000, maxTokens: 4_096, reasoning: true, input: ["text", "image"] }),
      ]) } },
    });
    await expect(runCli(home, ["provider", "model", "remove", "local", "local-extra"])).resolves.toMatchObject({
      code: 0,
      stdout: "Removed model local/local-extra.\n",
    });
    await expect(runCli(home, ["provider", "remove", "local"])).resolves.toMatchObject({
      code: 0,
      stdout: "Removed provider local.\n",
    });

    const secret = "CLI_PROVIDER_SECRET_SENTINEL_9f4d";
    const login = await runCliPty(home, ["login", "anthropic", "--method", "api-key"], `${secret}\n`);
    expect(login.code, login.stderr).toBe(0);
    expect(login.signal).toBeNull();
    expect(login.stderr).toContain("Connected to Anthropic.");
    expect(`${login.stdout}${login.stderr}`).not.toContain(secret);
    for (const file of ["auth.json", "models-store.json", "device_id"]) {
      expect((await stat(join(home, file))).mode & 0o777).toBe(0o600);
    }
    await expect(readJson(join(home, "auth.json"))).resolves.toMatchObject({
      anthropic: { type: "api_key", key: secret },
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
  envOverrides: Readonly<Record<string, string>> = {},
): Promise<{
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
}> {
  const env = sanitizedProviderEnv(envOverrides);
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

function runCliPty(
  home: string,
  args: readonly string[],
  input: string,
): Promise<{ code: number | null; signal: NodeJS.Signals | null; stdout: string; stderr: string }> {
  const env = sanitizedProviderEnv();
  return new Promise((resolve, reject) => {
    const command = [
      process.execPath,
      tsxCli,
      "--tsconfig",
      join(appRoot, "tsconfig.dev.json"),
      "--import",
      rawTextLoader,
      main,
      ...args,
    ];
    const child = spawn("python3", ["-u", "-c", PTY_BRIDGE, ...command], {
      cwd: appRoot,
      env: { ...env, KIMI_CODE_HOME: home, KIMI_LOG_LEVEL: "off", TERM: "xterm-256color" },
      stdio: ["pipe", "pipe", "pipe"],
    });
    let output = "";
    let sent = false;
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`CLI PTY timed out: ${args.join(" ")}`));
    }, CLI_PROCESS_TIMEOUT_MS);
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    const receive = (chunk: string): void => {
      output += chunk;
      if (!sent && output.includes("Enter Anthropic API key:")) {
        sent = true;
        // A real terminal user types after readline has switched the TTY to
        // hidden/raw input; avoid injecting a paste in the same write tick as
        // the prompt redraw.
        setTimeout(() => child.stdin.end(input), 500);
      }
    };
    child.stdout.on("data", receive);
    child.stderr.on("data", receive);
    child.once("error", (error) => { clearTimeout(timeout); reject(error); });
    child.once("close", (code, signal) => {
      clearTimeout(timeout);
      resolve({ code, signal, stdout: output, stderr: output });
    });
  });
}

/** Tests opt into one provider at a time; inherited shell credentials must not make refresh nondeterministic. */
function sanitizedProviderEnv(overrides: Readonly<Record<string, string>> = {}): NodeJS.ProcessEnv {
  const env = { ...process.env };
  for (const name of [
    "ANT_LING_API_KEY", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_OAUTH_TOKEN",
    "AZURE_OPENAI_API_KEY", "CEREBRAS_API_KEY", "CLOUDFLARE_API_KEY", "COPILOT_GITHUB_TOKEN",
    "DEEPSEEK_API_KEY", "FIREWORKS_API_KEY", "GEMINI_API_KEY", "GOOGLE_CLOUD_API_KEY",
    "GOOGLE_CLOUD_PROJECT", "GOOGLE_CLOUD_LOCATION", "GOOGLE_APPLICATION_CREDENTIALS", "GCLOUD_PROJECT",
    "GROQ_API_KEY", "HF_TOKEN", "KIMI_API_KEY", "MINIMAX_API_KEY", "MINIMAX_CN_API_KEY",
    "MISTRAL_API_KEY", "MOONSHOT_API_KEY", "NVIDIA_API_KEY", "OPENAI_API_KEY", "OPENCODE_API_KEY",
    "OPENROUTER_API_KEY", "QWEN_TOKEN_PLAN_API_KEY", "QWEN_TOKEN_PLAN_CN_API_KEY", "RADIUS_API_KEY",
    "TOGETHER_API_KEY", "AI_GATEWAY_API_KEY", "XAI_API_KEY", "XIAOMI_API_KEY",
    "XIAOMI_TOKEN_PLAN_AMS_API_KEY", "XIAOMI_TOKEN_PLAN_CN_API_KEY", "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
    "ZAI_API_KEY", "ZAI_CODING_CN_API_KEY", "AWS_PROFILE", "AWS_REGION", "AWS_DEFAULT_REGION",
  ]) delete env[name];
  return { ...env, ...overrides };
}

async function readJson(path: string): Promise<unknown> {
  return JSON.parse(await readFile(path, "utf8")) as unknown;
}
