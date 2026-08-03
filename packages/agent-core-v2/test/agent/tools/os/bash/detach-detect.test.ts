/**
 * `detectDetachedProcesses` / `formatDetachedProcessNotice` tests — the
 * "command left processes outside dimi control" detection used by BashTool.
 *
 * The detection is inherently OS-dependent: it probes a session/process
 * group via `kill(-pid, 0)` and lists survivors with `ps -g`. Pure logic
 * (platform guards, notice text) is unit-tested; the real detach scenario
 * is an integration test that spawns a `setsid`-style shell (the same way
 * dimi spawns commands) with a `nohup … &` survivor and verifies the
 * detector finds it on Unix. On Windows the detector is a no-op, so the
 * integration test is skipped there.
 */

import { spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';

import { afterAll, describe, expect, it, vi } from 'vitest';

import {
  detectDetachedProcesses,
  formatDetachedProcessNotice,
  type DetachedProcessInfo,
} from '#/agent/tools/os/bash/bashTool';

const spawned: ChildProcess[] = [];
afterAll(() => {
  for (const child of spawned) {
    try {
      child.kill('SIGKILL');
    } catch {
      // already gone
    }
  }
});

const isUnix = process.platform !== 'win32';

describe('detectDetachedProcesses', () => {
  it('returns [] on win32 (no session/process-group kill semantics)', async () => {
    const platform = vi.spyOn(process, 'platform', 'get').mockReturnValue('win32');
    try {
      await expect(detectDetachedProcesses(1234)).resolves.toEqual([]);
    } finally {
      platform.mockRestore();
    }
  });

  it('returns [] for a non-positive or non-integer pid', async () => {
    await expect(detectDetachedProcesses(0)).resolves.toEqual([]);
    await expect(detectDetachedProcesses(-1)).resolves.toEqual([]);
    await expect(detectDetachedProcesses(Number.NaN)).resolves.toEqual([]);
  });

  it('returns [] when the session has no survivors (ESRCH)', async () => {
    // Pick a pid that is very unlikely to exist and probe it.
    await expect(detectDetachedProcesses(2_147_483_647)).resolves.toEqual([]);
  });

  it.skipIf(!isUnix)(
    'detects a nohup survivor that outlived the command shell',
    async () => {
      // dimi spawns the command shell in its own session (setsid, pid ==
      // sid == pgid). Reproduce that: spawn sh detached and have it launch
      // `nohup sleep 30 &` then exit.
      const child = spawn('sh', ['-c', 'nohup sleep 30 &'], {
        detached: true,
        stdio: 'ignore',
      });
      spawned.push(child);
      const [code] = (await once(child, 'exit')) as [number | null];
      expect(code).toBe(0);

      // Give the survivor a moment to be reparented (ppid -> 1) while still
      // in the same session/process group.
      await new Promise((resolve) => setTimeout(resolve, 300));

      const infos = await detectDetachedProcesses(child.pid!);
      expect(infos.length).toBeGreaterThan(0);
      const sleep = infos.find((info) => info.command.includes('sleep 30'));
      expect(sleep).toBeDefined();
      expect(sleep!.ppid).toBe(1);

      // Clean up the survivor session.
      try {
        process.kill(-child.pid!, 'SIGKILL');
      } catch {
        // already gone
      }
    },
  );
});

describe('formatDetachedProcessNotice', () => {
  it('lists the detached processes and explains the dimi gap', () => {
    const infos: readonly DetachedProcessInfo[] = [
      { pid: 4242, ppid: 1, command: 'node server.js' },
    ];
    const text = formatDetachedProcessNotice(infos);
    expect(text).toContain('pid 4242');
    expect(text).toContain('node server.js');
    expect(text).toContain('outside dimi control');
    expect(text).toContain('run_in_background=true');
  });

  it('renders multiple processes as separate lines', () => {
    const text = formatDetachedProcessNotice([
      { pid: 1001, ppid: 0, command: 'a' },
      { pid: 1002, ppid: 1001, command: 'b' },
    ]);
    expect(text.match(/- pid 100\d/g)).toHaveLength(2);
  });
});
