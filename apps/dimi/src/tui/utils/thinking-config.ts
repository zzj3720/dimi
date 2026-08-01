import type { DimiConfig, ThinkingEffort } from '@dimi-agent/dimi-sdk';

/** Whether a thinking effort represents "thinking enabled" (anything but 'off'). */
export function isThinkingOn(effort: ThinkingEffort): boolean {
  return effort !== 'off';
}

/**
 * Key under the `[model_efforts]` config section for a (provider, model) pair.
 * The mapping records the effort the user last chose for that model so each
 * model keeps its own thinking level.
 */
export function modelEffortKey(provider: string, model: string): string {
  return `${provider}/${model}`;
}

/**
 * The remembered effort for a model from the `[model_efforts]` config section,
 * or `undefined` when the user never chose one for it (the global `[thinking]`
 * default or the model's own default applies then).
 */
export function rememberedEffortFromConfig(
  config: DimiConfig,
  model: { readonly provider: string; readonly model: string } | undefined,
): ThinkingEffort | undefined {
  if (model === undefined) return undefined;
  const efforts = config['modelEfforts'] as Record<string, string> | undefined;
  const remembered = efforts?.[modelEffortKey(model.provider, model.model)];
  return remembered as ThinkingEffort | undefined;
}

/**
 * Project a thinking effort to the `[thinking]` config patch persisted to
 * config.toml. `'off'` disables thinking; `'on'` is the boolean-model
 * on-signal rather than a declared effort, so it only persists `enabled` —
 * boolean models resolve back to their default at runtime via
 * `defaultThinkingEffortFor`. Any concrete effort — including the model's
 * highest declared level — persists as the global default, so a new session
 * resumes exactly the tier the user chose.
 */
export function thinkingEffortToConfig(
  effort: ThinkingEffort,
): {
  enabled: boolean;
  effort?: string;
} {
  if (effort === 'off') return { enabled: false };
  if (effort === 'on') return { enabled: true };
  return { enabled: true, effort };
}

/**
 * Inverse of {@link thinkingEffortToConfig}: derive the runtime thinking effort
 * to activate a model with from the persisted `[thinking]` config. Returns
 * `'off'` when thinking is disabled, the configured concrete effort when set,
 * and `undefined` when thinking is enabled without a concrete effort so the
 * model's own default applies.
 */
export function thinkingEffortFromConfig(
  config: { enabled?: boolean; effort?: string } | undefined,
): ThinkingEffort | undefined {
  if (config?.enabled === false) return 'off';
  return config?.effort;
}
