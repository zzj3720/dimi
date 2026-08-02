/**
 * @dimi-agent/dimi-native — TS wrapper over the dimi Rust runtime bridge.
 *
 * The native binding (`dist/dimi_bridge.node`) is produced from
 * `crates/dimi-bridge` by `pnpm --filter @dimi-agent/dimi-native run build:native`.
 * Every function here mirrors a `#[napi]` export 1:1; contract semantics
 * live in `dimi-wire` (Rust) and `@dimi-agent/transcript` (zod) and are
 * cross-checked by the differential test suite.
 */
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

export interface NativeBinding {
  /** Parse one transcript item and re-serialize it canonically. */
  normalizeItem: typeof normalizeItem;
  /** Parse one transcript step and re-serialize it canonically. */
  normalizeStep: typeof normalizeStep;
  /** Parse one task and re-serialize it canonically. */
  normalizeTask: typeof normalizeTask;
  /** Parse one agent phase and re-serialize it canonically. */
  normalizePhase: typeof normalizePhase;
  /** Filename-safe agent id check. */
  isPlainAgentId: typeof isPlainAgentId;
}

let binding: NativeBinding | undefined;

/**
 * Loads the native binding, building nothing. Throws with a pointer to the
 * build command when `dist/dimi_bridge.node` is missing.
 */
export function loadNative(): NativeBinding {
  if (binding) return binding;
  try {
    binding = require('../dist/dimi_bridge.node') as NativeBinding;
  } catch (error) {
    throw new Error(
      'dimi-native: native binding not found; run `pnpm --filter @dimi-agent/dimi-native run build:native`',
      { cause: error },
    );
  }
  return binding;
}

export function normalizeItem(json: string): string {
  return loadNative().normalizeItem(json);
}

export function normalizeStep(json: string): string {
  return loadNative().normalizeStep(json);
}

export function normalizeTask(json: string): string {
  return loadNative().normalizeTask(json);
}

export function normalizePhase(json: string): string {
  return loadNative().normalizePhase(json);
}

export function isPlainAgentId(id: string): boolean {
  return loadNative().isPlainAgentId(id);
}
