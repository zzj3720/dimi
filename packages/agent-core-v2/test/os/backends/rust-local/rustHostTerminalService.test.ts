/**
 * `terminal` domain (L6) — integration test against the real Rust pty
 * (portable-pty via the napi bridge) on the local shell.
 */

import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { RustHostTerminalService } from '#/os/backends/rust-local/rustHostTerminalService';
import type { TerminalProcess } from '#/os/interface/terminal';

const SHELL = process.env.SHELL ?? '/bin/sh';
const wait = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

describe('RustHostTerminalService', () => {
  let root: string;
  let service: RustHostTerminalService | undefined;
  let process: TerminalProcess | undefined;

  afterEach(async () => {
    process?.kill();
    process = undefined;
    service?.dispose();
    service = undefined;
    if (root) await rm(root, { recursive: true, force: true });
  });

  async function start(): Promise<{ data: string; exit: { exitCode: number | null } | null }> {
    const collected: { data: string; exit: { exitCode: number | null } | null } = {
      data: '',
      exit: null,
    };
    service = new RustHostTerminalService();
    process = await service.spawn({
      cwd: root,
      shell: SHELL,
      cols: 80,
      rows: 24,
    });
    process.onProcessData((chunk) => {
      collected.data += chunk;
    });
    process.onProcessExit((event) => {
      collected.exit = event;
    });
    return collected;
  }

  it('spawns a shell, writes input and receives output', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustterm-'));
    const collected = await start();
    process!.write('echo rust-term-hello\n');
    await wait(1500);
    expect(collected.data).toContain('rust-term-hello');
  });

  it('reports the exit code', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustterm-'));
    const collected = await start();
    process!.write('exit 7\n');
    const deadline = Date.now() + 4000;
    while (collected.exit === null && Date.now() < deadline) {
      await wait(50);
    }
    expect(collected.exit?.exitCode).toBe(7);
  });

  it('resize does not throw', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustterm-'));
    await start();
    expect(() => process!.resize(120, 40)).not.toThrow();
    await wait(200);
  });

  it('kill terminates the shell', async () => {
    root = await mkdtemp(join(tmpdir(), 'rustterm-'));
    const collected = await start();
    process!.write('sleep 30\n');
    await wait(400);
    process!.kill();
    const deadline = Date.now() + 4000;
    while (collected.exit === null && Date.now() < deadline) {
      await wait(50);
    }
    expect(collected.exit).not.toBeNull();
    expect(collected.exit?.exitCode).toBeNull();
  });
});
