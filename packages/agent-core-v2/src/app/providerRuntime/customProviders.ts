/**
 * `providerRuntime` domain (L5) — composes built-ins, models.json overlays,
 * and user-defined providers into one runtime provider contract.
 *
 * Resolves persisted model metadata and request configuration while preserving
 * the built-in provider's authentication, catalog, and stream behaviour when
 * it remains applicable. Bound at App scope through `providerRuntime`.
 */
import { execFileSync } from "node:child_process";

import { z } from "zod";

import { Error2 } from "#/_base/errors/errors";

import { createRadiusOAuth, envApiKeyAuth } from "./auth";
import { ProviderRuntimeErrors } from "./errors";
import { streamProvider, supportsProviderApi } from "./stream";
import type {
  Api,
  ApiKeyAuth,
  AuthCheck,
  AuthResult,
  CustomModelDefinition,
  CustomModelOverride,
  CustomProviderDefinition,
  Model,
  OAuthAuth,
  Provider,
  ProviderHeaders,
} from "./types";

const API_SCHEMA = z.string().min(1);
const HEADER_SCHEMA = z.record(z.string(), z.string().nullable());
const COMPAT_SCHEMA = z.object({
  supportsStore: z.boolean().optional(),
  supportsDeveloperRole: z.boolean().optional(),
  supportsReasoningEffort: z.boolean().optional(),
  supportsUsageInStreaming: z.boolean().optional(),
  maxTokensField: z.enum(["max_completion_tokens", "max_tokens"]).optional(),
  requiresToolResultName: z.boolean().optional(),
  requiresAssistantAfterToolResult: z.boolean().optional(),
  requiresThinkingAsText: z.boolean().optional(),
  requiresReasoningContentOnAssistantMessages: z.boolean().optional(),
  thinkingFormat: z.enum([
    "openai", "openrouter", "deepseek", "together", "zai", "qwen", "chat-template",
    "qwen-chat-template", "string-thinking", "ant-ling",
  ]).optional(),
  chatTemplateKwargs: z.record(z.string(), z.union([
    z.string(), z.number(), z.boolean(), z.null(), z.object({
      $var: z.enum(["thinking.enabled", "thinking.effort"]),
      omitWhenOff: z.boolean().optional(),
    }).strict(),
  ])).optional(),
  openRouterRouting: z.record(z.string(), z.unknown()).optional(),
  vercelGatewayRouting: z.object({
    only: z.array(z.string()).optional(),
    order: z.array(z.string()).optional(),
  }).strict().optional(),
  zaiToolStream: z.boolean().optional(),
  supportsStrictMode: z.boolean().optional(),
  supportsOpenAIGrammarTools: z.boolean().optional(),
  cacheControlFormat: z.literal("anthropic").optional(),
  deferredToolsMode: z.literal("kimi").optional(),
  supportsLongCacheRetention: z.boolean().optional(),
  sendSessionAffinityHeaders: z.boolean().optional(),
  sessionAffinityFormat: z.enum(["openai", "openai-nosession", "openrouter"]).optional(),
  supportsToolSearch: z.boolean().optional(),
  supportsExplicitPromptCacheMode: z.boolean().optional(),
  supportsEagerToolInputStreaming: z.boolean().optional(),
  supportsCacheControlOnTools: z.boolean().optional(),
  supportsTemperature: z.boolean().optional(),
  forceAdaptiveThinking: z.boolean().optional(),
  allowEmptySignature: z.boolean().optional(),
  supportsStrictTools: z.boolean().optional(),
  supportsToolReferences: z.boolean().optional(),
}).strict();
const COST_SCHEMA = z.object({
  input: z.number().min(0),
  output: z.number().min(0),
  cacheRead: z.number().min(0),
  cacheWrite: z.number().min(0),
  tiers: z.array(z.object({
    inputTokensAbove: z.number().min(0),
    input: z.number().min(0),
    output: z.number().min(0),
    cacheRead: z.number().min(0),
    cacheWrite: z.number().min(0),
  }).strict()).optional(),
}).strict();
const COST_OVERRIDE_SCHEMA = z.object({
  input: z.number().min(0).optional(),
  output: z.number().min(0).optional(),
  cacheRead: z.number().min(0).optional(),
  cacheWrite: z.number().min(0).optional(),
  tiers: COST_SCHEMA.shape.tiers,
}).strict();
const MODEL_SCHEMA = z.object({
  id: z.string().min(1),
  name: z.string().min(1).optional(),
  api: API_SCHEMA.optional(),
  // Pi accepts provider-owned endpoints and templates. Adapters such as
  // Cloudflare materialize their final URL from auth at request time, so an
  // eager URL parser would reject valid config before that ownership runs.
  baseUrl: z.string().min(1).optional(),
  reasoning: z.boolean().optional(),
  input: z.array(z.enum(["text", "image"])).min(1).optional(),
  contextWindow: z.number().int().positive().optional(),
  maxTokens: z.number().int().positive().optional(),
  cost: COST_SCHEMA.optional(),
  headers: HEADER_SCHEMA.optional(),
  compat: COMPAT_SCHEMA.optional(),
  thinkingLevelMap: z.record(z.string(), z.union([z.string(), z.number(), z.null()])).optional(),
}).strict();
const MODEL_OVERRIDE_SCHEMA = MODEL_SCHEMA.omit({ id: true }).extend({
  cost: COST_OVERRIDE_SCHEMA.optional(),
}).strict();
const CUSTOM_PROVIDER_SCHEMA = z.object({
  id: z.string().regex(/^[a-z0-9][a-z0-9_-]*$/u),
  name: z.string().min(1).optional(),
  api: API_SCHEMA.optional(),
  baseUrl: z.string().min(1).optional(),
  apiKey: z.string().min(1).optional(),
  oauth: z.literal("radius").optional(),
  authHeader: z.boolean().optional(),
  headers: HEADER_SCHEMA.optional(),
  compat: COMPAT_SCHEMA.optional(),
  models: z.array(MODEL_SCHEMA).optional(),
  modelOverrides: z.record(z.string(), MODEL_OVERRIDE_SCHEMA).optional(),
}).strict();

