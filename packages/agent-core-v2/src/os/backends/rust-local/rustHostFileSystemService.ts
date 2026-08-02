/**
 * `hostFs` domain (L1) — `IHostFileSystem` Rust-backed implementation
 * (M2, slice 2).
 *
 * Default backend since M2 (the CLI `--legacy` flag keeps node-local): every call goes through the
 * `dimi-exec` napi bridge (`RustFileSystem`) instead of `node:fs/promises`.
 * The bridge formats failures as `"{ERRNO} {op} failed: {message}"`; this
 * adapter extracts the leading errno symbol and reuses the shared
 * `toHostFsError` mapper, so consumers see the same `os.fs.*` codes in
 * both modes. `readLines` wraps the Rust streaming handle (`RustReadLines`)
 * in the `AsyncGenerator` contract.
 */

import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';
import type { TextDecodeErrors } from '#/_base/execEnv/decodeText';
import {
  RustFileSystem,
  type RustReadLinesHandle,
  type RustReadTextOptions,
} from '@dimi-agent/dimi-native';

import {
  type HostDirEntry,
  type HostFileStat,
  IHostFileSystem,
} from '#/os/interface/hostFileSystem';
import { toHostFsError, type HostFsError } from '#/os/interface/hostFsErrors';

const ERRNO_PREFIX = /^([A-Z0-9_]+) /;

/** Re-shape a bridge error into the ErrnoException-like input `toHostFsError` reads. */
function toFsError(error: unknown, ctx: { path: string; op: string }): HostFsError {
  const errno = ERRNO_PREFIX.exec((error as Error).message ?? '')?.[1];
  return toHostFsError({ code: errno, message: (error as Error).message }, ctx);
}

function toRustOptions(options?: {
  encoding?: BufferEncoding;
  errors?: TextDecodeErrors;
}): RustReadTextOptions | undefined {
  if (options === undefined) return undefined;
  const rust: RustReadTextOptions = {};
  if (options.encoding !== undefined) rust.encoding = options.encoding;
  if (options.errors !== undefined) rust.errors = options.errors;
  return rust;
}

export class RustHostFileSystem implements IHostFileSystem {
  declare readonly _serviceBrand: undefined;

  async readText(
    path: string,
    options?: { encoding?: BufferEncoding; errors?: TextDecodeErrors },
  ): Promise<string> {
    try {
      return await RustFileSystem.readText(path, toRustOptions(options));
    } catch (error) {
      throw toFsError(error, { path, op: 'read' });
    }
  }

  async writeText(path: string, data: string): Promise<void> {
    try {
      await RustFileSystem.writeText(path, data);
    } catch (error) {
      throw toFsError(error, { path, op: 'write' });
    }
  }

  async appendText(path: string, data: string): Promise<void> {
    try {
      await RustFileSystem.appendText(path, data);
    } catch (error) {
      throw toFsError(error, { path, op: 'append' });
    }
  }

  async readBytes(path: string, n?: number): Promise<Uint8Array> {
    try {
      return await RustFileSystem.readBytes(path, n);
    } catch (error) {
      throw toFsError(error, { path, op: 'read' });
    }
  }

  async writeBytes(path: string, data: Uint8Array): Promise<void> {
    try {
      await RustFileSystem.writeBytes(path, data);
    } catch (error) {
      throw toFsError(error, { path, op: 'write' });
    }
  }

  async *readLines(
    path: string,
    options?: { encoding?: BufferEncoding; errors?: TextDecodeErrors },
  ): AsyncGenerator<string> {
    let handle: RustReadLinesHandle;
    try {
      handle = await RustFileSystem.readLines(path, toRustOptions(options));
    } catch (error) {
      throw toFsError(error, { path, op: 'read' });
    }
    try {
      for (;;) {
        let line: string | null;
        try {
          line = await handle.next();
        } catch (error) {
          throw toFsError(error, { path, op: 'read' });
        }
        if (line === null) break;
        yield line;
      }
    } finally {
      handle.dispose();
    }
  }

  async createExclusive(path: string, data: Uint8Array): Promise<boolean> {
    try {
      return await RustFileSystem.createExclusive(path, data);
    } catch (error) {
      throw toFsError(error, { path, op: 'create' });
    }
  }

  async stat(path: string): Promise<HostFileStat> {
    try {
      const stat = await RustFileSystem.stat(path);
      return {
        isFile: stat.isFile,
        isDirectory: stat.isDirectory,
        isSymbolicLink: stat.isSymbolicLink,
        size: stat.size,
        mtimeMs: stat.mtimeMs,
        ino: stat.ino,
      };
    } catch (error) {
      throw toFsError(error, { path, op: 'stat' });
    }
  }

  async lstat(path: string): Promise<HostFileStat> {
    try {
      const stat = await RustFileSystem.lstat(path);
      return {
        isFile: stat.isFile,
        isDirectory: stat.isDirectory,
        isSymbolicLink: stat.isSymbolicLink,
        size: stat.size,
        mtimeMs: stat.mtimeMs,
        ino: stat.ino,
      };
    } catch (error) {
      throw toFsError(error, { path, op: 'lstat' });
    }
  }

  async readdir(path: string): Promise<readonly HostDirEntry[]> {
    try {
      const entries = await RustFileSystem.readdir(path);
      return entries.map((entry) => ({
        name: entry.name,
        isFile: entry.isFile,
        isDirectory: entry.isDirectory,
        isSymbolicLink: entry.isSymbolicLink,
      }));
    } catch (error) {
      throw toFsError(error, { path, op: 'readdir' });
    }
  }

  async mkdir(path: string, options?: { readonly recursive?: boolean }): Promise<void> {
    try {
      await RustFileSystem.mkdir(path, options?.recursive ?? false);
    } catch (error) {
      throw toFsError(error, { path, op: 'mkdir' });
    }
  }

  async remove(path: string): Promise<void> {
    try {
      await RustFileSystem.remove(path);
    } catch (error) {
      throw toFsError(error, { path, op: 'remove' });
    }
  }

  async realpath(path: string): Promise<string> {
    try {
      return await RustFileSystem.realpath(path);
    } catch (error) {
      throw toFsError(error, { path, op: 'realpath' });
    }
  }
}
