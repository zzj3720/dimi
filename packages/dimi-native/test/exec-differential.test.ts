/**
 * M2 process-slice differential suite: Node `child_process.spawn` semantics
 * vs the Rust `dimi-exec` bridge.
 *
 * The TS baseline mirrors `HostProcessService`'s observable contract
 * (spawn → pid / stdout / stderr / exit code / kill / env / cwd / shell);
 * the Rust side runs the same inputs through `rustHostProcessSpawn`. Outputs
 * must match byte-for-byte and exit codes exactly.
 *
 * Skips itself when the native binding is not built (same policy as the
 * other suites).
 */
import { spawn as nodeSpawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

import { rustHostProcessSpawn, type RustHostProcessHandle } from '#/index';

const bindingPath = fileURLToPath(new URL('../dist/dimi_bridge.node', import.meta.url));
const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

interface TsResult {
  stdout: Buffer;
  stderr: Buffer;
  exitCode: number | null;
  pid: number;
}

/** TS baseline: `spawn` + stream collection, mirroring HostProcess semantics. */
function tsSpawn(
  command: string,
  args: readonly string[] = [],
  options: {
    cwd?: string;
    env?: Record<string, string>;
    shell?: boolean | string;
    detached?: boolean;
  } = {},
): Promise<TsResult> {
  return new Promise((resolve, reject) => {
    const child = nodeSpawn(command, [...args], {
      cwd: options.cwd,
      env: options.env,
      shell: options.shell,
      detached: options.detached ?? (process.platform !== 'win32'),
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    child.stdout.on('data', (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on('data', (chunk: Buffer) => stderr.push(chunk));
    child.on('error', reject);
    child.on('exit', (code) => {
      resolve({
        stdout: Buffer.concat(stdout),
        stderr: Buffer.concat(stderr),
        exitCode: code,
        pid: child.pid ?? -1,
      });
    });
  });
}

interface RustResult {
  stdout: Buffer;
  stderr: Buffer;
  exitCode: number | null;
  pid: number;
}

/** Rust bridge: spawn + pump callbacks + wait. */
async function rustSpawn(
  command: string,
  args: readonly string[] = [],
  options: {
    cwd?: string;
    env?: Record<string, string>;
    shellDefault?: boolean;
    shellPath?: string;
    detached?: boolean;
  } = {},
): Promise<RustResult> {
  const handle: RustHostProcessHandle = await rustHostProcessSpawn(command, args, options);
  const stdout: Buffer[] = [];
  const stderr: Buffer[] = [];
  await new Promise<void>((resolve) => {
    let ended = 0;
    handle.setStreamCallbacks(
      (chunk) => stdout.push(Buffer.from(chunk)),
      (chunk) => stderr.push(Buffer.from(chunk)),
      () => {
        ended += 1;
        if (ended === 2) resolve();
      },
      () => {
        ended += 1;
        if (ended === 2) resolve();
      },
    );
  });
  const exitCode = await handle.wait();
  return { stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr), exitCode, pid: handle.pid };
}

suite('exec: TS child_process vs Rust dimi-exec', () => {
  test('captures stdout and exit code', async () => {
    const ts = await tsSpawn('echo', ['hello dimi']);
    const rust = await rustSpawn('echo', ['hello dimi']);
    expect(rust.stdout).toEqual(ts.stdout);
    expect(rust.stderr).toEqual(ts.stderr);
    expect(rust.exitCode).toBe(ts.exitCode);
    expect(rust.pid).toBeGreaterThan(0);
  });

  test('separates stderr from stdout', async () => {
    const args = ['-c', 'echo out; echo err >&2'];
    const ts = await tsSpawn('sh', args);
    const rust = await rustSpawn('sh', args);
    expect(rust.stdout).toEqual(ts.stdout);
    expect(rust.stderr).toEqual(ts.stderr);
    expect(rust.exitCode).toBe(ts.exitCode);
  });

  test('propagates nonzero exit codes', async () => {
    const args = ['-c', 'exit 7'];
    const ts = await tsSpawn('sh', args);
    const rust = await rustSpawn('sh', args);
    expect(rust.exitCode).toBe(7);
    expect(rust.exitCode).toBe(ts.exitCode);
  });

  test('env overrides overlay process.env', async () => {
    const args = ['-c', 'printf %s "$DIMI_EXEC_TEST"'];
    const env = { ...(process.env as Record<string, string>), DIMI_EXEC_TEST: '42' };
    const ts = await tsSpawn('sh', args, { env });
    const rust = await rustSpawn('sh', args, { env });
    expect(rust.stdout).toEqual(ts.stdout);
    expect(rust.stdout.toString()).toBe('42');
  });

  test('respects cwd', async () => {
    const ts = await tsSpawn('pwd', [], { cwd: '/tmp' });
    const rust = await rustSpawn('pwd', [], { cwd: '/tmp' });
    expect(rust.stdout.toString()).toBe(ts.stdout.toString());
  });

  test('shell: true runs through the default shell', async () => {
    const ts = await tsSpawn('echo shell-ok', [], { shell: true });
    const rust = await rustSpawn('echo shell-ok', [], { shellDefault: true });
    expect(rust.stdout).toEqual(ts.stdout);
    expect(rust.exitCode).toBe(ts.exitCode);
  });

  test('missing command fails spawn on both sides', async () => {
    await expect(tsSpawn('definitely-not-a-command-xyz')).rejects.toThrow();
    await expect(
      rustHostProcessSpawn('definitely-not-a-command-xyz', []),
    ).rejects.toThrow(/Failed to spawn/);
  });
});