const COST = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

/** Parses shape only. Validity that depends on a built-in base is checked while composing. */
export function parseCustomProviderDefinition(value: unknown): CustomProviderDefinition {
  const parsed = CUSTOM_PROVIDER_SCHEMA.safeParse(value);
  if (!parsed.success) {
    throw new Error2(
      ProviderRuntimeErrors.codes.PROVIDER_INVALID_DEFINITION,
      `Invalid models.json provider: ${parsed.error.issues.map(formatIssue).join("; ")}`,
    );
  }
  return parsed.data;
}

/** Compose one models.json layer over a built-in or process-owned provider. */
export function composeProvider(
  definition: CustomProviderDefinition,
  base: Provider | undefined,
): Provider {
  const parsed = parseCustomProviderDefinition(definition);
  // Built-ins with remote catalogs recreate their catalog closure at an
  // overlay endpoint.  Calling the inherited refresh function directly would
  // keep its generated endpoint and silently fetch the wrong service.
  const effectiveBase = parsed.baseUrl === undefined
    ? base
    : base?.withBaseUrl?.(parsed.baseUrl) ?? base;
  const getBaseModels = (): readonly Model[] => effectiveBase?.getModels() ?? [];
  const getModels = (): readonly Model[] => composeModels(parsed, getBaseModels(), effectiveBase?.baseUrl);
  const auth = composeAuth(parsed, effectiveBase);
  // Validate eagerly. A malformed entry must never silently turn into a
  // guessed-capability model later in a TUI list.
  getModels();
  if (auth.apiKey === undefined && auth.oauth === undefined) {
    throw invalid(parsed.id, "no authentication method is available");
  }

  return {
    id: parsed.id,
    name: parsed.name ?? effectiveBase?.name ?? parsed.id,
    baseUrl: parsed.baseUrl ?? effectiveBase?.baseUrl,
    headers: mergeHeaders(effectiveBase?.headers, parsed.headers),
    auth,
    getModels,
    filterModels: effectiveBase?.filterModels?.bind(effectiveBase),
    refreshModels: effectiveBase?.refreshModels?.bind(effectiveBase),
    stream: (model, context, resolvedAuth, options) => {
      const baseSupportsApi = effectiveBase?.getModels().some((entry) => entry.api === model.api) ?? false;
      return effectiveBase !== undefined && baseSupportsApi
        ? effectiveBase.stream(model, context, resolvedAuth, options)
        : streamProvider(model, context, resolvedAuth, options);
    },
  };
}

