/**
 * M2 fs-slice differential suite: `node:fs/promises` semantics vs the Rust
 * `dimi-exec` bridge (`RustFileSystem`).
 *
 * Every `IHostFileSystem` method runs the same inputs through both sides on
 * the same temp tree; outputs must match byte-for-byte. Error paths compare
 * the Node errno symbol (`ENOENT`, …) against the bridge's `{ERRNO} …`
 * message prefix — the TS adapter maps both through `toHostFsError`.
 *
 * Skips itself when the native binding is not built (same policy as the
 * other suites).
 */
import { existsSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { promises as fsp } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, test } from 'vitest';

import { RustFileSystem } from '#/index';

const bindingPath = new URL('../dist/dimi_bridge.node', import.meta.url);
const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

/** Extract the leading errno symbol from a bridge error message. */
function bridgeErrno(error: unknown): string | null {
  const match = /^([A-Z0-9_]+) /.exec((error as Error).message ?? '');
  return match?.[1] ?? null;
}

async function nodeErrno(fn: () => Promise<unknown>): Promise<string | undefined> {
  try {
    await fn();
    return undefined;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code;
  }
}

async function bridgeErrnoOf(fn: () => Promise<unknown>): Promise<string | null> {
  try {
    await fn();
    return null;
  } catch (error) {
    return bridgeErrno(error);
  }
}

