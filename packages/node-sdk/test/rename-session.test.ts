import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { createKimiHarness, KimiError, type Event } from '#/index';
import { TEST_IDENTITY } from './test-identity';

const tempDirs: string[] = [];

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await rm(dir, { recursive: true, force: true });
  }
});

async function makeTempDir(): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), 'kimi-sdk-rename-'));
  tempDirs.push(dir);
  return dir;
}

describe('KimiHarness.renameSession', () => {
  it('persists a title and emits an event for an active session', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    const workDir = await makeTempDir();
    try {
      const session = await harness.createSession({ id: 'ses_harness_rename', workDir });
      const events: Event[] = [];
      const unsubscribe = session.onEvent((event) => events.push(event));

      await harness.renameSession({ id: session.id, title: 'Harness Title' });
      unsubscribe();

      expect((await harness.listSessions({ workDir })).find((item) => item.id === session.id)?.title).toBe('Harness Title');
      expect(events).toContainEqual(expect.objectContaining({
        type: 'session.meta.updated',
        sessionId: session.id,
        agentId: 'main',
        title: 'Harness Title',
      }));
    } finally {
      await harness.close();
    }
  });

  it('renames a persisted session after its active wrapper closes', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    const workDir = await makeTempDir();
    try {
      const session = await harness.createSession({ id: 'ses_inactive_rename', workDir });
      await harness.closeSession(session.id);
      await harness.renameSession({ id: session.id, title: 'Inactive Title' });

      expect((await harness.listSessions({ workDir })).find((item) => item.id === session.id)?.title).toBe('Inactive Title');
    } finally {
      await harness.close();
    }
  });

  it('rejects missing session ids', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    try {
      const rename = harness.renameSession({ id: 'ses_missing', title: 'Missing Title' });
      await expect(rename).rejects.toBeInstanceOf(KimiError);
      await expect(rename).rejects.toMatchObject({
        code: 'session.not_found',
        details: { sessionId: 'ses_missing' },
      } satisfies Partial<KimiError>);
    } finally {
      await harness.close();
    }
  });
});
