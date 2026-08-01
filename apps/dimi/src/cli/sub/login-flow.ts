import { execFileSync } from "node:child_process";
import { closeSync, openSync } from "node:fs";
import { createInterface } from "node:readline/promises";

import { createDimiHarness, type AuthInteraction, type AuthType } from "@dimi-agent/dimi-sdk";

import { createDimiCodeHostIdentity } from "#/cli/version";
import { openUrl } from "#/utils/open-url";

const DEFAULT_PROVIDER = "kimi-coding";

export async function runLoginFlow(
  providerId: string = DEFAULT_PROVIDER,
  requestedMethod?: string,
): Promise<never> {
  const harness = createDimiHarness({
    identity: createDimiCodeHostIdentity(),
    uiMode: "cli",
  });
  const provider = (await harness.auth.providers()).find((entry) => entry.id === providerId);
  if (provider === undefined) {
    process.stderr.write(`Unknown provider: ${providerId}\n`);
    process.exit(1);
  }
  const method = resolveMethod(
    provider.methods.map((entry) => entry.type),
    requestedMethod,
  );
  if (method === undefined) {
    process.stderr.write(
      `Provider ${providerId} does not support method "${requestedMethod ?? ""}".\n`,
    );
    process.exit(1);
  }

  const controller = new AbortController();
  process.once("SIGINT", () => {
    controller.abort();
  });
  // Secret prompts install their own raw-input listener. Keeping readline in
  // non-terminal mode prevents its keypress renderer from echoing those bytes
  // through a PTY while normal question prompts still use the same input.
  const readline = createInterface({ input: process.stdin, output: process.stderr, terminal: false });
  const interaction: AuthInteraction = {
    signal: controller.signal,
    prompt: async (prompt) => {
      if (prompt.type === "select") {
        process.stderr.write(
          `${prompt.options.map((option, index) => `${String(index + 1)}. ${option.label}`).join("\n")}\n`,
        );
        const answer = await readline.question(`${prompt.message} `, { signal: prompt.signal });
        const selected = prompt.options[Number(answer) - 1];
        return selected?.id ?? answer;
      }
      return prompt.type === "secret"
        ? promptSecret(readline, `${prompt.message} `, prompt.signal)
        : readline.question(`${prompt.message} `, { signal: prompt.signal });
    },
    notify: (event) => {
      if (event.type === "device_code") {
        process.stderr.write(
          `Open ${event.verificationUri} and enter code ${event.userCode}.\nWaiting for authorization...\n`,
        );
        try {
          openUrl(event.verificationUri);
        } catch {}
      } else if (event.type === "auth_url") {
        process.stderr.write(`${event.instructions ?? "Open this URL"}: ${event.url}\n`);
        try {
          openUrl(event.url);
        } catch {}
      } else {
        process.stderr.write(`${event.message}\n`);
      }
    },
  };

  try {
    await harness.auth.login(providerId, method, interaction);
    process.stderr.write(`Connected to ${provider.name}.\n`);
    process.exit(0);
  } catch (error) {
    process.stderr.write(
      controller.signal.aborted
        ? "Login cancelled.\n"
        : `Login failed: ${error instanceof Error ? error.message : String(error)}\n`,
    );
    process.exit(1);
  } finally {
    readline.close();
    await harness.close();
  }
}

/** Keep API keys out of terminal scrollback while preserving piped CLI input. */
async function promptSecret(
  readline: ReturnType<typeof createInterface>,
  message: string,
  signal?: AbortSignal,
): Promise<string> {
  if (process.stdin.isTTY === true) {
    return promptTtySecret(readline, message, signal);
  }
  // Pipes have no terminal echo, so readline does not need a private output
  // hook here. This is also the supported path for scripts and E2E harnesses.
  return readline.question(message, { signal });
}

async function promptTtySecret(
  readline: ReturnType<typeof createInterface>,
  message: string,
  signal?: AbortSignal,
): Promise<string> {
  const input = process.stdin;
  readline.pause();
  // The interface was created with `terminal: false`, so it never redraws raw
  // keypresses while this dedicated listener owns the TTY.
  input.setRawMode?.(true);
  setTerminalEcho(false);
  input.resume();
  await new Promise<void>((resolve) => setImmediate(resolve));
  process.stderr.write(message);
  return new Promise<string>((resolve, reject) => {
    let value = "";
    const cleanup = (): void => {
      input.off("data", onData);
      signal?.removeEventListener("abort", onAbort);
      input.setRawMode?.(false);
      setTerminalEcho(true);
      readline.resume();
    };
    const finish = (result: { value: string } | { error: Error }): void => {
      cleanup();
      process.stderr.write("\n");
      if ("value" in result) resolve(result.value);
      else reject(result.error);
    };
    const onAbort = (): void => finish({ error: new Error("Login cancelled") });
    const onData = (chunk: Buffer | string): void => {
      for (const char of String(chunk)) {
        if (char === "\r" || char === "\n") {
          finish({ value });
          return;
        }
        if (char === "\u0003") {
          finish({ error: new Error("Login cancelled") });
          return;
        }
        if (char === "\u007f" || char === "\b") {
          value = value.slice(0, -1);
          process.stderr.write("\b \b");
          continue;
        }
        value += char;
        process.stderr.write("*");
      }
    };
    signal?.addEventListener("abort", onAbort, { once: true });
    input.on("data", onData);
    if (signal?.aborted) onAbort();
  });
}

function setTerminalEcho(enabled: boolean): void {
  if (process.platform === "win32") return;
  try {
    const terminal = openSync("/dev/tty", "r+");
    try {
      execFileSync("stty", [enabled ? "echo" : "-echo"], {
        stdio: [terminal, terminal, "ignore"],
      });
    } finally {
      closeSync(terminal);
    }
  } catch {
    // Raw mode still protects terminals where stty is unavailable.
  }
}

function resolveMethod(methods: readonly AuthType[], requested?: string): AuthType | undefined {
  const normalized =
    requested === "api-key"
      ? "api_key"
      : requested === "oauth" || requested === "api_key"
        ? requested
        : undefined;
  if (requested !== undefined) {
    return normalized !== undefined && methods.includes(normalized) ? normalized : undefined;
  }
  return methods.includes("oauth") ? "oauth" : methods.includes("api_key") ? "api_key" : undefined;
}
