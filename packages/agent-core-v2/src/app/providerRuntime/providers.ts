import {
  anthropicApiKeyAuth,
  anthropicOAuth,
  bedrockAuth,
  cloudflareAIGatewayAuth,
  cloudflareWorkersAIAuth,
  createRadiusOAuth,
  envApiKeyAuth,
  githubCopilotOAuth,
  googleVertexAuth,
  kimiCodingOAuth,
  openaiCodexOAuth,
  openRouterOAuth,
  xaiOAuth,
} from "./auth";
import { BUILTIN_CATALOG } from "./builtinCatalog.generated";
import { streamProvider, supportsProviderApi } from "./stream";
import type { BuiltinCatalogProvider } from "./builtinCatalog";
import type {
  Api,
  AuthResult,
  Credential,
  Model,
  ModelCost,
  ModelInput,
  Provider,
  ProviderHeaders,
  RefreshModelsContext,
} from "./types";

const CATALOG_REFRESH_INTERVAL_MS = 4 * 60 * 60 * 1_000;
const BUILTIN_CATALOG_GENERATED_AT = Date.parse(BUILTIN_CATALOG.generatedAt);

interface ProviderDefinition {
  id: string;
  name: string;
  baseUrl?: string;
  api: Api;
  envNames?: readonly string[];
  oauth?: Provider["auth"]["oauth"];
  models: readonly ModelDefinition[];
  catalog?: {
    path: string;
    parse?: (value: unknown) => readonly CatalogModel[];
  };
  /** Timestamp of the generated metadata this provider was composed from. */
  metadataGeneratedAt?: number;
}

interface ModelDefinition {
  id: string;
  name?: string;
  api?: Api;
  reasoning?: boolean;
  input?: readonly ModelInput[];
  cost?: ModelCost;
  contextWindow?: number;
  maxTokens?: number;
  dynamicTools?: boolean;
  compat?: Readonly<Record<string, unknown>>;
  thinkingLevelMap?: Readonly<Record<string, string | number | null>>;
  defaultThinkingLevel?: string;
}

interface CatalogModel extends ModelDefinition {
  type?: string;
}

export function builtinProviders(
  hostHeaders: Readonly<Record<string, string>> = {},
): readonly Provider[] {
  return (BUILTIN_CATALOG.providers as readonly BuiltinCatalogProvider[]).flatMap((catalog) => {
    if (catalog.id === "radius") return [radiusProvider(hostHeaders)];
    const definition = builtinDefinition(catalog);
    if (!supportsProviderApi(definition.api)) {
      return [];
    }
    const composed = provider(
      {
        id: definition.id,
        name: definition.name,
        baseUrl: definition.baseUrl,
        api: definition.api,
        envNames: definition.envNames,
        oauth: providerOAuth(catalog.id),
        models: definition.models,
        catalog: catalogEndpoint(catalog.id),
      },
      hostHeaders,
    );
    return [composed];
  });
}

function providerOAuth(providerId: string): Provider["auth"]["oauth"] | undefined {
  if (providerId === "kimi-coding") return kimiCodingOAuth;
  if (providerId === "openai-codex") return openaiCodexOAuth;
  if (providerId === "xai") return xaiOAuth;
  if (providerId === "anthropic") return anthropicOAuth;
  if (providerId === "github-copilot") return githubCopilotOAuth;
  if (providerId === "openrouter") return openRouterOAuth;
  if (providerId === "radius") return createRadiusOAuth();
  return undefined;
}

function providerApiKeyAuth(definition: ProviderDefinition): Provider["auth"]["apiKey"] | undefined {
  if (definition.id === "anthropic") return anthropicApiKeyAuth();
  if (definition.id === "amazon-bedrock") return bedrockAuth;
  if (definition.id === "cloudflare-workers-ai") return cloudflareWorkersAIAuth();
  if (definition.id === "cloudflare-ai-gateway") return cloudflareAIGatewayAuth();
  if (definition.id === "google-vertex") return googleVertexAuth;
  if (definition.id === "github-copilot") return envApiKeyAuth("GitHub Copilot token", ["COPILOT_GITHUB_TOKEN"]);
  return definition.envNames === undefined ? undefined : envApiKeyAuth(`${definition.name} API key`, definition.envNames);
}

