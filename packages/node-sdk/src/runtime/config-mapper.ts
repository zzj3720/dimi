/**
 * Pure functions that project the runtime's per-domain config view
 * (`IConfigService.getAll()` / `inspect().userValue` / `diagnostics()`) onto
 * the public SDK `DimiConfig` / `ConfigDiagnostics` shapes.
 *
 * The runtime registers one config section per owning domain, while the SDK
 * exposes a single document. Their top-level names line up, so the read
 * mapping is a field pick rather than a reshape.
 */
import type { ConfigDiagnostics, DimiConfig } from "#/types";

/**
 * Every public `DimiConfig` field except the internal `raw` write-path field.
 * Each entry is also the runtime config domain name.
 */
const DIMI_CONFIG_DOMAINS = [
  "defaultProvider",
  "defaultModel",
  "thinking",
  "modelEfforts",
  "planMode",
  "yolo",
  "defaultPermissionMode",
  "defaultPlanMode",
  "permission",
  "hooks",
  "services",
  "mergeAllAvailableSkills",
  "extraSkillDirs",
  "loopControl",
  "background",
  "subagent",
  "secondaryModel",
  "mcp",
  "image",
  "experimental",
  "telemetry",
] as const;

/**
 * Map the runtime's resolved config to the public SDK config
 * (`config.getAll()` — the effective view: file values plus env overlays
 * plus registered section defaults). Runtime-only domains
 * (`cron`, `tools`, `secondaryModel`, `extraAgentDirs`, ...) are dropped,
 * are intentionally omitted from the public SDK document.
 */
export function resolvedConfigToDimiConfig(resolved: Record<string, unknown>): DimiConfig {
  const config: Record<string, unknown> = {};
  for (const domain of DIMI_CONFIG_DOMAINS) {
    const value = resolved[domain];
    if (value !== undefined) {
      config[domain] = value;
    }
  }
  return config as DimiConfig;
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
