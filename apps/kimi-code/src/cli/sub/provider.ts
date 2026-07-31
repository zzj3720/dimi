import {
  createKimiHarness,
  type KimiHarness,
  type ProviderAuthState,
  type ProviderModel,
} from "@moonshot-ai/kimi-code-sdk";
import type { Command } from "commander";

import { createKimiCodeHostIdentity } from "#/cli/version";

interface WritableLike {
  write(chunk: string): boolean;
}

export interface ProviderDeps {
  readonly getHarness: () => KimiHarness;
  readonly close: () => Promise<void>;
  readonly stdout: WritableLike;
  readonly stderr: WritableLike;
  readonly exit: (code: number) => never;
}

export async function handleProviderList(
  deps: ProviderDeps,
  options: { readonly json: boolean },
): Promise<void> {
  const harness = deps.getHarness();
  const providers = await harness.auth.providers();
  const models = await harness.auth.models();
  if (options.json) {
    deps.stdout.write(`${JSON.stringify({ providers, models }, null, 2)}\n`);
    return;
  }
  for (const provider of providers) {
    const count = models.filter((model) => model.provider === provider.id).length;
    deps.stdout.write(formatProvider(provider, count));
  }
}

export async function handleProviderModels(deps: ProviderDeps, providerId?: string): Promise<void> {
  const models = await deps.getHarness().auth.models(providerId);
  if (models.length === 0) {
    deps.stdout.write(
      providerId === undefined
        ? "No authenticated provider models are available.\n"
        : `No models are available for "${providerId}". Connect it with "kimi login ${providerId}".\n`,
    );
    return;
  }
  for (const model of models) deps.stdout.write(formatModel(model));
}

export async function handleProviderRefresh(deps: ProviderDeps): Promise<void> {
  const result = await deps.getHarness().auth.refreshModels({ force: true });
  if (result.aborted) {
    deps.stderr.write("Provider refresh cancelled.\n");
    deps.exit(1);
  }
  for (const [provider, error] of result.errors) {
    deps.stderr.write(`${provider}: ${error.message}\n`);
  }
  deps.stdout.write(
    result.errors.size === 0
      ? "Provider model catalogs refreshed.\n"
      : `Provider model catalogs refreshed with ${String(result.errors.size)} failure(s).\n`,
  );
}

export function registerProviderCommand(parent: Command, deps?: Partial<ProviderDeps>): void {
  const provider = parent
    .command("provider")
    .description("Inspect providers and refresh their dynamic model catalogs.");

  provider
    .command("list")
    .description("List built-in providers and connection state.")
    .option("--json", "Emit provider and model data as JSON.", false)
    .action(async (options: { json?: boolean }) => {
      await run(resolveDeps(deps), (resolved) =>
        handleProviderList(resolved, { json: options.json === true }),
      );
    });

  provider
    .command("models [providerId]")
    .description("List models currently available through authenticated providers.")
    .action(async (providerId?: string) => {
      await run(resolveDeps(deps), (resolved) => handleProviderModels(resolved, providerId));
    });

  provider
    .command("refresh")
    .description("Refresh authenticated providers from their model-list endpoints.")
    .action(async () => {
      await run(resolveDeps(deps), handleProviderRefresh);
    });
}

function resolveDeps(overrides: Partial<ProviderDeps> = {}): ProviderDeps {
  let harness: KimiHarness | undefined;
  const identity = createKimiCodeHostIdentity();
  const ownsHarness = overrides.getHarness === undefined;
  return {
    getHarness:
      overrides.getHarness ??
      (() => {
        harness ??= createKimiHarness({ identity });
        return harness;
      }),
    close:
      overrides.close ??
      (ownsHarness
        ? async () => {
            await harness?.close();
          }
        : async () => {}),
    stdout: overrides.stdout ?? process.stdout,
    stderr: overrides.stderr ?? process.stderr,
    exit: overrides.exit ?? ((code: number) => process.exit(code)),
  };
}

async function run(
  deps: ProviderDeps,
  action: (deps: ProviderDeps) => Promise<void>,
): Promise<void> {
  try {
    await action(deps);
  } catch (error) {
    deps.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    deps.exit(1);
  } finally {
    await deps.close();
  }
}

function formatProvider(provider: ProviderAuthState, models: number): string {
  const connection = provider.configured
    ? provider.credentialType === "oauth"
      ? "oauth"
      : "api-key"
    : "disconnected";
  return `${provider.id}\t${connection}\tmodels=${String(models)}\t${provider.name}\n`;
}

function formatModel(model: ProviderModel): string {
  const flags = [
    model.reasoning ? "reasoning" : undefined,
    model.input.includes("image") ? "image" : undefined,
  ].filter((value): value is string => value !== undefined);
  return `${model.provider}/${model.id}\tcontext=${String(model.contextWindow)}${flags.length === 0 ? "" : `\t${flags.join(",")}`}\t${model.name}\n`;
}