/** Transport/auth corrections are runtime facts; generated metadata remains data-only. */
function builtinDefinition(catalog: BuiltinCatalogProvider): ProviderDefinition {
  if (catalog.id === "amazon-bedrock") return { ...catalog, baseUrl: "https://bedrock-runtime.us-east-1.amazonaws.com" };
  // AI Gateway's endpoint is account/gateway-specific and is materialized by
  // its auth resolver.  A neutral base keeps models.json overlays usable.
  if (catalog.id === "cloudflare-ai-gateway") return { ...catalog, baseUrl: "https://gateway.ai.cloudflare.com/v1" };
  return catalog;
}

/** Radius catalog is gateway-owned: its OAuth/API-key login determines the live model set. */
function radiusProvider(hostHeaders: Readonly<Record<string, string>>): Provider {
  const id = "radius";
  const gateway = "https://radius.pi.dev";
  let models: readonly Model[] = [];
  let inflight: Promise<void> | undefined;
  return {
    id,
    name: "Radius",
    baseUrl: gateway,
    auth: { apiKey: envApiKeyAuth("Radius API key", ["RADIUS_API_KEY"]), oauth: createRadiusOAuth(gateway) },
    getModels: () => models,
    refreshModels: (context) => {
      inflight ??= refreshRadiusCatalog(context, gateway, hostHeaders, (next) => { models = next; }).finally(() => { inflight = undefined; });
      return inflight;
    },
    stream: (model, context, auth, options) => streamProvider(model, context, auth, options),
  };
}

async function refreshRadiusCatalog(
  context: RefreshModelsContext,
  gateway: string,
  hostHeaders: Readonly<Record<string, string>>,
  update: (models: readonly Model[]) => void,
): Promise<void> {
  const stored = await context.store.read();
  if (stored !== undefined) update(stored.models.filter((model) => model.provider === "radius"));
  if (!context.allowNetwork || context.signal?.aborted || context.auth === undefined) return;
  const response = await fetch(`${gateway}/v1/config`, {
    headers: catalogHeaders(context.auth, undefined, providerHeaders("radius", hostHeaders)),
    signal: context.signal,
  });
  if (!response.ok) throw new Error(`Radius model catalog failed (HTTP ${response.status})`);
  const models = parseRadiusCatalog(await response.json());
  update(models);
  await context.store.write({ models, checkedAt: Date.now() });
}

function parseRadiusCatalog(value: unknown): readonly Model[] {
  const root = record(value);
  const baseUrl = string(root?.["baseUrl"]);
  const entries = array(root?.["models"]);
  if (baseUrl === undefined || entries.length === 0) {
    if (entries.length === 0 && baseUrl !== undefined) return [];
    throw new Error("Radius model catalog is invalid");
  }
  return entries.map((entry) => {
    const model = record(entry);
    const id = string(model?.["id"]);
    const name = string(model?.["name"]);
    const reasoning = boolean(model?.["reasoning"]);
    const input = array(model?.["input"]).flatMap((value): ModelInput[] =>
      value === "text" || value === "image" ? [value] : [],
    );
    const cost = radiusCost(record(model?.["cost"]));
    const contextWindow = positiveNumber(model?.["contextWindow"]);
    const maxTokens = positiveNumber(model?.["maxTokens"]);
    if (id === undefined || name === undefined || reasoning === undefined || input.length === 0 || cost === undefined || contextWindow === undefined || maxTokens === undefined) {
      throw new Error("Radius model catalog contains incomplete metadata");
    }
    return {
      id,
      name,
      api: "pi-messages" as const,
      provider: "radius",
      baseUrl,
      reasoning,
      input,
      cost,
      contextWindow,
      maxTokens,
      dynamicTools: boolean(model?.["dynamicTools"]),
      thinkingLevelMap: radiusThinkingLevels(record(model?.["thinkingLevelMap"])),
      defaultThinkingLevel: string(model?.["defaultThinkingLevel"]),
    };
  });
}