/** Compatibility name retained for callers that construct a new provider. */
export function customProvider(definition: CustomProviderDefinition): Provider {
  return composeProvider(definition, undefined);
}

function composeModels(
  definition: CustomProviderDefinition,
  baseModels: readonly Model[],
  baseUrl: string | undefined,
): readonly Model[] {
  const byId = new Map<string, Model>();
  for (const base of baseModels) byId.set(base.id, applyProvider(definition, base));
  for (const configured of definition.models ?? []) {
    const existing = byId.get(configured.id);
    byId.set(configured.id, materialize(definition, configured, existing, baseModels[0], baseUrl));
  }
  for (const [id, override] of Object.entries(definition.modelOverrides ?? {})) {
    const existing = byId.get(id);
    // Pi treats stale overrides as harmless: a provider may remove a dynamic
    // catalog model while the user still keeps its metadata override.
    if (existing === undefined) continue;
    byId.set(id, materialize(definition, { ...override, id }, existing, baseModels[0], baseUrl));
  }
  if (byId.size === 0) {
    throw invalid(definition.id, "requires models when it does not overlay a built-in provider");
  }
  return [...byId.values()];
}

function applyProvider(definition: CustomProviderDefinition, model: Model): Model {
  // Generated/provider-owned compat may include forward metadata we do not
  // own. Only reject unsupported user models.json fields; never invalidate a
  // built-in while composing an otherwise harmless overlay.
  assertSupportedCompat(definition.id, model.api, definition.compat);
  const compat = mergeRecords(model.compat, definition.compat);
  return {
    ...model,
    baseUrl: definition.baseUrl ?? model.baseUrl,
    headers: mergeHeaders(model.headers, definition.headers),
    compat,
  };
}

function materialize(
  definition: CustomProviderDefinition,
  configured: CustomModelDefinition | (CustomModelOverride & { id: string }),
  existing: Model | undefined,
  fallback: Model | undefined,
  providerBaseUrl: string | undefined,
): Model {
  const baseline = existing === undefined ? fallback : existing;
  const api = configured.api ?? definition.api ?? baseline?.api;
  const baseUrl = configured.baseUrl ?? definition.baseUrl ?? baseline?.baseUrl ?? providerBaseUrl;
  if (api === undefined) throw invalid(definition.id, `model ${configured.id} requires api`);
  if (baseUrl === undefined) throw invalid(definition.id, `model ${configured.id} requires baseUrl`);
  if (!supportsProviderApi(api) && existing?.api !== api) {
    throw invalid(definition.id, `model ${configured.id} uses unimplemented protocol ${api}`);
  }
  // New ids are unknown to the catalog. Never borrow token limits from an
  // unrelated first built-in model, even when that is superficially convenient.
  if (existing === undefined && !hasLimits(configured)) {
    throw invalid(definition.id, `model ${configured.id} requires contextWindow and maxTokens`);
  }
  const source = existing ?? fallback;
  assertSupportedCompat(definition.id, api, definition.compat);
  assertSupportedCompat(definition.id, api, configured.compat);
  const compat = mergeRecords(mergeRecords(source?.compat, definition.compat), configured.compat);
  return {
    id: configured.id,
    name: configured.name ?? source?.name ?? configured.id,
    api,
    provider: definition.id,
    baseUrl,
    reasoning: configured.reasoning ?? source?.reasoning ?? false,
    input: configured.input ?? source?.input ?? ["text"],
    cost: mergeCost(source?.cost, configured.cost),
    contextWindow: configured.contextWindow ?? source?.contextWindow ?? requiredLimit(definition.id, configured.id, "contextWindow"),
    maxTokens: configured.maxTokens ?? source?.maxTokens ?? requiredLimit(definition.id, configured.id, "maxTokens"),
    headers: mergeHeaders(mergeHeaders(source?.headers, definition.headers), configured.headers),
    compat,
    thinkingLevelMap: configured.thinkingLevelMap
      ? { ...source?.thinkingLevelMap, ...configured.thinkingLevelMap }
      : source?.thinkingLevelMap,
  };
}

