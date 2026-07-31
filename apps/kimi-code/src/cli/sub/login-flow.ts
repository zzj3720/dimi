import { createInterface } from "node:readline/promises";

import { createKimiHarness, type AuthInteraction, type AuthType } from "@moonshot-ai/kimi-code-sdk";

import { createKimiCodeHostIdentity } from "#/cli/version";
import { openUrl } from "#/utils/open-url";

const DEFAULT_PROVIDER = "kimi-coding";

export async function runLoginFlow(
  providerId: string = DEFAULT_PROVIDER,
  requestedMethod?: string,
): Promise<never> {
  const harness = createKimiHarness({
    identity: createKimiCodeHostIdentity(),
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
  const readline = createInterface({ input: process.stdin, output: process.stderr });
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
      return readline.question(`${prompt.message} `, { signal: prompt.signal });
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