function radiusCost(value: Record<string, unknown> | undefined): ModelCost | undefined {
  const input = positiveOrZero(value?.["input"]);
  const output = positiveOrZero(value?.["output"]);
  const cacheRead = positiveOrZero(value?.["cacheRead"]);
  const cacheWrite = positiveOrZero(value?.["cacheWrite"]);
  return input === undefined || output === undefined || cacheRead === undefined || cacheWrite === undefined ? undefined : { input, output, cacheRead, cacheWrite };
}

function radiusThinkingLevels(value: Record<string, unknown> | undefined): Readonly<Record<string, string | number | null>> | undefined {
  if (value === undefined) return undefined;
  return Object.fromEntries(Object.entries(value).filter((entry): entry is [string, string | number | null] => typeof entry[1] === "string" || typeof entry[1] === "number" || entry[1] === null));
}

function catalogEndpoint(providerId: string): ProviderDefinition["catalog"] | undefined {
  if (providerId === "openai-codex") return { path: "/codex/models", parse: parseCodexCatalog };
  if (
    providerId === "openai" ||
    providerId === "xai" ||
    providerId === "deepseek" ||
    providerId === "kimi-coding" ||
    providerId === "moonshotai" ||
    providerId === "moonshotai-cn"
  ) {
    return { path: "/models" };
  }
  return undefined;
}

function provider(
  definition: ProviderDefinition,
  hostHeaders: Readonly<Record<string, string>>,
): Provider {
  const headers = providerHeaders(definition.id, hostHeaders);
  const baseline = definition.models.flatMap((model) => {
    const materialized = materializeModel(definition, model, headers);
    return materialized === undefined ? [] : [materialized];
  });
  let dynamic: readonly Model[] | undefined;
  let inflight: Promise<void> | undefined;

  return {
    id: definition.id,
    name: definition.name,
    baseUrl: definition.baseUrl,
    auth: {
      apiKey: providerApiKeyAuth(definition),
      oauth: definition.oauth,
    },
    getModels: () => (dynamic ?? baseline).filter((model) => supportsProviderApi(model.api)),
    filterModels:
      definition.id === "github-copilot"
        ? (models, credential) => filterCopilotModels(models, credential)
        : undefined,
    withBaseUrl: (baseUrl) => provider({ ...definition, baseUrl }, hostHeaders),
    refreshModels:
      definition.catalog === undefined
        ? undefined
        : (context) => {
            inflight ??= refreshCatalog(definition, context, headers)
              .then((models) => {
                if (models !== undefined) dynamic = models;
              })
              .finally(() => {
                inflight = undefined;
              });
            return inflight;
          },
    stream: (model, context, auth, options) => streamProvider(model, context, auth, options),
  };
}

function filterCopilotModels(models: readonly Model[], credential: Credential | undefined): readonly Model[] {
  if (credential?.type !== "oauth") return models;
  const ids = credential["availableModelIds"];
  if (!Array.isArray(ids) || !ids.every((id) => typeof id === "string")) return models;
  const available = new Set(ids);
  return models.filter((model) => available.has(model.id));
}

