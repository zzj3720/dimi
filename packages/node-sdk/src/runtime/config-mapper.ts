/**
 * Pure functions that project the runtime's per-domain config view
 * (`IConfigService.getAll()` / `inspect().userValue` / `diagnostics()`) onto
 * the public SDK `KimiConfig` / `ConfigDiagnostics` shapes.
 *
 * The runtime registers one config section per owning domain, while the SDK
 * exposes a single document. Their top-level names line up, so the read
 * mapping is a field pick rather than a reshape.
 */
import type { ConfigDiagnostics, KimiConfig } from '#/types';

/**
 * Every public `KimiConfig` field except the internal `raw` write-path field.
 * Each entry is also the runtime config domain name.
 */
const KIMI_CONFIG_DOMAINS = [
  'providers',
  'defaultProvider',
  'defaultModel',
  'models',
  'thinking',
  'planMode',
  'yolo',
  'defaultPermissionMode',
  'defaultPlanMode',
  'permission',
  'hooks',
  'services',
  'mergeAllAvailableSkills',
  'extraSkillDirs',
  'loopControl',
  'background',
  'subagent',
  'mcp',
  'image',
  'modelCatalog',
  'experimental',
  'telemetry',
] as const;

/**
 * Map the runtime's resolved config to the public SDK config
 * (`config.getAll()` — the effective view: file values plus env overlays
 * plus registered section defaults). Runtime-only domains
 * (`cron`, `tools`, `secondaryModel`, `extraAgentDirs`, ...) are dropped,
 * are intentionally omitted from the public SDK document.
 */
export function resolvedConfigToKimiConfig(resolved: Record<string, unknown>): KimiConfig {
  const config: Record<string, unknown> = {};
  for (const domain of KIMI_CONFIG_DOMAINS) {
    const value = resolved[domain];
    if (value !== undefined) {
      config[domain] = value;
    }
  }
  return config as KimiConfig;
}

/** Structural minimum of the runtime's `ConfigDiagnostic`. */
export interface RuntimeConfigDiagnostic {
  readonly domain?: string;
  readonly severity: string;
  readonly message: string;
}

/**
 * The runtime carries structured `{domain, severity, message}` entries while
 * the public SDK exposes warning strings. Native klient callers retain the
 * structured view.
 */
export function diagnosticsToConfigDiagnostics(
  diagnostics: readonly RuntimeConfigDiagnostic[],
): ConfigDiagnostics {
  return { warnings: diagnostics.map((diagnostic) => diagnostic.message) };
}

/** The writes required by the public `removeProvider` contract. */
export interface ProviderRemovalPlan {
  readonly providers: Record<string, unknown>;
  readonly models: Record<string, unknown>;
  readonly clearDefaultModel: boolean;
  readonly clearDefaultProvider: boolean;
}

/**
 * Compute the provider-removal cascade: drop the provider entry,
 * drop every model whose `provider` points at it, and clear the default
 * pointers when they dangle. Inputs are the user-layer values returned by
 * `inspect().userValue`.
 */
export function planProviderRemoval(input: {
  readonly providers: Record<string, unknown> | undefined;
  readonly models: Record<string, Record<string, unknown>> | undefined;
  readonly defaultModel: string | undefined;
  readonly defaultProvider: string | undefined;
  readonly providerId: string;
}): ProviderRemovalPlan {
  const providers = { ...input.providers };
  delete providers[input.providerId];

  const models: Record<string, unknown> = {};
  let removedDefault = false;
  for (const [key, model] of Object.entries(input.models ?? {})) {
    if (model['provider'] === input.providerId) {
      if (input.defaultModel === key) removedDefault = true;
      continue;
    }
    models[key] = model;
  }

  return {
    providers,
    models,
    clearDefaultModel: removedDefault,
    clearDefaultProvider: input.defaultProvider === input.providerId,
  };
}
