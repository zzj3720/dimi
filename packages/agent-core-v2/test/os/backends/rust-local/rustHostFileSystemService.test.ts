/**
 * `hostFs` domain (L1) — integration test for the Rust-backed
 * `RustHostFileSystem` (M2 slice 2 swap-in socket, `DIMI_RUST_FS=1`).
 *
 * Exercises the adapter contract through the real napi bridge: the shared
 * `toHostFsError` mapping (bridge errno symbol → `os.fs.*` code), the
 * `readLines` generator, `createExclusive` semantics and the stat/readdir
 * models.
 */
import { mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'pathe';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { DisposableStore } from '#/_base/di/lifecycle';
import { createServices, type TestInstantiationService } from '#/_base/di/test';
import { IHostFileSystem } from '#/os/interface/hostFileSystem';
import { HostFsError, OsFsErrors } from '#/os/interface/hostFsErrors';
import { RustHostFileSystem } from '#/os/backends/rust-local/rustHostFileSystemService';

let dir: string;
let disposables: DisposableStore;
let ix: TestInstantiationService;

async function fs(): Promise<IHostFileSystem> {
  return ix.get(IHostFileSystem);
}

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'dimi-rust-fs-'));
  disposables = new DisposableStore();
  ix = createServices(disposables, {
    additionalServices: (reg) => {
      reg.define(IHostFileSystem, RustHostFileSystem);
    },
  });
});

afterEach(async () => {
  disposables.dispose();
  await rm(dir, { recursive: true, force: true });
});

describe('RustHostFileSystem error mapping', () => {
  it('maps ENOENT to os.fs.not_found', async () => {
    const missing = join(dir, 'missing.txt');
    await expect(fs().then((f) => f.readText(missing))).rejects.toSatisfy((err: unknown) => {
      expect(err).toBeInstanceOf(HostFsError);
      const error = err as HostFsError;
      expect(error.code).toBe(OsFsErrors.codes.OS_FS_NOT_FOUND);
      expect(error.details).toMatchObject({ path: missing, op: 'read' });
      return true;
    });
  });

  it('maps EEXIST to os.fs.already_exists for non-recursive mkdir', async () => {
    const sub = join(dir, 'sub');
    await (await fs()).mkdir(sub);
    await expect((await fs()).mkdir(sub)).rejects.toSatisfy((err: unknown) => {
      expect((err as HostFsError).code).toBe(OsFsErrors.codes.OS_FS_ALREADY_EXISTS);
      return true;
    });
  });

  it('maps ENOTDIR to os.fs.not_directory for readdir on a file', async () => {
    const file = join(dir, 'f.txt');
    await writeFile(file, 'x', 'utf-8');
    await expect((await fs()).readdir(file)).rejects.toSatisfy((err: unknown) => {
      expect((err as HostFsError).code).toBe(OsFsErrors.codes.OS_FS_NOT_DIRECTORY);
      return true;
    });
  });
});

describe('RustHostFileSystem read / write', () => {
  it('roundtrips text and bytes', async () => {
    const f = await fs();
    const file = join(dir, 'a.txt');
    await f.writeText(file, 'hello');
    expect(await f.readText(file)).toBe('hello');
    await f.appendText(file, ' world');
    expect(await f.readText(file)).toBe('hello world');
    await f.writeBytes(file, new TextEncoder().encode('bytes'));
    expect(new TextDecoder().decode(await f.readBytes(file))).toBe('bytes');
    expect(new TextDecoder().decode(await f.readBytes(file, 2))).toBe('by');
  });

  it('streams readLines as an async generator', async () => {
    const file = join(dir, 'lines.txt');
    await writeFile(file, 'a\nb\nc', 'utf-8');
    const lines: string[] = [];
    for await (const line of (await fs()).readLines(file)) {
      lines.push(line);
    }
    expect(lines).toEqual(['a\n', 'b\n', 'c']);
  });

  it('createExclusive returns true once, false after', async () => {
    const f = await fs();
    const file = join(dir, 'ex.txt');
    expect(await f.createExclusive(file, new TextEncoder().encode('x'))).toBe(true);
    expect(await f.createExclusive(file, new TextEncoder().encode('y'))).toBe(false);
    expect(await f.readText(file)).toBe('x');
  });
});

describe('RustHostFileSystem stat / readdir / mkdir / remove', () => {
  it('stat follows symlinks while lstat does not', async () => {
    const f = await fs();
    const target = join(dir, 'target.txt');
    await writeFile(target, 'hello', 'utf-8');
    const link = join(dir, 'link.txt');
    await symlink(target, link);

    const st = await f.stat(link);
    expect(st.isFile).toBe(true);
    expect(st.isSymbolicLink).not.toBe(true);
    const lst = await f.lstat(link);
    expect(lst.isSymbolicLink).toBe(true);
    expect(lst.isFile).toBe(false);
  });

  it('readdir lists entries with flags', async () => {
    const f = await fs();
    await mkdir(join(dir, 'sub'));
    await writeFile(join(dir, 'f.txt'), 'x', 'utf-8');
    const entries = await f.readdir(dir);
    const byName = new Map(entries.map((e) => [e.name, e]));
    expect(byName.get('sub')?.isDirectory).toBe(true);
    expect(byName.get('f.txt')?.isFile).toBe(true);
  });

  it('remove handles files, trees and missing paths', async () => {
    const f = await fs();
    const file = join(dir, 'f.txt');
    await writeFile(file, 'x', 'utf-8');
    await f.remove(file);
    await expect(f.readText(file)).rejects.toMatchObject({ code: OsFsErrors.codes.OS_FS_NOT_FOUND });

    const tree = join(dir, 'tree');
    await mkdir(join(tree, 'deep'), { recursive: true });
    await f.remove(tree);
    await expect(f.stat(tree)).rejects.toMatchObject({ code: OsFsErrors.codes.OS_FS_NOT_FOUND });

    await expect(f.remove(join(dir, 'never'))).resolves.toBeUndefined();
  });

  it('realpath resolves symlinks', async () => {
    const f = await fs();
    const target = join(dir, 'real.txt');
    await writeFile(target, 'x', 'utf-8');
    const link = join(dir, 'alias.txt');
    await symlink(target, link);
    const resolved = await f.realpath(link);
    expect(resolved.endsWith('real.txt')).toBe(true);
  });
});
