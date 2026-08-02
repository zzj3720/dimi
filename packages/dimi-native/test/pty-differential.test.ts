/**
 * M2 pty differential — node-pty (TS status quo) vs the Rust pty bridge
 * (`dimi-exec::pty` via napi) over the same shell scenarios. Both backends
 * must produce the same observable behavior for the `IHostTerminalService`
 * surface: output, exit codes, cwd, resize, env and kill.
 */

import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';
import * as pty from 'node-pty';

import { rustTerminalSpawn } from '../src/index';

const SHELL = process.env['SHELL'] ?? '/bin/sh';
const wait = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

interface Collected {
  data: string;
  exitCode: number | null | undefined;
  exited: boolean;
}

/** Drive a node-pty process through a script; return collected output + exit. */
async function collectNodePty(options: {
  cwd: string;
  cols: number;
  rows: number;
  env?: Record<string, string>;
  script?: string;
  resizeTo?: { cols: number; rows: number };
  killAfterMs?: number;
}): Promise<Collected> {
  const proc = pty.spawn(SHELL, [], {
    name: 'xterm-256color',
    cwd: options.cwd,
    cols: options.cols,
    rows: options.rows,
    env: options.env ?? process.env as Record<string, string>,
  });
  const collected: Collected = { data: '', exitCode: undefined, exited: false };
  proc.onData((chunk) => {
    collected.data += chunk;
  });
  const exitPromise = new Promise<void>((resolve) => {
    proc.onExit((event) => {
      collected.exitCode = event.exitCode;
      collected.exited = true;
      resolve();
    });
  });
  if (options.resizeTo) proc.resize(options.resizeTo.cols, options.resizeTo.rows);
  if (options.script) proc.write(options.script);
  if (options.killAfterMs !== undefined) {
    await wait(options.killAfterMs);
    proc.kill();
  }
  await Promise.race([exitPromise, wait(4000)]);
  return collected;
}

/** Drive the Rust pty bridge through the same script. */
async function collectRustPty(options: {
  cwd: string;
  cols: number;
  rows: number;
  env?: Record<string, string>;
  script?: string;
  resizeTo?: { cols: number; rows: number };
  killAfterMs?: number;
}): Promise<Collected> {
  const proc = rustTerminalSpawn({
    cwd: options.cwd,
    shell: SHELL,
    cols: options.cols,
    rows: options.rows,
    env: options.env ?? (process.env as Record<string, string>),
  });
  const collected: Collected = { data: '', exitCode: undefined, exited: false };
  proc.setOnData((chunk) => {
    collected.data += chunk;
  });
  const exitPromise = new Promise<void>((resolve) => {
    proc.setOnExit((event) => {
      // napi serializes `None` as `undefined`, not `null` — normalize.
      collected.exitCode = event.exitCode ?? null;
      collected.exited = true;
      resolve();
    });
  });
  if (options.resizeTo) proc.resize(options.resizeTo.cols, options.resizeTo.rows);
  if (options.script) proc.write(options.script);
  if (options.killAfterMs !== undefined) {
    await wait(options.killAfterMs);
    proc.kill();
  }
  await Promise.race([exitPromise, wait(4000)]);
  return collected;
}

describe('pty differential: node-pty vs Rust', () => {
  it('echo output arrives on both backends', async () => {
    const cwd = await mkdtemp(join(tmpdir(), 'pty-diff-echo-'));
    const [node, rust] = await Promise.all([
      collectNodePty({ cwd, cols: 80, rows: 24, script: 'echo pty-diff-hello\n' }),
      collectRustPty({ cwd, cols: 80, rows: 24, script: 'echo pty-diff-hello\n' }),
    ]);
    expect(node.data).toContain('pty-diff-hello');
    expect(rust.data).toContain('pty-diff-hello');
    await rm(cwd, { recursive: true, force: true });
  });

  it('exit code 42 propagates on both backends', async () => {
    const cwd = await mkdtemp(join(tmpdir(), 'pty-diff-exit-'));
    const [node, rust] = await Promise.all([
      collectNodePty({ cwd, cols: 80, rows: 24, script: 'exit 42\n' }),
      collectRustPty({ cwd, cols: 80, rows: 24, script: 'exit 42\n' }),
    ]);
    expect(node.exitCode).toBe(42);
    expect(rust.exitCode).toBe(42);
    await rm(cwd, { recursive: true, force: true });
  });

  it('cwd is respected on both backends', async () => {
    const cwd = await mkdtemp(join(tmpdir(), 'pty-diff-cwd-'));
    const [node, rust] = await Promise.all([
      collectNodePty({ cwd, cols: 80, rows: 24, script: 'pwd\n' }),
      collectRustPty({ cwd, cols: 80, rows: 24, script: 'pwd\n' }),
    ]);
    expect(node.data).toContain(cwd);
    expect(rust.data).toContain(cwd);
    await rm(cwd, { recursive: true, force: true });
  });

  it('resize reaches the shell on both backends', async () => {
    const cwd = await mkdtemp(join(tmpdir(), 'pty-diff-resize-'));
    const [node, rust] = await Promise.all([
      collectNodePty({ cwd, cols: 80, rows: 24, resizeTo: { cols: 100, rows: 40 }, script: 'stty size\n' }),
      collectRustPty({ cwd, cols: 80, rows: 24, resizeTo: { cols: 100, rows: 40 }, script: 'stty size\n' }),
    ]);
    expect(node.data).toMatch(/(?:^|\n)\s*40 100/);
    expect(rust.data).toMatch(/(?:^|\n)\s*40 100/);
    await rm(cwd, { recursive: true, force: true });
  });

  it('env is forwarded on both backends', async () => {
    const cwd = await mkdtemp(join(tmpdir(), 'pty-diff-env-'));
    const env = { ...(process.env as Record<string, string>), DIMI_PTY_DIFF: 'shared-env' };
    const [node, rust] = await Promise.all([
      collectNodePty({ cwd, cols: 80, rows: 24, env, script: 'echo $DIMI_PTY_DIFF\n' }),
      collectRustPty({ cwd, cols: 80, rows: 24, env, script: 'echo $DIMI_PTY_DIFF\n' }),
    ]);
    expect(node.data).toContain('shared-env');
    expect(rust.data).toContain('shared-env');
    await rm(cwd, { recursive: true, force: true });
  });

  it('kill terminates the shell on both backends', async () => {
    const cwd = await mkdtemp(join(tmpdir(), 'pty-diff-kill-'));
    const [node, rust] = await Promise.all([
      collectNodePty({ cwd, cols: 80, rows: 24, script: 'sleep 30\n', killAfterMs: 500 }),
      collectRustPty({ cwd, cols: 80, rows: 24, script: 'sleep 30\n', killAfterMs: 500 }),
    ]);
    // node-pty reports a numeric code for signal kills (129 = 128+SIGHUP on
    // macOS); the Rust bridge reports `null` (no exit code for a signal).
    expect(node.exited).toBe(true);
    expect(rust.exited).toBe(true);
    expect(typeof node.exitCode).toBe('number');
    expect(rust.exitCode).toBeNull();
    await rm(cwd, { recursive: true, force: true });
  });
});
