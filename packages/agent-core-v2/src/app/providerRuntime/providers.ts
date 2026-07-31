import { envApiKeyAuth, kimiCodingOAuth, openaiCodexOAuth, xaiOAuth } from "./auth";
import { streamProvider } from "./stream";
import type {
  Api,
  AuthResult,
  Model,
  Provider,
  ProviderHeaders,
  RefreshModelsContext,
} from "./types";

const CATALOG_REFRESH_INTERVAL_MS = 4 * 60 * 60 * 1_000;
const DEFAULT_CONTEXT_WINDOW = 128_000;
const DEFAULT_MAX_TOKENS = 32_000;
const NO_COST = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

interface ProviderDefinition {
  id: string;
  name: string;
  baseUrl: string;
  api: Api;
  envNames?: readonly string[];
  oauth?: Provider["auth"]["oauth"];
  models: readonly ModelDefinition[];
  catalog?: {
    path: string;
    parse?: (value: unknown) => readonly CatalogModel[];
  };
}

interface ModelDefinition {
  id: string;
  name?: string;
  api?: Api;
  reasoning?: boolean;
  image?: boolean;
  contextWindow?: number;
  maxTokens?: number;
}

interface CatalogModel {
  id: string;
  name?: string;
  api?: Api;
  reasoning?: boolean;
  image?: boolean;
  contextWindow?: number;
  maxTokens?: number;
}

export function builtinProviders(
  hostHeaders: Readonly<Record<string, string>> = {},
): readonly Provider[] {
  return [
    provider(
      {
        id: "kimi-coding",
        name: "Kimi Code",
        baseUrl: "https://api.kimi.com/coding/v1",
        api: "anthropic-messages",
        envNames: ["KIMI_API_KEY"],
        oauth: kimiCodingOAuth,
        catalog: { path: "/models" },
        models: [{ id: "kimi-for-coding", name: "Kimi for Coding", reasoning: true }],
      },
      hostHeaders,
    ),
    provider(
      {
        id: "openai-codex",
        name: "OpenAI Codex",
        baseUrl: "https://chatgpt.com/backend-api",
        api: "openai-codex-responses",
        oauth: openaiCodexOAuth,
        catalog: { path: "/codex/models", parse: parseCodexCatalog },
        models: [{ id: "gpt-5.4", name: "GPT-5.4", reasoning: true, image: true }],
      },
      hostHeaders,
    ),
    provider(
      {
        id: "xai",
        name: "xAI",
        baseUrl: "https://api.x.ai/v1",
        api: "openai-completions",
        envNames: ["XAI_API_KEY"],
        oauth: xaiOAuth,
        catalog: { path: "/models" },
        models: [{ id: "grok-4", name: "Grok 4", reasoning: true, image: true }],
      },
      hostHeaders,
    ),
    provider(
      {
        id: "openai",
        name: "OpenAI",
        baseUrl: "https://api.openai.com/v1",
        api: "openai-responses",
        envNames: ["OPENAI_API_KEY"],
        catalog: { path: "/models" },
        models: [{ id: "gpt-5.4", name: "GPT-5.4", reasoning: true, image: true }],
      },
      hostHeaders,
    ),
    provider(
      {
        id: "anthropic",
        name: "Anthropic",
        baseUrl: "https://api.anthropic.com/v1",
        api: "anthropic-messages",
        envNames: ["ANTHROPIC_API_KEY"],
        models: [
          {
            id: "claude-sonnet-4-6",
            name: "Claude Sonnet 4.6",
            reasoning: true,
            image: true,
            contextWindow: 200_000,
            maxTokens: 64_000,
          },
        ],
      },
      hostHeaders,
    ),
    compatibleProvider(
      "openrouter",
      "OpenRouter",
      "https://openrouter.ai/api/v1",
      ["OPENROUTER_API_KEY"],
      "openrouter/auto",
      hostHeaders,
    ),
    compatibleProvider(
      "deepseek",
      "DeepSeek",
      "https://api.deepseek.com/v1",
      ["DEEPSEEK_API_KEY"],
      "deepseek-chat",
      hostHeaders,
    ),
    compatibleProvider(
      "groq",
      "Groq",
      "https://api.groq.com/openai/v1",
      ["GROQ_API_KEY"],
      "openai/gpt-oss-120b",
      hostHeaders,
    ),
    compatibleProvider(
      "mistral",
      "Mistral",
      "https://api.mistral.ai/v1",
      ["MISTRAL_API_KEY"],
      "mistral-large-latest",
      hostHeaders,
    ),
    compatibleProvider(
      "together",
      "Together AI",
      "https://api.together.xyz/v1",
      ["TOGETHER_API_KEY"],
      "moonshotai/Kimi-K2.5",
      hostHeaders,
    ),
    compatibleProvider(
      "cerebras",
      "Cerebras",
      "https://api.cerebras.ai/v1",
      ["CEREBRAS_API_KEY"],
      "gpt-oss-120b",
      hostHeaders,
    ),
    compatibleProvider(
      "fireworks",
      "Fireworks AI",
      "https://api.fireworks.ai/inference/v1",
      ["FIREWORKS_API_KEY"],
      "accounts/fireworks/models/kimi-k2p5",
      hostHeaders,
    ),
    compatibleProvider(
      "zai",
      "Z.AI",
      "https://api.z.ai/api/paas/v4",
      ["ZAI_API_KEY"],
      "glm-5",
      hostHeaders,
    ),
    compatibleProvider(
      "qwen",
      "Alibaba Cloud Model Studio",
      "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
      ["DASHSCOPE_API_KEY"],
      "qwen3.5-plus",
      hostHeaders,
    ),
    compatibleProvider(
      "moonshot",
      "Moonshot AI",
      "https://api.moonshot.ai/v1",
      ["MOONSHOT_API_KEY", "KIMI_API_KEY"],
      "kimi-k2.5",
      hostHeaders,
    ),
  ];
}

