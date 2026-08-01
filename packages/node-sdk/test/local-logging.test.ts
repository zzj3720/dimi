import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { createDimiHarness, log } from '#/index';
import { TEST_IDENTITY } from './test-identity';

const tempDirs: string[] = [];

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await rm(dir, { recursive: true, force: true });
  }
});

async function makeHome(): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), 'dimi-sdk-logging-'));
  tempDirs.push(dir);
  return dir;
}

describe('SDK diagnostic logging', () => {
  it('exposes the public logger without exposing runtime logger internals', async () => {
    expect(log).toMatchObject({
      error: expect.any(Function),
      warn: expect.any(Function),
      info: expect.any(Function),
      debug: expect.any(Function),
    });

    const sdk = await import('#/index');
    expect(Object.keys(sdk)).not.toContain('RootLogger');
    expect(Object.keys(sdk)).not.toContain('getRootLogger');
    expect(Object.keys(sdk)).not.toContain('LoggingConfig');
  });

  it('allows multiple harnesses in one process and closes both cleanly', async () => {
    const first = createDimiHarness({ homeDir: await makeHome(), identity: TEST_IDENTITY });
    const second = createDimiHarness({ homeDir: await makeHome(), identity: TEST_IDENTITY });

    await expect(first.close()).resolves.toBeUndefined();
    await expect(second.close()).resolves.toBeUndefined();
  });
});
