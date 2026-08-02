/**
 * M2 env-slice differential suite: Node's `process`/`os` facts vs the Rust
 * `dimi-exec` environment probe (`RustHostEnvironment`).
 *
 * The probe is a pure function of the host, so both sides must agree on
 * every field for the same machine.
 *
 * Skips itself when the native binding is not built (same policy as the
 * other suites).
 */
import { existsSync } from 'node:fs';
import { homedir, release } from 'node:os';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

import { rustHostEnvironmentProbe } from '#/index';

const bindingPath = fileURLToPath(new URL('../dist/dimi_bridge.node', import.meta.url));
const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

suite('env: Node process/os vs Rust dimi-exec probe', () => {
  test('osKind matches Node platform mapping', () => {
    const rust = rustHostEnvironmentProbe();
    const expected =
      process.platform === 'darwin' ? 'macOS' : process.platform === 'linux' ? 'Linux' : process.platform === 'win32' ? 'Windows' : process.platform;
    expect(rust.osKind).toBe(expected);
  });

  test('osArch matches process.arch exactly', () => {
    const rust = rustHostEnvironmentProbe();
    expect(rust.osArch).toBe(process.arch);
  });

  test('osVersion matches os.release()', () => {
    const rust = rustHostEnvironmentProbe();
    // Windows kernel-release probing is deferred to the Windows parity pass.
    if (process.platform !== 'win32') {
      expect(rust.osVersion).toBe(release());
    }
  });

  test('homeDir matches os.homedir()', () => {
    const rust = rustHostEnvironmentProbe();
    expect(rust.homeDir).toBe(homedir());
  });

  test('pathClass matches platform', () => {
    const rust = rustHostEnvironmentProbe();
    expect(rust.pathClass).toBe(process.platform === 'win32' ? 'win32' : 'posix');
  });

  test('shell discovery agrees with the TS probe chain', async () => {
    const rust = rustHostEnvironmentProbe();
    if (process.platform === 'win32') return; // git-bash discovery deferred
    const { access } = await import('node:fs/promises');
    const isFile = async (p: string): Promise<boolean> => {
      try {
        await access(p);
        return true;
      } catch {
        return false;
      }
    };
    for (const candidate of ['/bin/bash', '/usr/bin/bash', '/usr/local/bin/bash']) {
      if (await isFile(candidate)) {
        expect(rust.shellName).toBe('bash');
        expect(rust.shellPath).toBe(candidate);
        return;
      }
    }
    expect(rust.shellName).toBe('sh');
    expect(rust.shellPath).toBe('/bin/sh');
  });

  test('probe fields are all non-empty', () => {
    const rust = rustHostEnvironmentProbe();
    expect(rust.osKind.length).toBeGreaterThan(0);
    expect(rust.osArch.length).toBeGreaterThan(0);
    expect(rust.osVersion.length).toBeGreaterThan(0);
    expect(rust.shellName.length).toBeGreaterThan(0);
    expect(rust.shellPath.length).toBeGreaterThan(0);
    expect(rust.pathClass.length).toBeGreaterThan(0);
    expect(rust.homeDir.length).toBeGreaterThan(0);
  });
});