/** Reject configuration that this runtime cannot serialize; never silently ignore it. */
function assertSupportedCompat(
  providerId: string,
  api: Api,
  compat: Readonly<Record<string, unknown>> | undefined,
): void {
  if (compat === undefined || Object.keys(compat).length === 0) return;
  const supported = api === "openai-completions"
    ? new Set([
      "supportsStore", "supportsDeveloperRole", "supportsReasoningEffort", "supportsUsageInStreaming",
      "maxTokensField", "requiresToolResultName", "requiresAssistantAfterToolResult", "requiresThinkingAsText",
      "requiresReasoningContentOnAssistantMessages", "thinkingFormat", "chatTemplateKwargs", "openRouterRouting",
      "vercelGatewayRouting", "zaiToolStream", "supportsStrictMode", "sendSessionAffinityHeaders", "sessionAffinityFormat",
    ])
    : api === "openai-responses" || api === "azure-openai-responses" || api === "openai-codex-responses"
      ? new Set(["sessionAffinityFormat", "supportsStrictMode"])
      : api === "anthropic-messages"
        ? new Set(["supportsTemperature"])
        : new Set<string>();
  const unsupported = Object.keys(compat).filter((key) => !supported.has(key));
  if (unsupported.length > 0) {
    throw invalid(providerId, `compat.${unsupported.join(", ")} is unsupported for ${api}`);
  }
}

function composeAuth(definition: CustomProviderDefinition, base: Provider | undefined): Provider["auth"] {
  if (definition.oauth === "radius" && definition.baseUrl === undefined) {
    throw invalid(definition.id, 'baseUrl is required when oauth is "radius"');
  }
  const configured = definition.apiKey;
  const needsWrapper = configured !== undefined || definition.headers !== undefined || definition.authHeader !== undefined;
  if (!needsWrapper && base !== undefined) return base.auth;
  const radius = definition.oauth === "radius"
    ? definition.baseUrl === undefined
      ? undefined
      : createRadiusOAuth(definition.baseUrl, definition.name ?? definition.id)
    : undefined;
  const oauth = wrapOAuth(definition, radius ?? base?.auth.oauth);
  // OAuth-only custom providers must not grow a fictitious API-key login.  A
  // configured key intentionally keeps both methods, matching Pi's overlay
  // semantics for gateways that offer OAuth and a personal token.
  const apiKey = wrapApiKey(
    definition,
    configured === undefined && base === undefined && oauth === undefined
      ? envApiKeyAuth(`${definition.name ?? definition.id} API key`, [])
      : base?.auth.apiKey,
  );
  return { apiKey, oauth };
}

