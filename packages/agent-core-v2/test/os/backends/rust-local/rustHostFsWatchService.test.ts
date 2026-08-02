/**
 * `hostFsWatch` domain (L1) — integration test against the real Rust
 * (`notify` via the napi bridge) watcher on a temporary directory.
 * Mirrors the node-local `hostFsWatchService.test.ts` surface.
 */

import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { RustHostFsWatchService } from '#/os/backends/rust-local/rustHostFsWatchService';
import type { HostFsChange, IHostFsWatchHandle } from '#/os/interface/hostFsWatch';

const wait = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

describe('RustHostFsWatchService', () => {
  let root: string;
  let handle: IHostFsWatchHandle | undefined;

  afterEach(async () => {
    handle?.dispose();
    handle = undefined;
    if (root) await rm(root, { recursive: true, force: true });
  });

  async function start(recursive = true): Promise<HostFsChange[]> {
    const events: HostFsChange[] = [];
    const svc = new RustHostFsWatchService();
    handle = svc.watch(root, { recursive });
    handle.onDidChange((e) => events.push(e));
    await wait(300);
    return events;
  }

  it('reports create / modify / delete for a file', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustfswatch-'));
    const events = await start();

    const file = join(root, 'a.txt');
    await writeFile(file, 'v1');
    await wait(400);
    await writeFile(file, 'v2');
    await wait(400);
    await rm(file);
    await wait(400);

    const actions = events.filter((e) => e.path === file).map((e) => e.action);
    expect(actions).toContain('created');
    expect(actions).toContain('modified');
    expect(actions).toContain('deleted');
    expect(events.find((e) => e.path === file)?.kind).toBe('file');
  });

  it('reports directory create / delete with directory kind', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustfswatch-'));
    const events = await start();

    const dir = join(root, 'sub');
    await mkdir(dir);
    await wait(400);
    await rm(dir, { recursive: true });
    await wait(400);

    const created = events.find((e) => e.path === dir && e.action === 'created');
    const deleted = events.find((e) => e.path === dir && e.action === 'deleted');
    expect(created?.kind).toBe('directory');
    expect(deleted?.kind).toBe('directory');
  });

  it('does not fire for paths ignored by default (.git)', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustfswatch-'));
    const events = await start();

    await mkdir(join(root, '.git'));
    await writeFile(join(root, '.git', 'config'), 'x');
    await wait(400);

    expect(events.some((e) => e.path.includes('/.git/') || e.path.endsWith('/.git'))).toBe(false);
  });

  it('does not fire for pre-existing files (ignoreInitial)', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustfswatch-'));
    const preexisting = join(root, 'pre.txt');
    await writeFile(preexisting, 'v0');

    const events = await start();
    await wait(400);

    expect(events.some((e) => e.path === preexisting)).toBe(false);
  });

  it('stops firing after the handle is disposed', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustfswatch-'));
    const events = await start();

    handle?.dispose();
    handle = undefined;

    await writeFile(join(root, 'after-dispose.txt'), 'x');
    await wait(400);

    expect(events).toHaveLength(0);
  });

  it('applies the ignored callback option on top of the default .git filter', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustfswatch-'));
    const events: HostFsChange[] = [];
    const svc = new RustHostFsWatchService();
    handle = svc.watch(root, { ignored: (p) => p.includes('skipme') });
    handle.onDidChange((e) => events.push(e));
    await wait(300);

    await writeFile(join(root, 'skipme.txt'), 'x');
    await writeFile(join(root, 'keep.txt'), 'x');
    await wait(400);

    expect(events.some((e) => e.path.endsWith('skipme.txt'))).toBe(false);
    expect(events.some((e) => e.path.endsWith('keep.txt'))).toBe(true);
  });
});
