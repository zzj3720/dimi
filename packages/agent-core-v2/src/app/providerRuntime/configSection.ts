import { z } from "zod";

import { type ConfigStripEnv, envBindings, stripEnvBoundFields } from "#/app/config/config";
import { registerConfigSection } from "#/app/config/configSectionContributions";

export const DEFAULT_PROVIDER_SECTION = "defaultProvider";
export const DEFAULT_MODEL_SECTION = "defaultModel";
export const THINKING_SECTION = "thinking";
export const SECONDARY_MODEL_SECTION = "secondaryModel";
export const MODEL_CATALOG_SECTION = "modelCatalog";
export const MODEL_OVERRIDES_SECTION = "modelOverrides";

export const DEFAULT_PROVIDER_ENV = "KIMI_MODEL_PROVIDER";
export const DEFAULT_MODEL_ENV = "KIMI_MODEL_NAME";
export const SECONDARY_MODEL_ENV = "KIMI_SECONDARY_MODEL";
export const SECONDARY_MODEL_PROVIDER_ENV = "KIMI_SECONDARY_PROVIDER";
export const SECONDARY_MODEL_EFFORT_ENV = "KIMI_SECONDARY_EFFORT";

const nonEmpty = (raw: string): string | undefined => {
  const value = raw.trim();
  return value.length > 0 ? value : undefined;
};

const OptionalNameSchema = z.string().min(1).optional();

registerConfigSection(DEFAULT_PROVIDER_SECTION, OptionalNameSchema, {
  env: DEFAULT_PROVIDER_ENV,
  stripEnv: stripScalarEnv(DEFAULT_PROVIDER_ENV),
});

registerConfigSection(DEFAULT_MODEL_SECTION, OptionalNameSchema, {
  env: DEFAULT_MODEL_ENV,
  stripEnv: stripScalarEnv(DEFAULT_MODEL_ENV),
});

export const ThinkingConfigSchema = z.object({
  enabled: z.boolean().optional(),
  effort: z.string().optional(),
  forcedEffort: z.string().optional(),
  keep: z.string().optional(),
});
export type ThinkingConfig = z.infer<typeof ThinkingConfigSchema>;

const thinkingEnvBindings = envBindings(ThinkingConfigSchema, {
  forcedEffort: "KIMI_MODEL_THINKING_EFFORT",
});
const stripThinkingEnv: ConfigStripEnv<ThinkingConfig> = (value) => {
  const result = { ...value };
  delete result.forcedEffort;
  return result;
};

registerConfigSection(THINKING_SECTION, ThinkingConfigSchema, {
  env: thinkingEnvBindings,
  stripEnv: stripThinkingEnv,
});

export const SecondaryModelConfigSchema = z.object({
  provider: z.string().min(1).optional(),
  model: z.string().min(1).optional(),
  defaultEffort: z.string().min(1).optional(),
});
export type SecondaryModelConfig = z.infer<typeof SecondaryModelConfigSchema>;

const secondaryEnvBindings = envBindings(SecondaryModelConfigSchema, {
  provider: { env: SECONDARY_MODEL_PROVIDER_ENV, parse: nonEmpty },
  model: { env: SECONDARY_MODEL_ENV, parse: nonEmpty },
  defaultEffort: { env: SECONDARY_MODEL_EFFORT_ENV, parse: nonEmpty },
});

registerConfigSection(SECONDARY_MODEL_SECTION, SecondaryModelConfigSchema, {
  env: secondaryEnvBindings,
  stripEnv: stripEnvBoundFields(secondaryEnvBindings),
});

export const ModelCatalogConfigSchema = z.object({
  refreshIntervalMs: z.number().int().min(0).optional(),
  refreshOnStart: z.boolean().optional(),
});
export type ModelCatalogConfig = z.infer<typeof ModelCatalogConfigSchema>;

registerConfigSection(MODEL_CATALOG_SECTION, ModelCatalogConfigSchema);

export const ModelOverridesSchema = z.object({
  temperature: z.number().optional(),
  topP: z.number().optional(),
  thinkingKeep: z.string().optional(),
  maxCompletionTokens: z.number().int().optional(),
});
export type ModelOverrides = z.infer<typeof ModelOverridesSchema>;

const modelOverrideEnvBindings = envBindings(ModelOverridesSchema, {
  temperature: {
    env: "KIMI_MODEL_TEMPERATURE",
    parse: (raw) => finiteNumber(raw),
  },
  topP: {
    env: "KIMI_MODEL_TOP_P",
    parse: (raw) => finiteNumber(raw),
  },
  thinkingKeep: {
    env: "KIMI_MODEL_THINKING_KEEP",
    parse: nonEmpty,
  },
  maxCompletionTokens: {
    env: "KIMI_MODEL_MAX_COMPLETION_TOKENS",
    parse: (raw) => integer(raw),
  },
});

registerConfigSection(MODEL_OVERRIDES_SECTION, ModelOverridesSchema, {
  env: modelOverrideEnvBindings,
  stripEnv: stripEnvBoundFields(modelOverrideEnvBindings),
});

function finiteNumber(raw: string): number | undefined {
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function integer(raw: string): number | undefined {
  const parsed = Number(raw);
  return Number.isInteger(parsed) ? parsed : undefined;
}

function stripScalarEnv(name: string): ConfigStripEnv<string | undefined> {
  return (value, raw, getEnv) =>
    getEnv?.(name) === undefined ? value : typeof raw === "string" ? raw : undefined;
}
