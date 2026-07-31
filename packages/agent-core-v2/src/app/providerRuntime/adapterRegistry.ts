/**
 * `providerRuntime` domain (L2) — protocol adapter registry.
 *
 * Owns the closed mapping between declared provider APIs and executable stream
 * adapters so catalog/provider construction can reject unsupported protocols.
 */
import type {
  AssistantMessageEvent,
  AuthResult,
  Context,
  KnownApi,
  Model,
  ModelsSimpleStreamOptions,
} from "./types";

export type StreamAdapter = (
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
) => AsyncIterable<AssistantMessageEvent>;

export interface AdapterRegistry {
  supports(api: string): api is KnownApi;
  stream(
    model: Model,
    context: Context,
    auth: AuthResult,
    options?: ModelsSimpleStreamOptions,
  ): AsyncIterable<AssistantMessageEvent> | undefined;
  apis(): readonly KnownApi[];
}

export function createAdapterRegistry(
  adapters: Readonly<Record<KnownApi, StreamAdapter>>,
): AdapterRegistry {
  const entries = Object.entries(adapters) as [KnownApi, StreamAdapter][];
  const byApi = new Map(entries);
  return {
    supports: (api): api is KnownApi => byApi.has(api as KnownApi),
    stream: (model, context, auth, options) => byApi.get(model.api as KnownApi)?.(model, context, auth, options),
    apis: () => [...byApi.keys()],
  };
}