async function refreshCatalog(
  definition: ProviderDefinition,
  context: RefreshModelsContext,
  headers: ProviderHeaders,
): Promise<readonly Model[] | undefined> {
  const stored = await context.store.read();
  const generatedAt = definition.metadataGeneratedAt ?? BUILTIN_CATALOG_GENERATED_AT;
  const cached =
    stored?.lastModified !== undefined && stored.lastModified >= generatedAt ? stored : undefined;
  let models = cached?.models.filter((model) => model.provider === definition.id);
  if (!context.allowNetwork || context.signal?.aborted || context.auth === undefined) return models;
  if (definition.baseUrl === undefined) return models;
  if (
    context.force !== true &&
    cached?.checkedAt !== undefined &&
    Date.now() - cached.checkedAt < CATALOG_REFRESH_INTERVAL_MS
  ) {
    return models;
  }

  const response = await fetch(
    `${definition.baseUrl.replace(/\/+$/u, "")}${definition.catalog!.path}`,
    {
      headers: catalogHeaders(context.auth, cached?.etag, headers),
      signal: context.signal,
    },
  );
  const checkedAt = Date.now();
  if (response.status === 304 && cached !== undefined) {
    await context.store.write({ ...cached, checkedAt });
    return models;
  }
  if (!response.ok) {
    if (cached !== undefined) await context.store.write({ ...cached, checkedAt });
    throw new Error(`${definition.name} model catalog failed (HTTP ${response.status})`);
  }

  const parse = definition.catalog?.parse ?? parseOpenAICatalog;
  models = parse(await response.json()).flatMap((model) => {
    const materialized = materializeModel(
      definition,
      model,
      headers,
      definition.models.find((baseline) => baseline.id === model.id),
    );
    return materialized === undefined ? [] : [materialized];
  });
  const entry = {
    models,
    checkedAt,
    lastModified: parseDate(response.headers.get("last-modified")) ?? checkedAt,
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
    if (id === undefined || !isChatCatalogModel(id, item)) return [];
    const capabilities = record(item?.["capabilities"]);
    return [
      {
        id,
        name: string(item?.["display_name"]) ?? string(item?.["name"]),
        contextWindow:
          positiveNumber(item?.["context_window"]) ?? positiveNumber(item?.["context_length"]),
        maxTokens:
          positiveNumber(item?.["max_output_tokens"]) ?? positiveNumber(item?.["max_tokens"]),
        reasoning: boolean(item?.["reasoning"]) ?? boolean(capabilities?.["reasoning"]),
        input: catalogInput(item, capabilities),
        cost: catalogCost(item),
        dynamicTools:
          boolean(item?.["dynamic_tools"]) ?? boolean(capabilities?.["dynamic_tools"]),
        thinkingLevelMap: thinkingLevels(item, capabilities),
        defaultThinkingLevel:
          string(item?.["default_reasoning_effort"]) ??
          string(capabilities?.["default_reasoning_effort"]),
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
        input: ["text", "image"],
        contextWindow:
          positiveNumber(item?.["context_window"]) ?? positiveNumber(item?.["context_window_size"]),
        maxTokens: positiveNumber(item?.["max_output_tokens"]),
        cost: catalogCost(item),
      },
    ];
  });
}

function catalogCost(item: Record<string, unknown> | undefined): ModelCost | undefined {
  const pricing = record(item?.["pricing"]) ?? record(item?.["cost"]);
  if (pricing === undefined) return undefined;
  const input = nonNegativeNumber(pricing["input"]);
  const output = nonNegativeNumber(pricing["output"]);
  if (input === undefined || output === undefined) return undefined;
  return {
    input,
    output,
    cacheRead: nonNegativeNumber(pricing["cache_read"]) ?? nonNegativeNumber(pricing["cacheRead"]) ?? 0,
    cacheWrite:
      nonNegativeNumber(pricing["cache_write"]) ?? nonNegativeNumber(pricing["cacheWrite"]) ?? 0,
  };
}

