/** `loop` domain (L4) — selected turn runtime for cross-domain wake routing. */

/**
 * The Rust engine is the default runtime; the CLI `--legacy` flag sets
 * `DIMI_LEGACY=1` to keep the TypeScript loop.
 */
export function rustEngineEnabled(): boolean {
  return process.env["DIMI_LEGACY"] !== "1";
}
