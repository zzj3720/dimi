import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { promises as fsp } from 'node:fs';
import os from 'node:os';
import { join } from 'node:path';

import {
  LifecycleScope,
  ScopeActivation,
  _clearScopedRegistryForTests,
  registerScopedService,
} from '#/_base/di/scope';
import { createScopedTestHost, stubPair } from '#/_base/di/test';
import { encodeWorkDirKey, workspaceRootKey } from '#/_base/utils/workdir-slug';
import { ErrorCodes, Error2 } from '#/errors';
import { HostFileSystem } from '#/os/backends/node-local/hostFsService';
import { IHostFileSystem } from '#/os/interface/hostFileSystem';
import { JsonAtomicDocumentStore } from '#/persistence/backends/node-fs/atomicDocumentStore';
import { FileStorageService } from '#/persistence/backends/node-fs/fileStorageService';
import { IAtomicDocumentStore } from '#/persistence/interface/atomicDocumentStore';
import { IFileSystemStorageService } from '#/persistence/interface/storage';
import { IWorkspaceService } from '#/app/workspace/workspace';
import { WorkspaceService } from '#/app/workspace/workspaceService';
import { FileWorkspacePersistence } from '#/app/workspace/fileWorkspacePersistence';
import { IWorkspacePersistence, type PersistedWorkspaceEntry } from '#/app/workspace/workspacePersistence';

