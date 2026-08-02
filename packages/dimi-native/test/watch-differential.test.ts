/**
 * M2 watch-slice differential suite: chokidar (the node-local backend)
 * event surface vs the Rust `dimi-exec` watch bridge (`RustFsWatch`).
 *
 * Both watchers run on the same temp tree; the assertions compare the
 * normalized event sets: every chokidar event must have a Rust counterpart
 * with the same path/action/kind (the Rust side may emit fewer duplicates).
 *
 * Skips itself when the native binding is not built (same policy as the
 * other suites).
 */
import { watch as chokidarWatch } from 'chokidar';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { mkdir, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, test } from 'vitest';

import { rustFsWatch, type RustFsChange, type RustFsWatchHandle } from '#/index';

const bindingPath = new URL('../dist/dimi_bridge.node', import.meta.url);
const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

const settle = (ms = 400): Promise<void> => new Promise((r) => setTimeout(r, ms));

interface WatchedChange {
  path: string;
  action: string;
  kind: string;
}

function collectRust(handle: RustFsWatchHandle, ms: number): Promise<WatchedChange[]> {
  return new Promise((resolve) => {
    const changes: WatchedChange[] = [];
    handle.setOnChange((change: RustFsChange) => {
      changes.push({ path: change.path, action: change.action, kind: change.kind });
    });
    setTimeout(() => resolve(changes), ms);
  });
}

async function collectChokidar(
  root: string,
  ms: number,
  options?: { depth?: number },
): Promise<WatchedChange[]> {
  const changes: WatchedChange[] = [];
  const watcher = chokidarWatch(root, {
    ignoreInitial: true,
    persistent: false,
    followSymlinks: false,
    depth: options?.depth,
    ignored: (p: string) => /(?:^|[/\\])\.git(?:$|[/\\])/.test(p),
  });
  watcher.on('all', (eventName: string, absPath: string) => {
    const mapped = mapEvent(eventName);
    if (mapped !== undefined) changes.push({ path: absPath, action: mapped.action, kind: mapped.kind });
  });
  await new Promise<void>((resolve) => watcher.on('ready', () => resolve()));
  await new Promise((r) => setTimeout(r, ms));
  await watcher.close();
  return changes;
}

function mapEvent(eventName: string): { action: string; kind: string } | undefined {
  switch (eventName) {
    case 'add':
      return { action: 'created', kind: 'file' };
    case 'addDir':
      return { action: 'created', kind: 'directory' };
    case 'change':
      return { action: 'modified', kind: 'file' };
    case 'unlink':
      return { action: 'deleted', kind: 'file' };
    case 'unlinkDir':
      return { action: 'deleted', kind: 'directory' };
    default:
      return undefined;
  }
}

/** Every chokidar change must have a Rust counterpart (same path/action/kind). */
function expectCovered(rust: WatchedChange[], chokidar: WatchedChange[]): void {
  for (const expected of chokidar) {
    expect(
      rust.some(
        (c) => c.path === expected.path && c.action === expected.action && c.kind === expected.kind,
      ),
      `missing Rust event for ${expected.action}/${expected.kind} ${expected.path}\nrust: ${JSON.stringify(rust, null, 1)}\nchokidar: ${JSON.stringify(chokidar, null, 1)}`,
    ).toBe(true);
  }
}

suite('fs watch: chokidar vs Rust dimi-exec', () => {
  let dir: string;

  beforeAll(() => {
    dir = mkdtempSync(join(tmpdir(), 'dimi-watch-diff-'));
  });

  afterAll(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  test('create / modify / delete file', async () => {
    const file = join(dir, 'a.txt');
    const rust = rustFsWatch(dir);
    const chokidarP = collectChokidar(dir, 800);
    const rustP = collectRust(rust, 800);
    await settle(300);
    await writeFile(file, 'one');
    await settle(100);
    await writeFile(file, 'two');
    await settle(100);
    await rm(file);
    const [chokidarChanges, rustChanges] = await Promise.all([chokidarP, rustP]);
    rust.dispose();
    expectCovered(rustChanges, chokidarChanges);
  });

  test('create / delete directory (kind preserved)', async () => {
    const sub = join(dir, 'subdir');
    const rust = rustFsWatch(dir);
    const chokidarP = collectChokidar(dir, 800);
    const rustP = collectRust(rust, 800);
    await settle(300);
    await mkdir(sub);
    await settle(150);
    await rm(sub, { recursive: true });
    const [chokidarChanges, rustChanges] = await Promise.all([chokidarP, rustP]);
    rust.dispose();
    expectCovered(rustChanges, chokidarChanges);
  });

  test('rename maps to delete + create', async () => {
    const from = join(dir, 'old.txt');
    const to = join(dir, 'new.txt');
    await writeFile(from, 'x');
    const rust = rustFsWatch(dir);
    const chokidarP = collectChokidar(dir, 900);
    const rustP = collectRust(rust, 900);
    await settle(300);
    await rename(from, to);
    const [chokidarChanges, rustChanges] = await Promise.all([chokidarP, rustP]);
    rust.dispose();
    expectCovered(rustChanges, chokidarChanges);
  });

  test('.git paths are filtered like the default ignore', async () => {
    const gitDir = join(dir, '.git');
    await mkdir(gitDir, { recursive: true });
    const rust = rustFsWatch(dir);
    const chokidarP = collectChokidar(dir, 800);
    const rustP = collectRust(rust, 800);
    await settle(300);
    await writeFile(join(gitDir, 'config'), 'x');
    await writeFile(join(dir, 'visible.txt'), 'y');
    const [chokidarChanges, rustChanges] = await Promise.all([chokidarP, rustP]);
    rust.dispose();
    expect(rustChanges.every((c) => !c.path.includes('.git'))).toBe(true);
    expect(rustChanges.some((c) => c.path.endsWith('visible.txt'))).toBe(true);
    expect(chokidarChanges.some((c) => c.path.endsWith('visible.txt'))).toBe(true);
  });

  test('non-recursive watch does not descend into subdirectories', async () => {
    const sub = join(dir, 'nested');
    await mkdir(sub, { recursive: true });
    const rust = rustFsWatch(dir, { recursive: false });
    const chokidarP = collectChokidar(dir, 800, { depth: 0 });
    const rustP = collectRust(rust, 800);
    await settle(300);
    await writeFile(join(sub, 'deep.txt'), 'x');
    await writeFile(join(dir, 'top.txt'), 'y');
    const [chokidarChanges, rustChanges] = await Promise.all([chokidarP, rustP]);
    rust.dispose();
    expect(rustChanges.some((c) => c.path.endsWith('top.txt'))).toBe(true);
    expect(rustChanges.some((c) => c.path.endsWith('deep.txt'))).toBe(false);
    expect(chokidarChanges.some((c) => c.path.endsWith('top.txt'))).toBe(true);
    expect(chokidarChanges.some((c) => c.path.endsWith('deep.txt'))).toBe(false);
  });
});
