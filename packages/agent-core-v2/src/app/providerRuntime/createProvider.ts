import type {
  Api,
  AuthResult,
  Context,
  Credential,
  Model,
  ModelsSimpleStreamOptions,
  Provider,
  RefreshModelsContext,
} from "./types";

export interface CreateProviderOptions<TApi extends Api = Api> {
  id: string;
  name?: string;
  baseUrl: string;
  auth: Provider["auth"];
  models: readonly Model<TApi>[];
  fetchModels?: (context: RefreshModelsContext) => Promise<readonly Model<TApi>[]>;
  filterModels?: (
    models: readonly Model<TApi>[],
    credential: Credential | undefined,
  ) => readonly Model<TApi>[];
  stream(
    model: Model<TApi>,
    context: Context,
    auth: AuthResult,
    options?: ModelsSimpleStreamOptions,
  ): AsyncIterable<import("./types").AssistantMessageEvent>;
}

/**
 * Builds a runtime provider from project-owned contracts. Static models remain
 * available before the first refresh; a dynamic model with the same id
 * replaces its baseline entry.
 */
export function createProvider<TApi extends Api = Api>(
  input: CreateProviderOptions<TApi>,
): Provider {
  const baseline = input.models;
  let dynamic: readonly Model<TApi>[] = [];
  let inflight: Promise<void> | undefined;

  const getModels = (): readonly Model<TApi>[] => {
    const models = new Map(baseline.map((model) => [model.id, model]));
    for (const model of dynamic) models.set(model.id, model);
    return [...models.values()];
  };

  return {
    id: input.id,
    name: input.name ?? input.id,
    baseUrl: input.baseUrl,
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
    stream: (model, context, auth, options) =>
      input.stream(model as Model<TApi>, context, auth, options),
  };
}

export function hasApi<TApi extends Api>(model: Model, api: TApi): model is Model<TApi> {
  return model.api === api;
}

async function refreshDynamicModels<TApi extends Api>(
  input: CreateProviderOptions<TApi>,
  context: RefreshModelsContext,
  update: (models: readonly Model<TApi>[]) => void,
): Promise<void> {
  const stored = await context.store.read();
  const models = (stored?.models ?? []).filter(
    (model): model is Model<TApi> => model.provider === input.id,
  );
  update(models);
  if (!context.allowNetwork || context.signal?.aborted) return;

  const refreshed = await input.fetchModels!(context);
  if (context.signal?.aborted) return;
  update(refreshed);
  await context.store.write({ models: refreshed, checkedAt: Date.now() });
}
