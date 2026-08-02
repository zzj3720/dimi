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
    input?: string | Buffer;
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
        // `code ?? -1` — HostProcess semantics (signal-killed → -1).
        exitCode: code ?? -1,
        pid: child.pid ?? -1,
      });
    });
    if (options.input !== undefined) {
      child.stdin.write(options.input);
      child.stdin.end();
    }
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
    input?: string | Buffer;
  } = {},
): Promise<RustResult> {
  const handle: RustHostProcessHandle = await rustHostProcessSpawn(command, args, options);
  if (options.input !== undefined) {
    // `new Uint8Array(string)` yields an EMPTY array in Node ≥22 (string is
    // not an array-like); Buffer.from does the byte conversion.
    handle.writeStdin(Buffer.from(options.input));
    handle.closeStdin();
  }
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

  test('stdin roundtrip reaches the child', async () => {
    const ts = await tsSpawn('cat', [], { input: 'ping\n' });
    const rust = await rustSpawn('cat', [], { input: 'ping\n' });
    expect(rust.stdout).toEqual(ts.stdout);
    expect(rust.stdout.toString()).toBe('ping\n');
    expect(rust.exitCode).toBe(ts.exitCode);
    expect(rust.exitCode).toBe(0);
  });

  test('kill terminates the tree and resolves -1 on both sides', async () => {
    // TS side: detached sleep, SIGTERM after 100ms → exit code null → -1.
    const tsExit = await new Promise<number | null>((resolve, reject) => {
      const child = nodeSpawn('sleep', ['30'], {
        detached: true,
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      child.on('error', reject);
      child.on('exit', (code) => resolve(code));
      setTimeout(() => child.kill('SIGTERM'), 100);
    });
    // Rust side: same shape through the bridge.
    const handle = await rustHostProcessSpawn('sleep', ['30']);
    handle.setStreamCallbacks(
      () => {},
      () => {},
      () => {},
      () => {},
    );
    setTimeout(() => handle.kill('SIGTERM'), 100);
    const rustExit = await handle.wait();
    expect(tsExit ?? -1).toBe(-1);
    expect(rustExit).toBe(-1);
    expect(handle.exitCode).toBe(-1);
  });
});
