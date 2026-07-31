import {
  createKimiHarness,
  type KimiHarness,
  type ProviderAuthState,
  type ProviderModel,
  type CustomProviderInput,
  parseJsonc,
} from "@moonshot-ai/kimi-code-sdk";
import { readFile } from "node:fs/promises";
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
  const diagnostic = await harness.auth.providerDefinitionDiagnostic();
  if (diagnostic !== undefined) deps.stderr.write(`${diagnostic}\n`);
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
  const auth = deps.getHarness().auth;
  const models = await auth.models(providerId);
  const diagnostic = await auth.providerDefinitionDiagnostic();
  if (diagnostic !== undefined) deps.stderr.write(`${diagnostic}\n`);
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

export async function handleProviderUpsert(
  deps: ProviderDeps,
  id: string,
  options: CustomProviderOptions,
): Promise<void> {
  const auth = deps.getHarness().auth;
  const existing = (await auth.customProviders()).find((provider) => provider.id === id);
  const imported = options.from === undefined ? undefined : await readProviderDefinition(options.from, id);
  if (imported !== undefined) {
    await auth.upsertCustomProvider(imported);
    deps.stdout.write(`${existing === undefined ? "Added" : "Updated"} provider ${id}.\n`);
    return;
  }
  const modelId = options.model ?? existing?.models?.[0]?.id;
  if (modelId === undefined) throw new Error("--model is required when creating a provider");
  const baseUrl = options.baseUrl ?? existing?.baseUrl;
  if (baseUrl === undefined) throw new Error("--base-url is required when creating a provider");
  const api = options.api ?? existing?.api ?? "openai-completions";
  if (
    existing === undefined &&
    (options.contextWindow === undefined || options.maxTokens === undefined)
  ) {
    throw new Error("--context-window and --max-tokens are required when creating a provider");
  }
  const previousModel = existing?.models?.find((item) => item.id === modelId);
  const model = {
    ...previousModel,
    id: modelId,
    name: options.modelName ?? previousModel?.name,
    contextWindow: options.contextWindow ?? previousModel?.contextWindow,
    maxTokens: options.maxTokens ?? previousModel?.maxTokens,
    reasoning: options.thinking === true ? true : previousModel?.reasoning,
    input: options.image === true ? (["text", "image"] as const) : previousModel?.input,
  };
  const models = new Map(existing?.models?.map((item) => [item.id, item]) ?? []);
  models.set(modelId, { ...models.get(modelId), ...model });
  const definition: CustomProviderInput = {
    ...existing,
    id,
    name: options.name ?? existing?.name ?? id,
    api,
    baseUrl,
    apiKey: options.apiKeyEnv === undefined ? existing?.apiKey : `$${options.apiKeyEnv}`,
    models: [...models.values()],
  };
  await auth.upsertCustomProvider(definition);
  deps.stdout.write(`${existing === undefined ? "Added" : "Updated"} provider ${id}.\n`);
}

export async function handleProviderRemove(deps: ProviderDeps, id: string): Promise<void> {
  await deps.getHarness().auth.deleteCustomProvider(id);
  deps.stdout.write(`Removed provider ${id}.\n`);
}

export async function handleProviderModelUpsert(
  deps: ProviderDeps,
  providerId: string,
  modelId: string,
  options: CustomModelOptions,
): Promise<void> {
  const provider = (await deps.getHarness().auth.customProviders())
    .find((entry) => entry.id === providerId);
  const existing = provider?.models?.find((model) => model.id === modelId);
  if (
    existing === undefined &&
    (options.contextWindow === undefined || options.maxTokens === undefined)
  ) {
    throw new Error("--context-window and --max-tokens are required when adding a model");
  }
  await deps.getHarness().auth.upsertCustomModel(providerId, {
    ...existing,
    id: modelId,
    name: options.name ?? existing?.name,
    contextWindow: options.contextWindow ?? existing?.contextWindow,
    maxTokens: options.maxTokens ?? existing?.maxTokens,
    reasoning: options.thinking === true ? true : existing?.reasoning,
    input: options.image === true ? ["text", "image"] : existing?.input,
  });
  deps.stdout.write(`Saved model ${providerId}/${modelId}.\n`);
}

export async function handleProviderModelRemove(
  deps: ProviderDeps,
  providerId: string,
  modelId: string,
): Promise<void> {
  await deps.getHarness().auth.deleteCustomModel(providerId, modelId);
  deps.stdout.write(`Removed model ${providerId}/${modelId}.\n`);
}