suite('fs: TS node:fs/promises vs Rust dimi-exec', () => {
  let dir: string;

  beforeAll(() => {
    dir = mkdtempSync(join(tmpdir(), 'dimi-fs-diff-'));
  });

  afterAll(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  const p = (name: string): string => join(dir, name);

  test('readText/writeText/appendText roundtrip', async () => {
    const file = p('roundtrip.txt');
    await fsp.writeFile(file, 'hello', 'utf8');
    await RustFileSystem.writeText(file, 'hello');
    expect(await RustFileSystem.readText(file)).toBe(await fsp.readFile(file, 'utf8'));
    await RustFileSystem.appendText(file, ' world');
    await fsp.appendFile(file, ' world', 'utf8');
    expect(await RustFileSystem.readText(file)).toBe(await fsp.readFile(file, 'utf8'));
  });

  test('readText with options: BOM and strict errors', async () => {
    const file = p('bom.txt');
    writeFileSync(file, Buffer.from([0xef, 0xbb, 0xbf, 0x41, 0x80]));
    // With options → TextDecoder semantics: BOM stripped, strict throws.
    await expect(RustFileSystem.readText(file, { errors: 'strict' })).rejects.toThrow();
    await expect(fsp.readFile(file, 'utf8')).resolves.toBe('\ufeffA\ufffd');
    // replace: BOM stripped, invalid → U+FFFD (TextDecoder).
    const tsReplace = new TextDecoder('utf-8', { fatal: false }).decode(
      await fsp.readFile(file),
    );
    expect(await RustFileSystem.readText(file, { errors: 'replace' })).toBe(tsReplace);
  });

  test('readBytes whole and partial', async () => {
    const file = p('bytes.bin');
    writeFileSync(file, Buffer.from('0123456789'));
    const tsFull = new Uint8Array(await fsp.readFile(file));
    expect(new Uint8Array(await RustFileSystem.readBytes(file))).toEqual(tsFull);
    const tsPart = (await fsp.open(file, 'r')).read(Buffer.alloc(4), 0, 4, 0);
    const fh = await fsp.open(file, 'r');
    const { buffer, bytesRead } = await fh.read(Buffer.alloc(4), 0, 4, 0);
    await fh.close();
    expect(new Uint8Array(await RustFileSystem.readBytes(file, 4))).toEqual(
      new Uint8Array(buffer.subarray(0, bytesRead)),
    );
  });

  test('readBytes partial shorter than n', async () => {
    const file = p('short.bin');
    writeFileSync(file, Buffer.from('ab'));
    expect(new Uint8Array(await RustFileSystem.readBytes(file, 100))).toEqual(
      new Uint8Array(await fsp.readFile(file)),
    );
  });

  test('readLines matches node readFile split, incl. cross-chunk lines', async () => {
    const file = p('lines.txt');
    let content = '';
    for (let i = 0; i < 200_000; i += 1) {
      content += `line${i}\n`;
    }
    content += 'tail-no-newline';
    writeFileSync(file, content, 'utf8');
    const tsLines = (await fsp.readFile(file, 'utf8')).split(/(?<=\n)/);
    if (tsLines[tsLines.length - 1] === '') tsLines.pop();
    const rust: string[] = [];
    const handle = RustFileSystem.readLines(file);
    for (;;) {
      const line = await handle.next();
      if (line === null) break;
      rust.push(line);
    }
    expect(rust).toEqual(tsLines);
  });

  test('readLines BOM stripped on the first line only', async () => {
    const file = p('bom-lines.txt');
    writeFileSync(file, Buffer.from([0xef, 0xbb, 0xbf, 0x41, 0x0a, 0x42]));
    const tsText = await fsp.readFile(file, 'utf8'); // keeps BOM
    const tsLines = tsText.split(/(?<=\n)/);
    const rust: string[] = [];
    const handle = RustFileSystem.readLines(file);
    for (;;) {
      const line = await handle.next();
      if (line === null) break;
      rust.push(line);
    }
    // Rust follows decodeTextWithErrors: BOM stripped on the first line,
    // so it differs from readFile('utf8') on the first line only.
    expect(rust[0]).toBe('A\n');
    expect(rust.slice(1)).toEqual(tsLines.slice(1));
  });

  test('readLines strict rejects invalid utf-8 like TextDecoder', async () => {
    const file = p('bad-utf8.txt');
    writeFileSync(file, Buffer.from([0x80, 0x0a]));
    const handle = RustFileSystem.readLines(file, { errors: 'strict' });
    await expect(handle.next()).rejects.toThrow();
    // Node TextDecoder fatal throws too (synchronous):
    expect(() =>
      new TextDecoder('utf-8', { fatal: true }).decode(Buffer.from([0x80, 0x0a])),
    ).toThrow();
  });

  test('readLines non-utf8 encoding materializes the file', async () => {
    const file = p('latin1.txt');
    writeFileSync(file, Buffer.from([0x41, 0x0a, 0xe9, 0x0a])); // A\né\n
    const ts = (await fsp.readFile(file, 'latin1')).split(/(?<=\n)/);
    const rust: string[] = [];
    const handle = RustFileSystem.readLines(file, { encoding: 'latin1' });
    for (;;) {
      const line = await handle.next();
      if (line === null) break;
      rust.push(line);
    }
    expect(rust).toEqual(ts);
  });

  test('createExclusive: first true, second false on both sides', async () => {
    // Rust side: first creates, second returns false.
    const rustFile = p('exclusive-rust.txt');
    const data = Buffer.from('payload');
    expect(await RustFileSystem.createExclusive(rustFile, data)).toBe(true);
    expect(await RustFileSystem.createExclusive(rustFile, data)).toBe(false);
    expect(await fsp.readFile(rustFile)).toEqual(data);

    // Node side: first creates, second rejects EEXIST.
    const nodeFile = p('exclusive-node.txt');
    const fh = await fsp.open(nodeFile, 'wx');
    await fh.writeFile(data);
    await fh.close();
    const secondNode = await nodeErrno(async () => {
      const h = await fsp.open(nodeFile, 'wx');
      await h.close();
    });
    expect(secondNode).toBe('EEXIST');
    // Cross-check: Rust refuses a file Node created.
    expect(await RustFileSystem.createExclusive(nodeFile, data)).toBe(false);
    // And Node refuses a file Rust created.
    const rustCreated = await nodeErrno(async () => {
      const h = await fsp.open(rustFile, 'wx');
      await h.close();
    });
    expect(rustCreated).toBe('EEXIST');
  });

  test('stat/lstat follow vs not-follow symlinks', async () => {
    const target = p('target.txt');
    const link = p('link.txt');
    writeFileSync(target, 'hello');
    symlinkSync(target, link);

    const tsStat = await fsp.stat(link);
    const rsStat = await RustFileSystem.stat(link);
    expect(rsStat.isFile).toBe(tsStat.isFile());
    expect(rsStat.isDirectory).toBe(tsStat.isDirectory());
    expect(rsStat.isSymbolicLink).toBe(tsStat.isSymbolicLink());
    expect(rsStat.size).toBe(tsStat.size);

    const tsLstat = await fsp.lstat(link);
    const rsLstat = await RustFileSystem.lstat(link);
    expect(rsLstat.isSymbolicLink).toBe(true);
    expect(rsLstat.isFile).toBe(tsLstat.isFile());
    expect(rsLstat.size).toBe(tsLstat.size);
  });

  test('readdir entry flags', async () => {
    const sub = p('subdir');
    await fsp.mkdir(sub);
    writeFileSync(p('f.txt'), 'x');
    const tsEntries = await fsp.readdir(dir, { withFileTypes: true });
    const rsEntries = await RustFileSystem.readdir(dir);
    const tsByName = new Map(tsEntries.map((e) => [e.name, e]));
    for (const entry of rsEntries) {
      const ts = tsByName.get(entry.name);
      expect(ts, `entry ${entry.name}`).toBeDefined();
      expect(entry.isFile).toBe(ts!.isFile());
      expect(entry.isDirectory).toBe(ts!.isDirectory());
      expect(entry.isSymbolicLink).toBe(ts!.isSymbolicLink());
    }
    expect(rsEntries.length).toBe(tsEntries.length);
  });

  test('mkdir recursive and non-recursive agree', async () => {
    const nested = p('a/b');
    const tsNonRecursive = await nodeErrno(() => fsp.mkdir(nested));
    const rsNonRecursive = await bridgeErrnoOf(() => RustFileSystem.mkdir(nested));
    expect(rsNonRecursive).toBe(tsNonRecursive ?? null);
    await RustFileSystem.mkdir(nested, true);
    await fsp.mkdir(nested, { recursive: true });
    const tsExists = await nodeErrno(() => fsp.mkdir(nested));
    const rsExists = await bridgeErrnoOf(() => RustFileSystem.mkdir(nested));
    expect(rsExists).toBe(tsExists ?? null);
    expect(rsExists).toBe('EEXIST');
  });

  test('remove file, dir and missing agree', async () => {
    const file = p('rm-file.txt');
    writeFileSync(file, 'x');
    await RustFileSystem.remove(file);
    expect(existsSync(file)).toBe(false);

    const tree = p('rm-tree');
    await fsp.mkdir(join(tree, 'deep'), { recursive: true });
    writeFileSync(join(tree, 'deep', 'x'), 'y');
    await RustFileSystem.remove(tree);
    expect(existsSync(tree)).toBe(false);

    // force: missing is Ok on both sides.
    await expect(RustFileSystem.remove(p('never-existed'))).resolves.toBeUndefined();
    await expect(fsp.rm(p('never-existed-2'), { recursive: true, force: true })).resolves.toBeUndefined();
  });

  test('realpath resolves symlinks', async () => {
    const target = p('real-target.txt');
    const link = p('real-link.txt');
    writeFileSync(target, 'x');
    symlinkSync(target, link);
    const tsReal = await fsp.realpath(link);
    const rsReal = await RustFileSystem.realpath(link);
    expect(rsReal).toBe(tsReal);
  });

  test('error errno symbols match node codes', async () => {
    const missing = p('does-not-exist.txt');
    const tsNotFound = await nodeErrno(() => fsp.readFile(missing));
    const rsNotFound = await bridgeErrnoOf(() => RustFileSystem.readText(missing));
    expect(rsNotFound).toBe(tsNotFound ?? null);
    expect(rsNotFound).toBe('ENOENT');

    // readdir on a file → ENOTDIR.
    const file = p('not-a-dir.txt');
    writeFileSync(file, 'x');
    const tsNotDir = await nodeErrno(() => fsp.readdir(file));
    const rsNotDir = await bridgeErrnoOf(() => RustFileSystem.readdir(file));
    expect(rsNotDir).toBe(tsNotDir ?? null);
    expect(rsNotDir).toBe('ENOTDIR');
  });
});
