/**
 * `hostEnvironment` domain (L1) — integration test for the Rust-backed
 * `RustHostEnvironmentService` (M2 slice 3 swap-in socket,
 * the default backend since the legacy flip).
 *
 * The probe is a pure function of the host; the assertions compare against
 * the Node facts (`process`/`os`) the node-local service would produce.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { homedir, release } from 'node:os';

import { DisposableStore } from '#/_base/di/lifecycle';
import { createServices, type TestInstantiationService } from '#/_base/di/test';
import { IHostEnvironment } from '#/os/interface/hostEnvironment';
import { RustHostEnvironmentService } from '#/os/backends/rust-local/rustHostEnvironmentService';

describe('RustHostEnvironmentService', () => {
  let disposables: DisposableStore;
  let ix: TestInstantiationService;

  beforeEach(() => {
    disposables = new DisposableStore();
    ix = createServices(disposables, {
      additionalServices: (reg) => {
        reg.define(IHostEnvironment, RustHostEnvironmentService);
      },
    });
  });

  afterEach(() => {
    disposables.dispose();
  });

  it('matches Node host facts after ready', async () => {
    const env = ix.get(IHostEnvironment);
    await env.ready;
    expect(env.osKind).toBe(
      process.platform === 'darwin'
        ? 'macOS'
        : process.platform === 'linux'
          ? 'Linux'
          : process.platform === 'win32'
            ? 'Windows'
            : process.platform,
    );
    expect(env.osArch).toBe(process.arch);
    expect(env.osVersion).toBe(release());
    expect(env.homeDir).toBe(homedir());
    expect(env.pathClass).toBe(process.platform === 'win32' ? 'win32' : 'posix');
  });

  it('discovers the same shell as the TS probe chain', async () => {
    const env = ix.get(IHostEnvironment);
    await env.ready;
    if (process.platform !== 'win32') {
      expect(env.shellPath.startsWith('/')).toBe(true);
      expect(['bash', 'sh']).toContain(env.shellName);
      if (env.shellName === 'bash') {
        expect(env.shellPath).toBe('/bin/bash');
      }
    }
  });

  it('throws before ready with a clear misuse message', async () => {
    const env = ix.get(IHostEnvironment);
    expect(() => env.osKind).toThrow(/await IHostEnvironment\.ready/);
  });
});
