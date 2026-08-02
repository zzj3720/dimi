/**
 * `hostProcess` domain (L1) — integration test for the Rust-backed
 * `RustHostProcessService` (M2 slice 1; the default backend since the legacy flip).
 *
 * Exercises the adapter contract through the real napi bridge: stream
 * wiring (incl. `mergeStderr` aliasing), error wrapping, `wait()` caching
 * and `kill()`. Skips nothing — the native binding is a dev dependency of
 * this swap-in path and the suite fails loudly when it is missing.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { Readable } from 'node:stream';

import { DisposableStore } from '#/_base/di/lifecycle';
import { createServices, type TestInstantiationService } from '#/_base/di/test';
import {
  HostProcessError,
  HostProcessErrorCode,
  IHostProcessService,
} from '#/os/interface/hostProcess';
import { RustHostProcessService } from '#/os/backends/rust-local/rustHostProcessService';

async function collect(stream: Readable): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of stream) {
    chunks.push(chunk as Buffer);
  }
  return Buffer.concat(chunks).toString('utf8');
}

describe('RustHostProcessService', () => {
  let disposables: DisposableStore;
  let ix: TestInstantiationService;

  beforeEach(() => {
    disposables = new DisposableStore();
    ix = createServices(disposables, {
      additionalServices: (reg) => {
        reg.define(IHostProcessService, RustHostProcessService);
      },
    });
  });

  afterEach(() => {
    disposables.dispose();
  });

  it('spawns a process and captures stdout + exit code', async () => {
    const svc = ix.get(IHostProcessService);
    const proc = await svc.spawn('node', ['-e', 'process.stdout.write("ok")']);
    const out = await collect(proc.stdout);
    expect(out).toBe('ok');
    expect(await proc.wait()).toBe(0);
    expect(proc.exitCode).toBe(0);
  });

  it('merges stderr into the stdout view when mergeStderr is set', async () => {
    const svc = ix.get(IHostProcessService);
    const proc = await svc.spawn('sh', ['-c', 'echo out; echo err >&2'], {
      mergeStderr: true,
    });
    // Alias, exactly like the node-local backend: one stream, stderr pipe
    // data is not consumed.
    expect(proc.stderr).toBe(proc.stdout);
    const out = await collect(proc.stdout);
    expect(await proc.wait()).toBe(0);
    expect(out).toContain('out');
    expect(out).not.toContain('err');
  });

  it('keeps stderr separate by default', async () => {
    const svc = ix.get(IHostProcessService);
    const proc = await svc.spawn('sh', ['-c', 'echo out; echo err >&2']);
    expect(proc.stderr).not.toBe(proc.stdout);
    const [out, err] = await Promise.all([collect(proc.stdout), collect(proc.stderr)]);
    expect(await proc.wait()).toBe(0);
    expect(out).toBe('out\n');
    expect(err).toBe('err\n');
  });

  it('wraps spawn failures in a coded HostProcessError without double prefix', async () => {
    const svc = ix.get(IHostProcessService);
    await expect(svc.spawn('definitely-not-a-real-command-42')).rejects.toSatisfy(
      (err: unknown) => {
        expect(err).toBeInstanceOf(HostProcessError);
        const error = err as HostProcessError;
        expect(error.code).toBe(HostProcessErrorCode.SpawnFailed);
        expect(error.message).toContain('Failed to spawn "definitely-not-a-real-command-42"');
        // The bridge message already carries the prefix; the adapter must
        // not re-wrap it.
        expect(error.message).not.toContain(
          'Failed to spawn "definitely-not-a-real-command-42": Failed to spawn',
        );
        expect(error.cause).toBeInstanceOf(Error);
        return true;
      },
    );
  });

  it('terminates a running process with kill()', async () => {
    const svc = ix.get(IHostProcessService);
    const proc = await svc.spawn('node', ['-e', 'setTimeout(() => {}, 30000)']);
    expect(proc.pid).toBeGreaterThan(0);
    await proc.kill('SIGTERM');
    const code = await proc.wait();
    expect(code).not.toBe(0);
    expect(proc.exitCode).toBe(code);
  });

  it('caches wait() across calls', async () => {
    const svc = ix.get(IHostProcessService);
    const proc = await svc.spawn('node', ['-e', 'process.exit(7)']);
    expect(proc.exitCode).toBeNull();
    expect(await proc.wait()).toBe(7);
    expect(await proc.wait()).toBe(7);
    expect(proc.exitCode).toBe(7);
  });
});
