/**
 * Scenario: Node SDK sessions persist and list through the public harness.
 * Wiring: real in-process runtime; no provider calls.
 */
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { createKimiHarness, type KimiError } from '#/index';
import { TEST_IDENTITY } from './test-identity';

const tempDirs: string[] = [];

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await rm(dir, { recursive: true, force: true });
  }
});

async function makeTempDir(): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), 'kimi-sdk-list-'));
  tempDirs.push(dir);
  return dir;
}

describe('KimiHarness.listSessions', () => {
  it('rejects whitespace-only workDir with request.work_dir_required', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    try {
      await expect(harness.listSessions({ workDir: '   ' })).rejects.toMatchObject({
        code: 'request.work_dir_required',
      } satisfies Partial<KimiError>);
    } finally {
      await harness.close();
    }
  });

  it('lists sessions across workspaces when no filter is provided', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    const workDir = await makeTempDir();
    const otherWorkDir = await makeTempDir();
    try {
      await harness.createSession({ id: 'ses_harness_all_a', workDir });
      await harness.createSession({ id: 'ses_harness_all_b', workDir: otherWorkDir });

      expect((await harness.listSessions()).map((session) => session.id).toSorted()).toEqual([
        'ses_harness_all_a',
        'ses_harness_all_b',
      ]);
    } finally {
      await harness.close();
    }
  });

  it('filters native, relative, spaced, and non-ASCII workspace paths', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    const root = await makeTempDir();
    const workDir = join(root, 'Workspace With Spaces', '项目');
    await mkdir(workDir, { recursive: true });
    const originalCwd = process.cwd();
    try {
      process.chdir(root);
      const canonicalWorkDir = join(process.cwd(), 'Workspace With Spaces', '项目');
      const session = await harness.createSession({
        id: 'ses_unicode_workdir',
        workDir: canonicalWorkDir,
      });

      expect((await harness.listSessions({ workDir: canonicalWorkDir })).map((item) => item.id)).toEqual([session.id]);
      expect((await harness.listSessions({ workDir: './Workspace With Spaces/项目' })).map((item) => item.id)).toEqual([session.id]);
    } finally {
      process.chdir(originalCwd);
      await harness.close();
    }
  });

  it('keeps a persisted session visible after its active wrapper closes', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    const workDir = await makeTempDir();
    try {
      const session = await harness.createSession({ id: 'ses_closed_but_listed', workDir });
      await harness.closeSession(session.id);

      expect((await harness.listSessions({ workDir })).map((item) => item.id)).toEqual([session.id]);
    } finally {
      await harness.close();
    }
  });
});
