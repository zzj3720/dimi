import type {
  Api,
  Credential,
  Model,
  Provider,
  ProviderHeaders,
  ProviderStreams,
  RefreshModelsContext,
} from "./types";

export interface CreateProviderOptions<TApi extends Api = Api> {
  id: string;
  name?: string;
  baseUrl?: string;
  headers?: ProviderHeaders;
  auth: Provider["auth"];
  models: readonly Model<TApi>[];
  fetchModels?: (context: RefreshModelsContext) => Promise<readonly Model<TApi>[]>;
  filterModels?: (
    models: readonly Model<TApi>[],
    credential: Credential | undefined,
  ) => readonly Model<TApi>[];
  /** One protocol implementation, or an API-keyed map for mixed providers. */
  api: ProviderStreams<TApi> | Partial<Record<TApi, ProviderStreams<TApi>>>;
}

/**
 * Builds a runtime provider from project-owned contracts. Static models remain
 * available before the first refresh. Once a dynamic source has supplied a
 * catalog snapshot, that snapshot is authoritative, including an empty list.
 */
export function createProvider<TApi extends Api = Api>(
  input: CreateProviderOptions<TApi>,
): Provider {
  const baseline = input.models;
  let dynamic: readonly Model<TApi>[] | undefined;
  let inflight: Promise<void> | undefined;

  const getModels = (): readonly Model<TApi>[] => dynamic ?? baseline;

  const single = isProviderStreams(input.api) ? input.api : undefined;
  const byApi: Partial<Record<TApi, ProviderStreams<TApi>>> | undefined =
    single === undefined ? (input.api as Partial<Record<TApi, ProviderStreams<TApi>>>) : undefined;

  return {
    id: input.id,
    name: input.name ?? input.id,
    baseUrl: input.baseUrl,
    headers: input.headers,
    auth: input.auth,
    getModels,
    filterModels: input.filterModels,
    refreshModels:
      input.fetchModels === undefined
        ? undefined
        : (context) => {
            inflight ??= refreshDynamicModels(input, context, (models) => {
              dynamic = models;
            }).finally(() => {
              inflight = undefined;
            });
            return inflight;
          },
    stream: (model, context, auth, options) => {
      const streams = single ?? byApi?.[model.api as TApi];
      return streams?.stream(model as Model<TApi>, context, auth, options) ??
        missingApiStream(model, input.id);
    },
  };
}

function isProviderStreams<TApi extends Api>(
  value: ProviderStreams<TApi> | Partial<Record<TApi, ProviderStreams<TApi>>>,
): value is ProviderStreams<TApi> {
  return typeof (value as ProviderStreams<TApi>).stream === "function";
}

async function* missingApiStream(
  model: Model,
  providerId: string,
): AsyncIterable<import("./types").AssistantMessageEvent> {
  const output = {
    role: "assistant" as const,
    content: [],
    api: model.api,
    provider: model.provider,
    model: model.id,
    usage: {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 0,
      totalTokens: 0,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
    },
    stopReason: "error" as const,
    errorMessage: `Provider ${providerId} has no API implementation for ${model.api}`,
    timestamp: Date.now(),
  };
  yield { type: "error", reason: "error", error: output };
}

export function hasApi<TApi extends Api>(model: Model, api: TApi): model is Model<TApi> {
  return model.api === api;
}

async function refreshDynamicModels<TApi extends Api>(
  input: CreateProviderOptions<TApi>,
  context: RefreshModelsContext,
  update: (models: readonly Model<TApi>[] | undefined) => void,
): Promise<void> {
  const stored = await context.store.read();
  const models = stored?.models.filter(
    (model): model is Model<TApi> => model.provider === input.id,
  );
  if (models !== undefined) update(models);
  if (!context.allowNetwork || context.signal?.aborted) return;

  const refreshed = await input.fetchModels!(context);
  if (context.signal?.aborted) return;
  update(refreshed);
  await context.store.write({ models: refreshed, checkedAt: Date.now() });
}
