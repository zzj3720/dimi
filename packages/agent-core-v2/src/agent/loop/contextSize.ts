/**
 * `loop` domain (L4) — user-configurable context-size scaling.
 *
 * The effective conversation context window is the model's fixed
 * `contextWindow` by default. Users may cap it as a percentage of that
 * window (config `[loop_control] context_size_percent`), in 5% steps,
 * never below `CONTEXT_SIZE_FLOOR_TOKENS` (200k). Models whose own window
 * is already below the floor are not adjustable: `scaleContextTokens`
 * leaves them unchanged because the floor never exceeds the window
 * (`min(window, floor)`).
 *
 * The UI derives its offered steps from `contextSizePercentOptions`;
 * the engine applies `scaleContextTokens` / `scaleModelCapabilityContext`
 * on every profile context resolution so config changes take effect on
 * the next request without restart.
 */

import type { ModelCapability } from "#/llmProtocol/capability";

/** Step between offered context-size levels (percent). */
export const CONTEXT_SIZE_STEP_PERCENT = 5;

/** Smallest user-selectable percentage of the model window. */
export const CONTEXT_SIZE_PERCENT_MIN = 5;

/** Effective context never drops below this token count when adjustable. */
export const CONTEXT_SIZE_FLOOR_TOKENS = 200_000;

/**
 * Scale a token count by `percent` of the model's default window, keeping
 * at least `CONTEXT_SIZE_FLOOR_TOKENS` — but never more than the original
 * window, so models already below the floor (and `percent` at or above
 * 100, or unset) pass through unchanged.
 */
export function scaleContextTokens(tokens: number, percent: number | undefined): number {
  if (!Number.isFinite(tokens) || tokens <= 0) return tokens;
  if (percent === undefined || percent <= 0 || percent >= 100) return tokens;
  const scaled = Math.floor((tokens * percent) / 100);
  return Math.max(scaled, Math.min(tokens, CONTEXT_SIZE_FLOOR_TOKENS));
}

/**
 * Apply the configured context-size percentage to a model capability.
 * Returns the original object unchanged when nothing changes, so
 * `UNKNOWN_CAPABILITY` identity and other callers relying on object
 * equality keep working.
 */
export function scaleModelCapabilityContext(
  capability: ModelCapability,
  percent: number | undefined,
): ModelCapability {
  if (percent === undefined || percent <= 0 || percent >= 100) return capability;
  const maxContextTokens = scaleContextTokens(capability.max_context_tokens, percent);
  const maxInputTokens =
    capability.max_input_tokens === undefined
      ? undefined
      : scaleContextTokens(capability.max_input_tokens, percent);
  if (
    maxContextTokens === capability.max_context_tokens &&
    maxInputTokens === capability.max_input_tokens
  ) {
    return capability;
  }
  return { ...capability, max_context_tokens: maxContextTokens, max_input_tokens: maxInputTokens };
}

/**
 * Offered percentage levels for a model window: `100`, `95`, … descending
 * in `CONTEXT_SIZE_STEP_PERCENT` steps while the scaled size stays at or
 * above `CONTEXT_SIZE_FLOOR_TOKENS`. Empty when the window is below the
 * floor — the model's context size is not adjustable.
 */
export function contextSizePercentOptions(contextWindow: number): readonly number[] {
  if (!Number.isFinite(contextWindow) || contextWindow <= 0) return [];
  const options: number[] = [];
  for (
    let percent = 100;
    percent >= CONTEXT_SIZE_PERCENT_MIN;
    percent -= CONTEXT_SIZE_STEP_PERCENT
  ) {
    if (Math.floor((contextWindow * percent) / 100) < CONTEXT_SIZE_FLOOR_TOKENS) break;
    options.push(percent);
  }
  return options;
}