describe('WorkspaceService (file-backed)', () => {
  let homeDir: string;
  let currentHost: ReturnType<typeof createScopedTestHost> | undefined;

  beforeEach(async () => {
    _clearScopedRegistryForTests();
    registerScopedService(
      LifecycleScope.App,
      IWorkspacePersistence,
      FileWorkspacePersistence,
      ScopeActivation.OnDemand,
      'workspace',
    );
    registerScopedService(
      LifecycleScope.App,
      IWorkspaceService,
      WorkspaceService,
      ScopeActivation.OnDemand,
      'workspace',
    );
    homeDir = await fsp.mkdtemp(join(os.tmpdir(), 'ws-registry-'));
  });

  afterEach(async () => {
    currentHost?.dispose();
    currentHost = undefined;
    await fsp.rm(homeDir, { recursive: true, force: true });
  });

  function build(hostFs: IHostFileSystem = new HostFileSystem()): IWorkspaceService {
    const fileStorage = new FileStorageService(homeDir);
    const host = createScopedTestHost([
      stubPair(IFileSystemStorageService, fileStorage),
      stubPair(IAtomicDocumentStore, new JsonAtomicDocumentStore(fileStorage)),
      stubPair(IHostFileSystem, hostFs),
    ]);
    currentHost = host;
    return host.app.accessor.get(IWorkspaceService);
  }

  function restart(): IWorkspaceService {
    currentHost?.dispose();
    currentHost = undefined;
    return build();
  }

  /**
   * hostFs stub that stats every path as an existing directory, so tests can
   * exercise Windows-shaped roots on Linux CI — real-fs stat of `C:\...` is
   * ENOENT there, and real fs case behavior must never be relied on.
   */
  function allDirsHostFs(): IHostFileSystem {
    return {
      stat: () => Promise.resolve({ isFile: false, isDirectory: true, size: 0 }),
    } as unknown as IHostFileSystem;
  }

  async function writeWorkspacesJson(
    workspaces: Record<string, PersistedWorkspaceEntry>,
  ): Promise<void> {
    await fsp.writeFile(
      join(homeDir, 'workspaces.json'),
      JSON.stringify({ version: 1, workspaces }),
      'utf8',
    );
  }

  async function readWorkspacesJson(): Promise<{
    workspaces: Record<string, PersistedWorkspaceEntry>;
  }> {
    return JSON.parse(await fsp.readFile(join(homeDir, 'workspaces.json'), 'utf8')) as {
      workspaces: Record<string, PersistedWorkspaceEntry>;
    };
  }

  it('persists the catalog across registry instances', async () => {
    const created = await build().createOrTouch(homeDir, 'proj');

    const list = await restart().list();
    expect(list.map((w) => w.id)).toContain(created.id);
    expect(list.find((w) => w.id === created.id)?.name).toBe('proj');
  });

  it('starts empty when the catalog does not exist', async () => {
    expect(await build().list()).toEqual([]);
  });

  it('delete removes the workspace from the persisted catalog', async () => {
    const dirA = join(homeDir, 'dir-a');
    const dirB = join(homeDir, 'dir-b');
    await fsp.mkdir(dirA);
    await fsp.mkdir(dirB);
    const registry = build();
    const a = await registry.createOrTouch(dirA);
    await registry.createOrTouch(dirB);

    await registry.delete(a.id);
    expect((await registry.list()).map((w) => w.id)).toEqual([encodeWorkDirKey(dirB)]);

    const onDisk = await readWorkspacesJson();
    expect(onDisk.workspaces[a.id]).toBeUndefined();
    expect((await restart().list()).map((w) => w.id)).toEqual([encodeWorkDirKey(dirB)]);
  });

  it('createOrTouch can recreate a deleted workspace', async () => {
    const dirA = join(homeDir, 'dir-a');
    await fsp.mkdir(dirA);
    const registry = build();
    const a = await registry.createOrTouch(dirA);
    await registry.delete(a.id);

    await registry.createOrTouch(dirA);
    expect((await registry.list()).map((w) => w.id)).toEqual([a.id]);

    expect((await restart().list()).map((w) => w.id)).toEqual([a.id]);
  });

  it('createOrTouch preserves external additions written after load', async () => {
    const dirA = join(homeDir, 'dir-a');
    const dirB = join(homeDir, 'dir-b');
    const dirC = join(homeDir, 'dir-c');
    await fsp.mkdir(dirA);
    await fsp.mkdir(dirC);
    const registry = build();
    await registry.createOrTouch(dirA);

    // Simulate another process adding a workspace after this service has run.
    const onDisk = await readWorkspacesJson();
    onDisk.workspaces[encodeWorkDirKey(dirB)] = {
      root: dirB,
      name: 'dir-b',
      created_at: '2024-01-01T00:00:00.000Z',
      last_opened_at: '2024-01-01T00:00:00.000Z',
    };
    await fsp.writeFile(
      join(homeDir, 'workspaces.json'),
      JSON.stringify({
        version: 1,
        workspaces: onDisk.workspaces,
      }),
      'utf8',
    );

    await registry.createOrTouch(dirC);

    const after = await readWorkspacesJson();
    expect(Object.keys(after.workspaces).toSorted()).toEqual(
      [encodeWorkDirKey(dirA), encodeWorkDirKey(dirB), encodeWorkDirKey(dirC)].toSorted(),
    );
    // Reads also see the external entry without a restart.
    expect((await registry.list()).map((w) => w.id)).toContain(encodeWorkDirKey(dirB));
  });

  it('update renames the current file entry and misses externally removed ids', async () => {
    const dirA = join(homeDir, 'dir-a');
    await fsp.mkdir(dirA);
    const registry = build();
    const a = await registry.createOrTouch(dirA);

    // External rename on disk: the update must start from it, not stale state.
    const onDisk = await readWorkspacesJson();
    const entry = onDisk.workspaces[a.id];
    if (entry === undefined) throw new Error('seed entry missing');
    onDisk.workspaces[a.id] = { ...entry, name: 'external-name' };
    await fsp.writeFile(
      join(homeDir, 'workspaces.json'),
      JSON.stringify({ version: 1, workspaces: onDisk.workspaces }),
      'utf8',
    );

    const renamed = await registry.update(a.id, { name: 'local-name' });
    expect(renamed?.name).toBe('local-name');
    expect(renamed?.lastOpenedAt).toBe(Date.parse(entry.last_opened_at));

    // External removal: update reports the id as gone instead of resurrecting.
    await fsp.writeFile(
      join(homeDir, 'workspaces.json'),
      JSON.stringify({ version: 1, workspaces: {} }),
      'utf8',
    );
    expect(await registry.update(a.id, { name: 'whatever' })).toBeUndefined();
  });

  it('writes through on update and delete', async () => {
    const created = await build().createOrTouch(homeDir, 'proj');
    await build().update(created.id, { name: 'renamed' });

    expect((await restart().get(created.id))?.name).toBe('renamed');

    await build().delete(created.id);
    expect(await restart().get(created.id)).toBeUndefined();
  });

  it('rejects createOrTouch when the root directory does not exist', async () => {
    const missing = join(homeDir, 'never-created');
    await expect(build().createOrTouch(missing)).rejects.toMatchObject({
      code: ErrorCodes.FS_PATH_NOT_FOUND,
    });
    expect(await build().list()).toEqual([]);
  });

  it('rejects createOrTouch when the root is not a directory', async () => {
    const file = join(homeDir, 'a-file.txt');
    await fsp.writeFile(file, 'hi', 'utf8');
    await expect(build().createOrTouch(file)).rejects.toMatchObject({
      code: ErrorCodes.FS_PATH_NOT_FOUND,
    });
    expect(await build().list()).toEqual([]);
  });

  it('accepts createOrTouch when the root is given through a symlink', async () => {
    const real = join(homeDir, 'real-root');
    await fsp.mkdir(real, { recursive: true });
    const link = join(homeDir, 'link-root');
    await fsp.symlink(real, link, 'dir');
    const ws = await build().createOrTouch(link);
    expect(ws.root).toBe(link);
    expect(ws.id).toBe(encodeWorkDirKey(link));
  });

  it('rejects createOrTouch when a parent of the root is not a directory', async () => {
    const file = join(homeDir, 'a-file.txt');
    await fsp.writeFile(file, 'hi', 'utf8');
    await expect(build().createOrTouch(join(file, 'child'))).rejects.toMatchObject({
      code: ErrorCodes.FS_PATH_NOT_FOUND,
    });
  });

  it('collapses duplicate registered entries for the same root, preferring the canonical id', async () => {
    const root = join(homeDir, 'dup');
    const canonicalId = encodeWorkDirKey(root);
    const legacyId = 'wd_duplegacy_deadbeef0000';
    const entry: PersistedWorkspaceEntry = {
      root,
      name: 'dup',
      created_at: '2026-01-01T00:00:00.000Z',
      last_opened_at: '2026-01-01T00:00:00.000Z',
    };
    await writeWorkspacesJson({
      [legacyId]: entry,
      [canonicalId]: entry,
    });

    const list = await build().list();
    const matches = list.filter((w) => w.root === root);
    expect(matches).toHaveLength(1);
    expect(matches[0]?.id).toBe(canonicalId);
  });

  it('folds Windows casing/slash variants onto the first-registered entry', async () => {
    const registry = build(allDirsHostFs());

    const first = await registry.createOrTouch('C:\\Users\\Foo\\Proj');
    const cased = await registry.createOrTouch('c:\\Users\\Foo\\Proj');
    const slashed = await registry.createOrTouch('C:/Users/Foo/Proj/');

    expect(cased.id).toBe(first.id);
    expect(slashed.id).toBe(first.id);
    // Folding never rewrites the stored root/name — the first spelling stays;
    // only lastOpenedAt advances.
    expect(cased.root).toBe('C:\\Users\\Foo\\Proj');
    expect(cased.name).toBe(first.name);
    expect(cased.lastOpenedAt).toBeGreaterThanOrEqual(first.lastOpenedAt);
    expect(await registry.list()).toHaveLength(1);

    // ...and the fold persists: a fresh instance over the same homeDir still
    // lists one entry under the first-seen spelling.
    const reloaded = await restart().list();
    expect(reloaded).toHaveLength(1);
    expect(reloaded[0]?.root).toBe('C:\\Users\\Foo\\Proj');
  });

  it('merges registered entries whose roots differ only by casing, preferring the canonical id', async () => {
    const lowerRoot = 'c:\\users\\foo\\proj';
    const typedRoot = 'C:\\Users\\Foo\\Proj';
    const legacyId = 'wd_proj_deadbeef0002';
    const canonicalId = encodeWorkDirKey(lowerRoot);
    const entry = (root: string): PersistedWorkspaceEntry => ({
      root,
      name: 'proj',
      created_at: '2026-01-01T00:00:00.000Z',
      last_opened_at: '2026-01-01T00:00:00.000Z',
    });
    await writeWorkspacesJson({
      // Non-canonical first so the canonical entry must actively replace it.
      [legacyId]: entry(typedRoot),
      [canonicalId]: entry(lowerRoot),
    });

    const list = await build().list();
    expect(list).toHaveLength(1);
    expect(list[0]?.id).toBe(canonicalId);
    expect(list[0]?.root).toBe(lowerRoot);
  });

  it('keeps POSIX roots case-sensitive', async () => {
    const registry = build(allDirsHostFs());

    const upper = await registry.createOrTouch('/tmp/Foo');
    const lower = await registry.createOrTouch('/tmp/foo');

    expect(lower.id).not.toBe(upper.id);
    expect((await registry.list()).map((w) => w.root).toSorted()).toEqual(['/tmp/Foo', '/tmp/foo']);
  });




  it('delete removes every registered alias of one root', async () => {
    const typedRoot = 'C:\\Users\\Foo\\Proj';
    const typedId = encodeWorkDirKey(typedRoot);
    const aliasRoot = 'c:\\Users\\Foo\\Proj';
    const aliasId = encodeWorkDirKey(aliasRoot);
    const unrelatedRoot = join(homeDir, 'unrelated');
    const unrelatedId = encodeWorkDirKey(unrelatedRoot);
    await writeWorkspacesJson({
      [typedId]: {
        root: typedRoot,
        name: 'proj',
        created_at: '2026-01-01T00:00:00.000Z',
        last_opened_at: '2026-01-01T00:00:00.000Z',
      },
      [aliasId]: {
        root: aliasRoot,
        name: 'proj',
        created_at: '2026-01-01T00:00:00.000Z',
        last_opened_at: '2026-01-01T00:00:00.000Z',
      },
      [unrelatedId]: {
        root: unrelatedRoot,
        name: 'unrelated',
        created_at: '2026-01-01T00:00:00.000Z',
        last_opened_at: '2026-01-01T00:00:00.000Z',
      },
    });

    const registry = build();
    await registry.delete(typedId);

    const stillListed = (await registry.list()).filter(
      (w) => workspaceRootKey(w.root) === workspaceRootKey(typedRoot),
    );
    expect(stillListed).toEqual([]);
    const saved = await readWorkspacesJson();
    expect(Object.keys(saved.workspaces)).toEqual([unrelatedId]);
  });
});

