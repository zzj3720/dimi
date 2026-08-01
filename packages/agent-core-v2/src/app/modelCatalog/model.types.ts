/**
 * `modelCatalog` domain (L2) — shared pure-data types no single contract owns.
 *
 * One home for the small data interfaces that would otherwise each sit in a
 * near-empty file:
 *   - `CompletionBudgetConfig` / `CompletionBudgetParams` — the budget knobs
 *     resolved and folded by the pure functions in `completionBudget.ts`.
 * The provider runtime owns provider auth and model thinking metadata directly.
 *
 * Types only — the functions and services that produce or consume them stay
 * in their own files.
 */

export interface CompletionBudgetConfig {
  readonly hardCap?: number;
  readonly fallback?: number;
}

export interface CompletionBudgetParams {
  readonly maxCompletionTokens: number;
  readonly usedContextTokens?: number;
  readonly maxContextTokens?: number;
}