export function registerProviderCommand(parent: Command, deps?: Partial<ProviderDeps>): void {
  const provider = parent
    .command("provider")
    .description("Inspect, connect, and manage built-in or custom providers.");

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

  provider
    .command("add <id>")
    .description("Create a custom provider and its first model.")
    .option("--base-url <url>", "Provider API base URL")
    .option("--model <id>", "Initial model id")
    .option("--from <path>", "Complete custom-provider JSON definition")
    .option("--name <name>", "Display name")
    .option("--model-name <name>", "Initial model display name")
    .option("--api <api>", "Protocol adapter", "openai-completions")
    .option("--api-key-env <name>", "Optional environment variable holding the API key")
    .option("--context-window <tokens>", "Model context window", parsePositiveInteger)
    .option("--max-tokens <tokens>", "Model max output tokens", parsePositiveInteger)
    .option("--thinking", "Enable thinking controls for this model")
    .option("--image", "Enable image input for this model")
    .action(async (id: string, options: CustomProviderOptions) => {
      await run(resolveDeps(deps), (resolved) => handleProviderUpsert(resolved, id, options));
    });

  provider
    .command("update <id>")
    .description("Update a custom provider or add/update one of its models.")
    .option("--base-url <url>", "Provider API base URL")
    .option("--model <id>", "Model id to add or update")
    .option("--from <path>", "Complete custom-provider JSON definition")
    .option("--name <name>", "Display name")
    .option("--model-name <name>", "Model display name")
    .option("--api <api>", "Protocol adapter")
    .option("--api-key-env <name>", "Optional environment variable holding the API key")
    .option("--context-window <tokens>", "Model context window", parsePositiveInteger)
    .option("--max-tokens <tokens>", "Model max output tokens", parsePositiveInteger)
    .option("--thinking", "Enable thinking controls for this model")
    .option("--image", "Enable image input for this model")
    .action(async (id: string, options: CustomProviderOptions) => {
      await run(resolveDeps(deps), (resolved) => handleProviderUpsert(resolved, id, options));
    });

  provider
    .command("remove <id>")
    .description("Remove a custom provider and its stored API key.")
    .action(async (id: string) => {
      await run(resolveDeps(deps), (resolved) => handleProviderRemove(resolved, id));
    });

  const model = provider.command("model").description("Manage custom provider models.");
  for (const name of ["add", "update"] as const) {
    model
      .command(`${name} <providerId> <modelId>`)
      .description(`${name === "add" ? "Add" : "Update"} a custom provider model.`)
      .option("--name <name>", "Model display name")
      .option("--context-window <tokens>", "Model context window", parsePositiveInteger)
      .option("--max-tokens <tokens>", "Model max output tokens", parsePositiveInteger)
      .option("--thinking", "Enable thinking controls")
      .option("--image", "Enable image input")
      .action(async (providerId: string, modelId: string, options: CustomModelOptions) => {
        await run(resolveDeps(deps), (resolved) =>
          handleProviderModelUpsert(resolved, providerId, modelId, options),
        );
      });
  }
  model
    .command("remove <providerId> <modelId>")
    .description("Remove one custom provider model.")
    .action(async (providerId: string, modelId: string) => {
      await run(resolveDeps(deps), (resolved) =>
        handleProviderModelRemove(resolved, providerId, modelId),
      );
    });
}

interface CustomProviderOptions {
  readonly from?: string;
  readonly baseUrl?: string;
  readonly model?: string;
  readonly name?: string;
  readonly modelName?: string;
  readonly api?: CustomProviderInput["api"];
  readonly apiKeyEnv?: string;
  readonly contextWindow?: number;
  readonly maxTokens?: number;
  readonly thinking?: boolean;
  readonly image?: boolean;
}

async function readProviderDefinition(path: string, id: string): Promise<CustomProviderInput> {
  const parsed = parseJsonc(await readFile(path, "utf8"));
  const definition =
    typeof parsed === "object" && parsed !== null && "providers" in parsed
      ? (parsed as { providers?: Record<string, unknown> }).providers?.[id]
      : parsed;
  if (typeof definition !== "object" || definition === null) {
    throw new Error(`Provider definition ${id} was not found in ${path}`);
  }
  return { ...(definition as CustomProviderInput), id };
}

interface CustomModelOptions {
  readonly name?: string;
  readonly contextWindow?: number;
  readonly maxTokens?: number;
  readonly thinking?: boolean;
  readonly image?: boolean;
}

function parsePositiveInteger(value: string): number {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 1) throw new Error("Expected a positive integer");
  return parsed;
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