describe('workspaceRootKey', () => {
  it('folds drive-letter casing and slash direction', () => {
    expect(workspaceRootKey('C:\\Users\\Foo\\Proj')).toBe('c:/users/foo/proj');
    expect(workspaceRootKey('c:/Users/Foo/Proj/')).toBe('c:/users/foo/proj');
    expect(workspaceRootKey('C:\\Users\\Foo\\Proj')).toBe(workspaceRootKey('c:/users/foo/proj'));
  });

  it('folds drive roots before separator stripping can mask the shape', () => {
    // `C:\` would strip to `C:` and stop reading as Windows-shaped.
    expect(workspaceRootKey('C:\\')).toBe('c:');
    expect(workspaceRootKey('C:\\')).toBe(workspaceRootKey('c:\\'));
    expect(workspaceRootKey('C:\\')).toBe(workspaceRootKey('c:/'));
  });

  it('folds UNC hosts and shares', () => {
    expect(workspaceRootKey('\\\\HOST\\Share\\Dir')).toBe('//host/share/dir');
    expect(workspaceRootKey('//HOST/Share/Dir/')).toBe('//host/share/dir');
  });

  it('strips trailing separators but never case-folds POSIX paths', () => {
    expect(workspaceRootKey('/tmp/Foo/')).toBe('/tmp/Foo');
    expect(workspaceRootKey('/tmp/Foo')).not.toBe(workspaceRootKey('/tmp/foo'));
  });
});