function materializeModel(
  providerDefinition: Pick<ProviderDefinition, "id" | "baseUrl" | "api">,
  definition: ModelDefinition | CatalogModel,
  headers: ProviderHeaders,
  fallback?: ModelDefinition,
): Model | undefined {
  const reasoning = definition.reasoning ?? fallback?.reasoning;
  const input = definition.input ?? fallback?.input;
  const cost = definition.cost ?? fallback?.cost;
  const contextWindow = definition.contextWindow ?? fallback?.contextWindow;
  const maxTokens = definition.maxTokens ?? fallback?.maxTokens;
  if (
    reasoning === undefined ||
    input === undefined ||
    cost === undefined ||
    contextWindow === undefined ||
    maxTokens === undefined ||
    providerDefinition.baseUrl === undefined
  ) {
    return undefined;
  }
  return {
    id: definition.id,
    name: definition.name ?? fallback?.name ?? definition.id,
    api: definition.api ?? fallback?.api ?? providerDefinition.api,
    provider: providerDefinition.id,
    baseUrl: providerDefinition.baseUrl,
    reasoning,
    input,
    cost,
    contextWindow,
    maxTokens,
    dynamicTools: definition.dynamicTools ?? fallback?.dynamicTools,
    headers,
    compat: definition.compat ?? fallback?.compat,
    thinkingLevelMap: definition.thinkingLevelMap ?? fallback?.thinkingLevelMap,
    defaultThinkingLevel: definition.defaultThinkingLevel ?? fallback?.defaultThinkingLevel,
  };
}

function providerHeaders(
  providerId: string,
  hostHeaders: Readonly<Record<string, string>>,
): ProviderHeaders {
  if (
    providerId === "kimi-coding" ||
    providerId === "moonshotai" ||
    providerId === "moonshotai-cn"
  ) {
    return { ...hostHeaders };
  }
  const userAgent = Object.entries(hostHeaders).find(
    ([name]) => name.toLowerCase() === "user-agent",
  );
  return userAgent === undefined ? {} : { [userAgent[0]]: userAgent[1] };
}

function isChatCatalogModel(id: string, item: Record<string, unknown> | undefined): boolean {
  const capabilities = record(item?.["capabilities"]);
  const kind = [item?.["type"], item?.["model_type"], item?.["object"]]
    .map((value) => string(value)?.toLowerCase())
    .find((value) => value !== undefined);
  if (
    kind !== undefined &&
    /embedding|image|audio|speech|transcription|moderation|rerank|video/iu.test(kind)
  ) {
    return false;
  }
  if (boolean(capabilities?.["chat"]) === false || boolean(capabilities?.["text_generation"]) === false) {
    return false;
  }
  const endpoints = array(item?.["supported_endpoint_types"])
    .concat(array(capabilities?.["supported_endpoint_types"]))
    .flatMap((value) => (typeof value === "string" ? [value] : []));
  if (
    endpoints.length > 0 &&
    !endpoints.some((endpoint) => /chat|completions|responses|messages/iu.test(endpoint))
  ) {
    return false;
  }
  return !/^(?:text-)?embedding|^(?:dall-e|tts|whisper|moderation|omni-moderation)/iu.test(id);
}

function catalogInput(
  item: Record<string, unknown> | undefined,
  capabilities: Record<string, unknown> | undefined,
): readonly ModelInput[] | undefined {
  const values = [
    ...array(item?.["input_modalities"]),
    ...array(item?.["modalities"]),
    ...array(capabilities?.["input"]),
  ]
    .flatMap((value) => (typeof value === "string" ? [value] : []))
    .map((value) => value.toLowerCase())
    .flatMap((value): ModelInput[] => {
      if (value === "text" || value === "image") {
        return [value];
      }
      return [];
    });
  if (boolean(item?.["vision"]) === true || boolean(capabilities?.["vision"]) === true) values.push("image");
  if (values.length === 0) return undefined;
  const input: ModelInput[] = values.includes("text") ? values : ["text", ...values];
  return [...new Set(input)];
}

function thinkingLevels(
  item: Record<string, unknown> | undefined,
  capabilities: Record<string, unknown> | undefined,
): Readonly<Record<string, string | number | null>> | undefined {
  const values = array(item?.["reasoning_efforts"])
    .concat(array(capabilities?.["reasoning_efforts"]))
    .flatMap((value) => (typeof value === "string" || typeof value === "number" ? [value] : []));
  if (values.length === 0) return undefined;
  return Object.fromEntries(values.map((value) => [String(value), value]));
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

function positiveOrZero(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
}

function nonNegativeNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
}
