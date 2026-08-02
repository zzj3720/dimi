/**
 * `hostProcess` domain (L1) — `IHostProcessService` Rust-backed
 * implementation (M2, slice 1).
 *
 * Swap-in socket for `DIMI_RUST_EXEC=1`: spawns through `dimi-exec` (the
 * napi bridge) instead of `node:child_process`. The bridge process handle
 * owns the pipes; this class adapts it to the `IHostProcess` contract —
 * stdout/stderr become `Readable` streams fed by the Rust pump callbacks,
 * stdin a `Writable` that forwards chunks to the child.
 *
 * Semantics mirrored from the node-local backend: unix `detached` defaults
 * to true (own session/process group, `kill` reaches the whole tree),
 * `mergeStderr` aliases stderr to the stdout stream, `wait()` resolves with
 * the exit code (`-1` when killed by a signal, `code ?? -1`).
 */

import { Readable, Writable } from 'node:stream';

import { rustHostProcessSpawn, type RustHostProcessHandle } from '@dimi-agent/dimi-native';

import {
  HostProcessError,
  HostProcessErrorCode,
  IHostProcessService,
  type HostProcessOptions,
  type IHostProcess,
} from '#/os/interface/hostProcess';

function toRustOptions(options: HostProcessOptions): {
  cwd?: string;
  env?: Record<string, string>;
  shellDefault?: boolean;
  shellPath?: string;
  detached?: boolean;
  windowsHide?: boolean;
} {
  const rust: {
    cwd?: string;
    env?: Record<string, string>;
    shellDefault?: boolean;
    shellPath?: string;
    detached?: boolean;
    windowsHide?: boolean;
  } = {
    cwd: options.cwd,
    env: buildEnv(options.env),
    windowsHide: options.windowsHide,
    detached: options.detached,
  };
  if (options.shell === true) rust.shellDefault = true;
  else if (typeof options.shell === 'string') rust.shellPath = options.shell;
  return rust;
}

function buildEnv(
  overrides: Record<string, string> | undefined,
): Record<string, string> | undefined {
  if (overrides === undefined) return undefined;
  return { ...(process.env as Record<string, string>), ...overrides };
}

class RustHostProcess implements IHostProcess {
  declare readonly _serviceBrand: undefined;

  readonly stdin: Writable;
  readonly stdout: Readable;
  readonly stderr: Readable;
  readonly pid: number;

  private readonly _handle: RustHostProcessHandle;
  private _exitCode: number | null = null;
  private readonly _exitPromise: Promise<number>;
  private _disposed = false;

  constructor(handle: RustHostProcessHandle, mergeStderr: boolean) {
    this._handle = handle;
    this.pid = handle.pid;

    this.stdout = new Readable({ highWaterMark: 128 * 1024, read() {} });
    this.stderr = mergeStderr
      ? this.stdout
      : new Readable({ highWaterMark: 128 * 1024, read() {} });

    this.stdin = new Writable({
      write: (chunk, _encoding, callback) => {
        try {
          this._handle.writeStdin(new Uint8Array(chunk));
          callback();
        } catch (error) {
          callback(error as Error);
        }
      },
      destroy: (_error, callback) => {
        this._handle.closeStdin();
        callback();
      },
    });

    handle.setStreamCallbacks(
      (chunk) => {
        if (!this.stdout.destroyed) this.stdout.push(Buffer.from(chunk));
      },
      (chunk) => {
        if (!this.stderr.destroyed && this.stderr !== this.stdout) {
          this.stderr.push(Buffer.from(chunk));
        }
      },
      () => {
        if (!this.stdout.destroyed) this.stdout.push(null);
      },
      () => {
        if (!this.stderr.destroyed && this.stderr !== this.stdout) {
          this.stderr.push(null);
        }
      },
    );

    this._exitPromise = handle.wait().then((code) => {
      this._exitCode = code;
      return code;
    });
  }

  get exitCode(): number | null {
    return this._exitCode;
  }

  async wait(): Promise<number> {
    return this._exitPromise;
  }

  async kill(signal?: NodeJS.Signals): Promise<void> {
    if (this.pid <= 0) return;
    try {
      this._handle.kill(signal ?? 'SIGTERM');
    } catch (error) {
      throw new HostProcessError(
        HostProcessErrorCode.KillFailed,
        `Failed to kill process ${this.pid}: ${(error as Error).message}`,
        { details: { pid: this.pid, signal: signal ?? 'SIGTERM' }, cause: error },
      );
    }
  }

  dispose(): void {
    if (this._disposed) return;
    this._disposed = true;
    this._handle.dispose();
    this.stdin.destroy();
    this.stdout.destroy();
    if (this.stderr !== this.stdout) {
      this.stderr.destroy();
    }
  }
}

export class RustHostProcessService implements IHostProcessService {
  declare readonly _serviceBrand: undefined;

  async spawn(
    command: string,
    args: readonly string[] = [],
    options: HostProcessOptions = {},
  ): Promise<IHostProcess> {
    let handle: RustHostProcessHandle;
    try {
      handle = await rustHostProcessSpawn(command, args, toRustOptions(options));
    } catch (error) {
      // The bridge message already carries the `Failed to spawn "<cmd>": …`
      // prefix and the OS error text — do not re-wrap it.
      const err = error as NodeJS.ErrnoException;
      throw new HostProcessError(
        HostProcessErrorCode.SpawnFailed,
        (error as Error).message,
        {
          details: { command, args: [...args], cwd: options.cwd, errno: err.code },
          cause: error,
        },
      );
    }
    return new RustHostProcess(handle, options.mergeStderr ?? false);
  }
}
