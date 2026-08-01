import { promises as fsp } from 'node:fs';
import os from 'node:os';
import { join } from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  LifecycleScope,
  ScopeActivation,
  _clearScopedRegistryForTests,
  registerScopedService,
} from '#/_base/di/scope';
import { createScopedTestHost, stubPair } from '#/_base/di/test';
import { ILogService } from '#/_base/log/log';
import { encodeWorkDirKey } from '#/_base/utils/workdir-slug';
import { IBootstrapService } from '#/app/bootstrap/bootstrap';
import { ISessionIndex, type SessionSummary } from '#/app/sessionIndex/sessionIndex';
import { FileSessionIndex } from '#/app/sessionIndex/sessionIndexService';
import { JsonAtomicDocumentStore } from '#/persistence/backends/node-fs/atomicDocumentStore';
import { FileStorageService } from '#/persistence/backends/node-fs/fileStorageService';
import { IAtomicDocumentStore } from '#/persistence/interface/atomicDocumentStore';
import { IFileSystemStorageService } from '#/persistence/interface/storage';

import { stubBootstrap } from '../bootstrap/stubs';
import { stubLog } from '../../_base/log/stubs';

const WORK_DIR = '/home/user/repo';

describe('FileSessionIndex (disk)', () => {
  let homeDir: string;
  let sessionsDir: string;
  let workspaceId: string;
  let disposeHost: (() => void) | undefined;

  beforeEach(async () => {
    _clearScopedRegistryForTests();
    registerScopedService(
      LifecycleScope.App,
      ISessionIndex,
      FileSessionIndex,
      ScopeActivation.OnDemand,
      'sessionIndex',
    );
    homeDir = await fsp.mkdtemp(join(os.tmpdir(), 'ws-sessions-'));
    sessionsDir = join(homeDir, 'sessions');
    workspaceId = encodeWorkDirKey(WORK_DIR);
  });

  afterEach(async () => {
    disposeHost?.();
    disposeHost = undefined;
    await fsp.rm(homeDir, { recursive: true, force: true });
  });

  function build(): ISessionIndex {
    const fileStorage = new FileStorageService(homeDir);
    const host = createScopedTestHost([
      stubPair(IFileSystemStorageService, fileStorage),
      stubPair(IAtomicDocumentStore, new JsonAtomicDocumentStore(fileStorage)),
      stubPair(IBootstrapService, stubBootstrap(homeDir)),
      stubPair(ILogService, stubLog()),
    ]);
    disposeHost = () => {
      host.dispose();
    };
    return host.app.accessor.get(ISessionIndex);
  }

  async function seedSession(
    sessionId: string,
    meta: Record<string, unknown>,
    wsId: string = workspaceId,
  ): Promise<void> {
    const dir = join(sessionsDir, wsId, sessionId);
    await fsp.mkdir(dir, { recursive: true });
    await fsp.writeFile(join(dir, 'state.json'), JSON.stringify(meta));
  }

  async function seedEmpty(sessionId: string, wsId: string = workspaceId): Promise<void> {
    await fsp.mkdir(join(sessionsDir, wsId, sessionId), { recursive: true });
  }

  it('list returns non-archived sessions by default', async () => {
    await seedSession('active', { createdAt: 1, updatedAt: 2 });
    await seedSession('archived', { archived: true });
    await seedEmpty('no-state');

    const store = build();
    const page = await store.list({ workspaceIds: [workspaceId] });
    expect(page.items.map((s) => s.id).toSorted()).toEqual(['active']);
    expect(page.items[0]?.workspaceId).toBe(workspaceId);
    expect(page.items[0]?.archived).toBe(false);
  });

  it('list includes archived when requested', async () => {
    await seedSession('active', {});
    await seedSession('archived', { archived: true });

    const store = build();
    const page = await store.list({ workspaceIds: [workspaceId], includeArchived: true });
    expect(page.items.map((s) => s.id).toSorted()).toEqual(['active', 'archived']);
  });

  it('get fetches a session by id across workspaces', async () => {
    await seedSession('active', { title: 'hello' });

    const store = build();
    const summary = await store.get('active');
    expect(summary?.id).toBe('active');
    expect(summary?.title).toBe('hello');
    expect(await store.get('missing')).toBeUndefined();
  });

  it('reads cwd from the current metadata field', async () => {
    await seedSession('current', { cwd: '/repo/current' });
    await seedSession('none', { title: 'no cwd' });

    const store = build();
    expect((await store.get('current'))?.cwd).toBe('/repo/current');
    expect((await store.get('none'))?.cwd).toBeUndefined();
  });

  it('list filters by sessionId without enumerating all sessions', async () => {
    await seedSession('active', { title: 'hello' });
    await seedSession('archived', { archived: true });

    const store = build();
    const active = await store.list({ sessionId: 'active' });
    expect(active.items.map((s) => s.id)).toEqual(['active']);

    const archived = await store.list({ sessionId: 'archived' });
    expect(archived.items).toEqual([]);

    const archivedIncluded = await store.list({ sessionId: 'archived', includeArchived: true });
    expect(archivedIncluded.items.map((s) => s.id)).toEqual(['archived']);
  });

  it('list filters by childOf using the parent_session_id + child_session_kind markers', async () => {
    await seedSession('parent', { createdAt: 1, updatedAt: 10 });
    await seedSession('child-a', {
      createdAt: 2,
      updatedAt: 9,
      custom: { parent_session_id: 'parent', child_session_kind: 'child' },
    });
    await seedSession('child-b', {
      createdAt: 3,
      updatedAt: 8,
      custom: { parent_session_id: 'parent', child_session_kind: 'child' },
    });
    await seedSession('fork', {
      createdAt: 4,
      updatedAt: 7,
      custom: { parent_session_id: 'parent' },
    });
    await seedSession('grandchild', {
      createdAt: 5,
      updatedAt: 6,
      custom: { parent_session_id: 'child-a', child_session_kind: 'child' },
    });

    const store = build();
    const page = await store.list({ childOf: 'parent' });
    expect(page.items.map((s) => s.id).toSorted()).toEqual(['child-a', 'child-b']);
  });

  it('countActive counts non-archived sessions', async () => {
    await seedSession('a', {});
    await seedSession('b', {});
    await seedSession('archived', { archived: true });
    await seedEmpty('no-state');

    const store = build();
    expect(await store.countActive([workspaceId])).toBe(2);
    expect(await store.countActive(['wd_unknown'])).toBe(0);
  });

  it('list merges a workspace-id set into one recency-ordered page', async () => {
    const otherId = encodeWorkDirKey('/home/user/other');
    await seedSession('a1', { createdAt: 1, updatedAt: 1 });
    await seedSession('a3', { createdAt: 3, updatedAt: 3 });
    await seedSession('b2', { createdAt: 2, updatedAt: 2 }, otherId);
    await seedSession('b4', { createdAt: 4, updatedAt: 4 }, otherId);

    const store = build();
    const page = await store.list({ workspaceIds: [workspaceId, otherId] });
    expect(page.items.map((s) => s.id)).toEqual(['b4', 'a3', 'b2', 'a1']);
    expect(page.items[0]?.workspaceId).toBe(otherId);
  });

  it('list applies limit after the cross-bucket merge', async () => {
    const otherId = encodeWorkDirKey('/home/user/other');
    await seedSession('a1', { createdAt: 1, updatedAt: 1 });
    await seedSession('a3', { createdAt: 3, updatedAt: 3 });
    await seedSession('b2', { createdAt: 2, updatedAt: 2 }, otherId);

    const store = build();
    const page = await store.list({ workspaceIds: [workspaceId, otherId], limit: 2 });
    expect(page.items.map((s) => s.id)).toEqual(['a3', 'b2']);
  });

  it('list filters archived across every bucket of the id set', async () => {
    const otherId = encodeWorkDirKey('/home/user/other');
    await seedSession('active', {});
    await seedSession('archived', { archived: true }, otherId);

    const store = build();
    const visible = await store.list({ workspaceIds: [workspaceId, otherId] });
    expect(visible.items.map((s) => s.id)).toEqual(['active']);

    const all = await store.list({ workspaceIds: [workspaceId, otherId], includeArchived: true });
    expect(all.items.map((s) => s.id).toSorted()).toEqual(['active', 'archived']);
  });

  it('countActive sums over the workspace-id set', async () => {
    const otherId = encodeWorkDirKey('/home/user/other');
    await seedSession('a', {});
    await seedSession('b', {}, otherId);
    await seedSession('archived', { archived: true }, otherId);

    const store = build();
    expect(await store.countActive([workspaceId, otherId])).toBe(2);
    expect(await store.countActive([otherId])).toBe(1);
  });
});