function wrapApiKey(
  definition: CustomProviderDefinition,
  inherited: ApiKeyAuth | undefined,
): ApiKeyAuth | undefined {
  const configured = definition.apiKey;
  if (configured === undefined && inherited === undefined) return undefined;
  const base = inherited ?? envApiKeyAuth(`${definition.name ?? definition.id} API key`, []);
  return {
    ...base,
    check: async (input): Promise<AuthCheck | undefined> => {
      if (input.credential !== undefined) {
        const stored = await base.check?.(input);
        if (stored !== undefined) return stored;
      }
      const configured = await configuredCheck(definition, input.ctx);
      if (configured !== undefined) return configured;
      return base.check?.({ ...input, credential: undefined });
    },
    resolve: async (input): Promise<AuthResult | undefined> => {
      const stored = input.credential === undefined ? undefined : await base.resolve(input);
      // Feed a models.json key through the inherited resolver rather than
      // manufacturing `{ apiKey }`: provider-owned authentication may choose
      // non-Bearer headers, a derived base URL, or credential environment.
      const configured = stored === undefined ? await configuredAuth(definition, base, input) : undefined;
      const inherited = stored === undefined && configured === undefined
        ? await base.resolve({ ...input, credential: undefined })
        : undefined;
      const resolved = stored ?? configured ?? inherited;
      if (resolved === undefined) {
        if (definition.authHeader === true) throw invalid(definition.id, "authHeader requires a resolved API key");
        return undefined;
      }
      return withProviderHeaders(definition, resolved, input.ctx);
    },
  };
}

function wrapOAuth(definition: CustomProviderDefinition, inherited: OAuthAuth | undefined): OAuthAuth | undefined {
  if (inherited === undefined) return undefined;
  return {
    ...inherited,
    toAuth: async (credential) => (
      await withProviderHeaders(definition, {
        auth: await inherited.toAuth(credential),
        env: credential.env,
      })
    ).auth,
  };
}

async function withProviderHeaders(
  definition: CustomProviderDefinition,
  result: AuthResult,
  ctx?: { env(name: string): Promise<string | undefined> },
): Promise<AuthResult> {
  const headers = mergeHeaders(result.auth.headers, await resolveHeaders(definition.headers, result.env, ctx));
  if (definition.authHeader === true && result.auth.apiKey === undefined) {
    throw invalid(definition.id, "authHeader requires a resolved API key");
  }
  if (definition.authHeader === true) {
    return { ...result, auth: { ...result.auth, headers: mergeHeaders(headers, { Authorization: `Bearer ${result.auth.apiKey}` }) } };
  }
  return { ...result, auth: { ...result.auth, headers } };
}

async function configuredAuth(
  definition: CustomProviderDefinition,
  inherited: ApiKeyAuth,
  input: Parameters<ApiKeyAuth["resolve"]>[0],
): Promise<AuthResult | undefined> {
  const key = await configuredKey(definition, input.ctx);
  if (key === undefined) return undefined;
  const resolved = await inherited.resolve({
    ...input,
    credential: { type: "api_key", key },
  });
  return resolved === undefined ? undefined : { ...resolved, source: configuredSource(definition) };
}

async function configuredKey(definition: CustomProviderDefinition, ctx: { env(name: string): Promise<string | undefined> }): Promise<string | undefined> {
  const value = definition.apiKey;
  return value === undefined ? undefined : resolveValue(value, undefined, ctx);
}

function configuredSource(definition: CustomProviderDefinition): string {
  if (definition.apiKey?.startsWith("!")) return "models.json command";
  return definition.apiKey?.startsWith("$") === true ? definition.apiKey : "models.json API key";
}

/** Auth discovery must not execute a user command; execution is request-time only. */
async function configuredCheck(
  definition: CustomProviderDefinition,
  ctx: { env(name: string): Promise<string | undefined> },
): Promise<AuthCheck | undefined> {
  if (definition.apiKey === undefined) return undefined;
  if (definition.apiKey.startsWith("!")) return { type: "api_key", source: configuredSource(definition) };
  const key = await configuredKey(definition, ctx);
  return key === undefined ? undefined : { type: "api_key", source: configuredSource(definition) };
}

