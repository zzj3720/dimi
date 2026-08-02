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
  /** Rust exec layer: filesystem (M2) — the IHostFileSystem socket. */
  RustFileSystem: RustFileSystemConstructor;
  /** `readLines` async-iterator handle class. */
  RustReadLines: RustReadLinesConstructor;
  /** Rust exec layer: environment probe (M2) — the IHostEnvironment socket. */
  RustHostEnvironment: RustHostEnvironmentConstructor;
  /** Rust exec layer: fs watch (M2) — the IHostFsWatchService socket. */
  RustFsWatch: RustFsWatchConstructor;
  /** Watch session handle class. */
  RustFsWatchHandle: RustFsWatchHandleConstructor;
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

/** `readText` / `readLines` options — `BufferEncoding` + `TextDecodeErrors`. */
export interface RustReadTextOptions {
  encoding?: string;
  errors?: 'strict' | 'replace' | 'ignore';
}

/** `HostFileStat` mirror (napi object). */
export interface RustFileStat {
  isFile: boolean;
  isDirectory: boolean;
  isSymbolicLink?: boolean;
  size: number;
  mtimeMs?: number;
  ino?: number;
}

/** `HostDirEntry` mirror (napi object). */
export interface RustDirEntry {
  name: string;
  isFile: boolean;
  isDirectory: boolean;
  isSymbolicLink?: boolean;
}

/** The napi `RustReadLines` class — line-iterator handle. */
export interface RustReadLinesConstructor {
  new (): RustReadLinesHandle;
}

/** `readLines` handle: `next()` yields one line (`null` at EOF). */
export interface RustReadLinesHandle {
  next(): Promise<string | null>;
  dispose(): void;
}

/** `HostEnvironmentInfo` mirror — the immutable host snapshot (napi object). */
export interface RustHostEnvironmentInfo {
  osKind: string;
  osArch: string;
  osVersion: string;
  shellName: string;
  shellPath: string;
  pathClass: string;
  homeDir: string;
}

/** The napi `RustHostEnvironment` class — stateless probe facade. */
export interface RustHostEnvironmentConstructor {
  probe(): RustHostEnvironmentInfo;
}

/** `HostFsChange` mirror (napi object). */
export interface RustFsChange {
  path: string;
  action: 'created' | 'modified' | 'deleted';
  kind: 'file' | 'directory';
}

/** `HostFsWatchOptions` — `recursive` only; `ignored` stays on the adapter. */
export interface RustFsWatchOptions {
  recursive?: boolean;
}

/** The napi `RustFsWatchHandle` class — one watch session. */
export interface RustFsWatchHandleConstructor {
  new (): RustFsWatchHandle;
}

/** Watch session: events arrive via `setOnChange` until `dispose`. */
export interface RustFsWatchHandle {
  setOnChange(onChange: (change: RustFsChange) => void): void;
  dispose(): void;
}

/** The napi `RustFsWatch` class — stateless facade. */
export interface RustFsWatchConstructor {
  watch(path: string, options?: RustFsWatchOptions): RustFsWatchHandle;
}

/**
 * `rustFsWatch` — the M2 fs-watch socket. Events are normalized to the
 * chokidar surface (created/modified/deleted × file/directory, `.git`
 * filtered); the `ignored` callback option is applied by the adapter.
 */
export function rustFsWatch(path: string, options?: RustFsWatchOptions): RustFsWatchHandle {
  return loadNative().RustFsWatch.watch(path, options);
}

/**
 * `RustFsWatch` — TS-side mirror of the napi class (binding-contract parity).
 */
export class RustFsWatch {
  static watch(path: string, options?: RustFsWatchOptions): RustFsWatchHandle {
    return rustFsWatch(path, options);
  }
}

/**
 * `RustFsWatchHandle` — TS-side mirror of the napi class (binding-contract
 * parity). Wraps a handle returned by `RustFsWatch.watch`.
 */
export class RustFsWatchHandle {
  readonly #inner: RustFsWatchHandle;

  constructor(handle: RustFsWatchHandle) {
    this.#inner = handle;
  }

  setOnChange(onChange: (change: RustFsChange) => void): void {
    this.#inner.setOnChange(onChange);
  }

  dispose(): void {
    this.#inner.dispose();
  }
}