function compatibleProvider(
  id: string,
  name: string,
  baseUrl: string,
  envNames: readonly string[],
  fallbackModel: string,
  hostHeaders: Readonly<Record<string, string>>,
): Provider {
  return provider(
    {
      id,
      name,
      baseUrl,
      api: "openai-completions",
      envNames,
      catalog: { path: "/models" },
      models: [{ id: fallbackModel, reasoning: inferReasoning(fallbackModel), image: true }],
    },
    hostHeaders,
  );
}

function provider(
  definition: ProviderDefinition,
  hostHeaders: Readonly<Record<string, string>>,
): Provider {
  const headers = providerHeaders(definition.id, hostHeaders);
  const baseline = definition.models.map((model) => materializeModel(definition, model, headers));
  let dynamic: readonly Model[] = [];
  let inflight: Promise<void> | undefined;

  return {
    id: definition.id,
    name: definition.name,
    baseUrl: definition.baseUrl,
    auth: {
      apiKey:
        definition.envNames === undefined
          ? undefined
          : envApiKeyAuth(`${definition.name} API key`, definition.envNames),
      oauth: definition.oauth,
    },
    getModels: () => mergeModels(baseline, dynamic),
    refreshModels:
      definition.catalog === undefined
        ? undefined
        : (context) => {
            inflight ??= refreshCatalog(definition, context, headers)
              .then((models) => {
                dynamic = models;
              })
              .finally(() => {
                inflight = undefined;
              });
            return inflight;
          },
    stream: (model, context, auth, options) => streamProvider(model, context, auth, options),
  };
}

async function refreshCatalog(
  definition: ProviderDefinition,
  context: RefreshModelsContext,
  headers: ProviderHeaders,
): Promise<readonly Model[]> {
  const stored = await context.store.read();
  let models = stored?.models.filter((model) => model.provider === definition.id) ?? [];
  if (!context.allowNetwork || context.signal?.aborted || context.auth === undefined) return models;
  if (
    context.force !== true &&
    stored?.checkedAt !== undefined &&
    Date.now() - stored.checkedAt < CATALOG_REFRESH_INTERVAL_MS
  ) {
    return models;
  }

  const response = await fetch(
    `${definition.baseUrl.replace(/\/+$/u, "")}${definition.catalog!.path}`,
    {
      headers: catalogHeaders(context.auth, stored?.etag, headers),
      signal: context.signal,
    },
  );
  const checkedAt = Date.now();
  if (response.status === 304 && stored !== undefined) {
    await context.store.write({ ...stored, checkedAt });
    return models;
  }
  if (!response.ok) {
    await context.store.write({ ...(stored ?? { models }), checkedAt });
    throw new Error(`${definition.name} model catalog failed (HTTP ${response.status})`);
  }

  const parse = definition.catalog?.parse ?? parseOpenAICatalog;
  models = parse(await response.json()).map((model) =>
    materializeModel(definition, model, headers),
  );
  const entry = {
    models,
    checkedAt,
    lastModified: parseDate(response.headers.get("last-modified")),
    etag: response.headers.get("etag") ?? undefined,
  };
  await context.store.write(entry);
  return models;
}