/** Pi-compatible `$ENV`/`${ENV}` templates and `!command` config values. */
async function resolveValue(
  value: string,
  env: Record<string, string> | undefined,
  ctx?: { env(name: string): Promise<string | undefined> },
): Promise<string | undefined> {
  if (value.startsWith("!")) {
    try {
      return execFileSync(process.env["SHELL"] ?? "/bin/sh", ["-lc", value.slice(1)], {
        encoding: "utf8", timeout: 10_000, stdio: ["ignore", "pipe", "ignore"],
      }).trim() || undefined;
    } catch {
      return undefined;
    }
  }
  let output = "";
  for (let index = 0; index < value.length;) {
    if (value[index] !== "$") { output += value[index++]!; continue; }
    const next = value[index + 1];
    if (next === "$" || next === "!") { output += next; index += 2; continue; }
    const braced = next === "{" ? /^\$\{([A-Za-z_][A-Za-z0-9_]*)\}/u.exec(value.slice(index)) : undefined;
    const plain = braced === undefined ? /^\$([A-Za-z_][A-Za-z0-9_]*)/u.exec(value.slice(index)) : undefined;
    const match = braced ?? plain;
    if (match === null || match === undefined) { output += "$"; index += 1; continue; }
    const name = match[1]!;
    const resolved = env?.[name] ?? await ctx?.env(name) ?? process.env[name];
    if (resolved === undefined) return undefined;
    output += resolved;
    index += match[0].length;
  }
  return output;
}

async function resolveHeaders(
  headers: ProviderHeaders | undefined,
  env: Record<string, string> | undefined,
  ctx?: { env(name: string): Promise<string | undefined> },
): Promise<ProviderHeaders | undefined> {
  if (headers === undefined) return undefined;
  const resolved: ProviderHeaders = {};
  for (const [name, value] of Object.entries(headers)) {
    if (value === null) resolved[name] = null;
    else {
      const actual = await resolveValue(value, env, ctx);
      if (actual === undefined) throw invalid("models.json", `failed to resolve header ${name}`);
      resolved[name] = actual;
    }
  }
  return resolved;
}

/** Resolve a model's persisted request headers at the auth boundary. */
export async function resolveModelHeaders(
  headers: ProviderHeaders | undefined,
  auth: AuthResult,
  ctx: { env(name: string): Promise<string | undefined> },
): Promise<ProviderHeaders | undefined> {
  return resolveHeaders(headers, auth.env, ctx);
}

function mergeHeaders(...sources: (ProviderHeaders | undefined)[]): ProviderHeaders | undefined {
  const result: ProviderHeaders = {};
  for (const source of sources) {
    for (const [name, value] of Object.entries(source ?? {})) {
      for (const existing of Object.keys(result)) if (existing.toLowerCase() === name.toLowerCase()) delete result[existing];
      result[name] = value;
    }
  }
  return Object.keys(result).length === 0 ? undefined : result;
}

function mergeRecords(
  base: Readonly<Record<string, unknown>> | undefined,
  override: Readonly<Record<string, unknown>> | undefined,
): Readonly<Record<string, unknown>> | undefined {
  if (base === undefined && override === undefined) return undefined;
  const result: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(override ?? {})) {
    const prior = result[key];
    result[key] = isRecord(prior) && isRecord(value) ? mergeRecords(prior, value) : value;
  }
  return result;
}

function mergeCost(
  base: Model["cost"] | undefined,
  override: CustomModelDefinition["cost"] | CustomModelOverride["cost"],
): Model["cost"] {
  return { ...COST, ...base, ...override };
}

function requiredLimit(provider: string, model: string, field: string): never {
  throw invalid(provider, `model ${model} requires ${field}`);
}

function hasLimits(
  model: CustomModelDefinition | (CustomModelOverride & { id: string }),
): model is
  | (CustomModelDefinition & { contextWindow: number; maxTokens: number })
  | (CustomModelOverride & { id: string; contextWindow: number; maxTokens: number }) {
  return model.contextWindow !== undefined && model.maxTokens !== undefined;
}

function invalid(providerId: string, message: string): Error2 {
  return new Error2(ProviderRuntimeErrors.codes.PROVIDER_INVALID_DEFINITION, `Provider ${providerId}: ${message}`);
}

function formatIssue(issue: z.core.$ZodIssue): string {
  const path = issue.path.map(String).join(".");
  return path.length === 0 ? issue.message : `${path}: ${issue.message}`;
}

function isRecord(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null && !Array.isArray(value); }
