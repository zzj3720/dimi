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
  /** Full cold rebuild: wire records → snapshot JSON. */
  coldRebuild: typeof coldRebuild;
  /** Page items by turn cursor. */
  paginateTurns: typeof paginateTurns;
  /** Parse a `wire.jsonl` file into records JSON. */
  readWireRecords: typeof readWireRecords;
  /** One agent's transcript store, held on the Rust side. */
  RustAgentTranscript: RustAgentTranscriptConstructor;
  /** Rust exec layer: process spawn (M2) — the IHostProcessService socket. */
  RustHostProcess: RustHostProcessConstructor;
}

export interface RustAgentTranscriptConstructor {
  new (agentId: string): RustAgentTranscriptHandle;
}

export interface RustAgentTranscriptHandle {
  /** Apply an op batch (ops JSON array); returns `AppliedOps` JSON. */
  apply(opsJson: string): string;
  /** Snapshot JSON; optional `{ tailTurns }` window JSON. */
  snapshot(windowJson?: string): string;
}

/** `HostProcessOptions` shape passed to `RustHostProcess.spawn`. */
export interface RustHostProcessOptions {
  cwd?: string;
  env?: Record<string, string>;
  /** `shell: true` — platform default shell. */
  shellDefault?: boolean;
  /** `shell: "/bin/bash"` — explicit shell binary. */
  shellPath?: string;
  detached?: boolean;
  windowsHide?: boolean;
  mergeStderr?: boolean;
}

export interface RustHostProcessConstructor {
  spawn(
    command: string,
    args: readonly string[],
    options?: RustHostProcessOptions,
  ): Promise<RustHostProcessHandle>;
}

export interface RustHostProcessHandle {
  readonly pid: number;
  readonly exitCode: number | null;
  /**
   * Wire stream pumps. Call once right after spawn: `onStdout(chunk)` /
   * `onStderr(chunk)` fire per pipe chunk, `onStdoutEnd()` / `onStderrEnd()`
   * once at EOF. Positional — the napi surface takes four functions.
   */
  setStreamCallbacks(
    onStdout: (chunk: Uint8Array) => void,
    onStderr: (chunk: Uint8Array) => void,
    onStdoutEnd: () => void,
    onStderrEnd: () => void,
  ): void;
  /** Resolves with the exit code (`-1` when killed by a signal). */
  wait(): Promise<number>;
  kill(signal?: string): void;
  writeStdin(chunk: Uint8Array): void;
  closeStdin(): void;
  dispose(): void;
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

/** Full cold rebuild: wire records JSON array → snapshot JSON. */
export function coldRebuild(recordsJson: string): string {
  return loadNative().coldRebuild(recordsJson);
}

/** Page items by turn cursor (items JSON array + query JSON → page JSON). */
export function paginateTurns(itemsJson: string, queryJson: string): string {
  return loadNative().paginateTurns(itemsJson, queryJson);
}

/** Parse a `wire.jsonl` file into records JSON. */
export function readWireRecords(path: string): string {
  return loadNative().readWireRecords(path);
}

/**
 * One agent's transcript store, held on the Rust side — the swap-in socket
 * for the kap-server `TranscriptService` storage backend.
 */
export class RustAgentTranscript {
  readonly #inner: RustAgentTranscriptHandle;

  constructor(agentId: string) {
    // NOTE: `new loadNative().RustAgentTranscript(...)` would be parsed as
    // `(new loadNative()).RustAgentTranscript(...)` — a `new`-less call that
    // returns a method-less stub object. Bind the class first.
    const NativeClass = loadNative().RustAgentTranscript;
    this.#inner = new NativeClass(agentId);
  }

  /** Apply an op batch (ops JSON array); returns `AppliedOps` JSON. */
  apply(opsJson: string): string {
    return this.#inner.apply(opsJson);
  }

  /** Snapshot JSON; optional `{ tailTurns }` window JSON. */
  snapshot(windowJson?: string): string {
    return this.#inner.snapshot(windowJson);
  }
}

/** `RustHostProcess.spawn` — the M2 exec spawn socket. */
export async function rustHostProcessSpawn(
  command: string,
  args: readonly string[],
  options?: RustHostProcessOptions,
): Promise<RustHostProcessHandle> {
  return loadNative().RustHostProcess.spawn(command, [...args], options);
}

/**
 * `RustHostProcess` — TS-side mirror of the napi class with the same name
 * (the binding-contract suite pins wrapper ↔ binding export parity). The
 * napi class is async-constructed through its static `spawn`, so this mirror
 * exposes the same static and throws from the constructor.
 */
export class RustHostProcess {
  /** Spawn one child; resolves with the process handle. */
  static spawn(
    command: string,
    args: readonly string[],
    options?: RustHostProcessOptions,
  ): Promise<RustHostProcessHandle> {
    return rustHostProcessSpawn(command, args, options);
  }

  constructor() {
    throw new Error(
      'RustHostProcess is async-constructed: use `await RustHostProcess.spawn(command, args, options)`',
    );
  }
}