/**
 * `rustHostEnvironmentProbe` — the M2 environment probe socket.
 * Node-parity details: `osArch` uses Node `process.arch` values (arm64/x64),
 * `osVersion` is the kernel release (`os.release()`), `shellName`/`shellPath`
 * follow the `/bin/bash` → `/usr/bin/bash` → `/usr/local/bin/bash` → `sh`
 * fallback chain.
 */
export function rustHostEnvironmentProbe(): RustHostEnvironmentInfo {
  return loadNative().RustHostEnvironment.probe();
}

/** The napi `RustFileSystem` class — stateless facade, all static methods. */
export interface RustFileSystemConstructor {
  readText(path: string, options?: RustReadTextOptions): Promise<string>;
  writeText(path: string, data: string): Promise<void>;
  appendText(path: string, data: string): Promise<void>;
  readBytes(path: string, n?: number): Promise<Uint8Array>;
  writeBytes(path: string, data: Uint8Array): Promise<void>;
  readLines(path: string, options?: RustReadTextOptions): Promise<RustReadLinesHandle>;
  createExclusive(path: string, data: Uint8Array): Promise<boolean>;
  stat(path: string): Promise<RustFileStat>;
  lstat(path: string): Promise<RustFileStat>;
  readdir(path: string): Promise<RustDirEntry[]>;
  mkdir(path: string, recursive?: boolean): Promise<void>;
  remove(path: string): Promise<void>;
  realpath(path: string): Promise<string>;
}

/**
 * `RustFileSystem` — TS-side mirror of the napi class with the same name
 * (binding-contract parity). Stateless: every method delegates to the
 * binding's static surface.
 */
export class RustFileSystem {
  static readText(path: string, options?: RustReadTextOptions): Promise<string> {
    return loadNative().RustFileSystem.readText(path, options);
  }
  static writeText(path: string, data: string): Promise<void> {
    return loadNative().RustFileSystem.writeText(path, data);
  }
  static appendText(path: string, data: string): Promise<void> {
    return loadNative().RustFileSystem.appendText(path, data);
  }
  static readBytes(path: string, n?: number): Promise<Uint8Array> {
    return loadNative().RustFileSystem.readBytes(path, n);
  }
  static writeBytes(path: string, data: Uint8Array): Promise<void> {
    return loadNative().RustFileSystem.writeBytes(path, data);
  }
  static async readLines(
    path: string,
    options?: RustReadTextOptions,
  ): Promise<RustReadLinesHandle> {
    return loadNative().RustFileSystem.readLines(path, options);
  }
  static createExclusive(path: string, data: Uint8Array): Promise<boolean> {
    return loadNative().RustFileSystem.createExclusive(path, data);
  }
  static stat(path: string): Promise<RustFileStat> {
    return loadNative().RustFileSystem.stat(path);
  }
  static lstat(path: string): Promise<RustFileStat> {
    return loadNative().RustFileSystem.lstat(path);
  }
  static readdir(path: string): Promise<RustDirEntry[]> {
    return loadNative().RustFileSystem.readdir(path);
  }
  static mkdir(path: string, recursive?: boolean): Promise<void> {
    return loadNative().RustFileSystem.mkdir(path, recursive);
  }
  static remove(path: string): Promise<void> {
    return loadNative().RustFileSystem.remove(path);
  }
  static realpath(path: string): Promise<string> {
    return loadNative().RustFileSystem.realpath(path);
  }
}

/**
 * `RustReadLines` — TS-side mirror of the napi class (binding-contract
 * parity). Wraps a handle returned by `RustFileSystem.readLines`.
 */
export class RustReadLines {
  readonly #inner: RustReadLinesHandle;

  constructor(handle: RustReadLinesHandle) {
    this.#inner = handle;
  }

  /** Next line including its `\n` terminator; `null` at EOF. */
  next(): Promise<string | null> {
    return this.#inner.next();
  }

  /** Drop the file handle early. */
  dispose(): void {
    this.#inner.dispose();
  }
}

/**
 * `RustHostEnvironment` — TS-side mirror of the napi class (binding-contract
 * parity). Stateless probe facade.
 */
export class RustHostEnvironment {
  /** `probeHostEnvironmentFromNode` — the immutable host snapshot. */
  static probe(): RustHostEnvironmentInfo {
    return rustHostEnvironmentProbe();
  }
}