function catalogHeaders(
  auth: AuthResult,
  etag: string | undefined,
  providerHeaders: ProviderHeaders,
): Headers {
  const headers = new Headers({ accept: "application/json" });
  for (const [name, value] of Object.entries(providerHeaders)) {
    if (value === null) headers.delete(name);
    else headers.set(name, value);
  }
  if (auth.auth.apiKey !== undefined) {
    headers.set("authorization", `Bearer ${auth.auth.apiKey}`);
  }
  for (const [name, value] of Object.entries(auth.auth.headers ?? {})) {
    if (value === null) headers.delete(name);
    else headers.set(name, value);
  }
  if (etag !== undefined) headers.set("if-none-match", etag);
  return headers;
}

function parseOpenAICatalog(value: unknown): readonly CatalogModel[] {
  const root = record(value);
  const entries = Array.isArray(value)
    ? value
    : array(root?.["data"]).length > 0
      ? array(root?.["data"])
      : array(root?.["models"]);
  return entries.flatMap((entry) => {
    const item = record(entry);
    const id = string(item?.["id"]) ?? string(item?.["model"]);
    if (id === undefined) return [];
    return [
      {
        id,
        name: string(item?.["display_name"]) ?? string(item?.["name"]),
        contextWindow:
          positiveNumber(item?.["context_window"]) ?? positiveNumber(item?.["context_length"]),
        maxTokens:
          positiveNumber(item?.["max_output_tokens"]) ?? positiveNumber(item?.["max_tokens"]),
        reasoning:
          boolean(item?.["reasoning"]) ??
          boolean(record(item?.["capabilities"])?.["reasoning"]) ??
          inferReasoning(id),
        image:
          boolean(item?.["vision"]) ??
          boolean(record(item?.["capabilities"])?.["vision"]) ??
          inferVision(id),
      },
    ];
  });
}

function parseCodexCatalog(value: unknown): readonly CatalogModel[] {
  const root = record(value);
  return array(root?.["models"]).flatMap((entry) => {
    const item = record(entry);
    const id = string(item?.["slug"]) ?? string(item?.["id"]);
    if (id === undefined) return [];
    return [
      {
        id,
        name: string(item?.["display_name"]) ?? id,
        reasoning: true,
        image: true,
        contextWindow:
          positiveNumber(item?.["context_window"]) ?? positiveNumber(item?.["context_window_size"]),
        maxTokens: positiveNumber(item?.["max_output_tokens"]),
      },
    ];
  });
}

function materializeModel(
  providerDefinition: Pick<ProviderDefinition, "id" | "baseUrl" | "api">,
  definition: ModelDefinition | CatalogModel,
  headers: ProviderHeaders,
): Model {
  return {
    id: definition.id,
    name: definition.name ?? definition.id,
    api: definition.api ?? providerDefinition.api,
    provider: providerDefinition.id,
    baseUrl: providerDefinition.baseUrl,
    reasoning: definition.reasoning ?? inferReasoning(definition.id),
    input: definition.image === true ? ["text", "image"] : ["text"],
    cost: NO_COST,
    contextWindow: definition.contextWindow ?? DEFAULT_CONTEXT_WINDOW,
    maxTokens: definition.maxTokens ?? DEFAULT_MAX_TOKENS,
    headers,
  };
}

function providerHeaders(
  providerId: string,
  hostHeaders: Readonly<Record<string, string>>,
): ProviderHeaders {
  if (providerId === "kimi-coding" || providerId === "moonshot") {
    return { ...hostHeaders };
  }
  const userAgent = Object.entries(hostHeaders).find(
    ([name]) => name.toLowerCase() === "user-agent",
  );
  return userAgent === undefined ? {} : { [userAgent[0]]: userAgent[1] };
}

function mergeModels(baseline: readonly Model[], dynamic: readonly Model[]): readonly Model[] {
  const models = new Map(baseline.map((model) => [model.id, model]));
  for (const model of dynamic) {
    const known = models.get(model.id);
    models.set(model.id, known === undefined ? model : { ...known, ...model });
  }
  return [...models.values()].toSorted((left, right) => left.name.localeCompare(right.name));
}

function inferReasoning(id: string): boolean {
  return /reason|thinking|gpt-5|grok|o[1-9](?:-|$)|r1|qwen3|glm-5|kimi-k2\.5/iu.test(id);
}

function inferVision(id: string): boolean {
  return /vision|vl|gpt-4o|gpt-5|grok|claude|gemini|kimi-k2\.5/iu.test(id);
}

function parseDate(value: string | null): number | undefined {
  if (value === null) return undefined;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? undefined : parsed;
}

function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function array(value: unknown): readonly unknown[] {
  return Array.isArray(value) ? value : [];
}

function string(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function boolean(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function positiveNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : undefined;
}
