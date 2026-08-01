/**
 * `providerRuntime` domain (L2) — generated builtin provider-model metadata.
 *
 * Defines the data-only catalog shape and lookup helpers consumed by provider
 * factories. The generated snapshot owns model capability and pricing facts;
 * provider factories own auth and transport wiring.
 */
import type { Api, ModelCost, ModelInput } from "./types";

export interface BuiltinCatalogModel {
  readonly id: string;
  readonly name: string;
  readonly api?: Api;
  readonly reasoning: boolean;
  readonly input: readonly ModelInput[];
  readonly cost: ModelCost;
  readonly contextWindow: number;
  readonly maxTokens: number;
  readonly dynamicTools?: boolean;
  readonly compat?: Readonly<Record<string, unknown>>;
  readonly thinkingLevelMap?: Readonly<Record<string, string | number | null>>;
  readonly defaultThinkingLevel?: string;
}

export interface BuiltinCatalogProvider {
  readonly id: string;
  readonly source: string;
  readonly name: string;
  readonly api: Api;
  readonly baseUrl?: string;
  readonly envNames: readonly string[];
  readonly models: readonly BuiltinCatalogModel[];
}

export interface BuiltinCatalogSnapshot {
  readonly generatedAt: string;
  readonly providers: readonly BuiltinCatalogProvider[];
}

export function catalogProvider(
  snapshot: BuiltinCatalogSnapshot,
  id: string,
): BuiltinCatalogProvider | undefined {
  return snapshot.providers.find((provider) => provider.id === id);
}
