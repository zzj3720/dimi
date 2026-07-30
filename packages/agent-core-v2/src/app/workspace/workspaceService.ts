/**
 * `workspace` domain (L2) — `IWorkspaceService` implementation.
 *
 * Process-wide catalog of known workspaces, durable in
 * `<homeDir>/workspaces.json`. Every operation reads the current catalog and
 * mutations are serialized through a promise-chain mutex, so concurrent
 * callers do not overwrite one another with stale in-memory state.
 *
 * `createOrTouch` is the single choke point every workspace/session creation
 * funnels through, so it owns the root-existence contract: the root must be
 * an existing directory on the host filesystem, otherwise it throws
 * `fs.path_not_found`. The
 * directory probe follows symlinks (`IHostFileSystem.stat` is lstat-based, so
 * a symlink-form root is re-checked through `realpath`), while the workspace
 * identity stays lexical. Bound at App scope.
 *
 * One physical folder can arrive under several spellings — most visibly on
 * Windows, where drive-letter casing, slash direction, and typed-vs-realpath
 * casing all differ for one directory. Every "same directory?" judgment
 * (`createOrTouch` reuse and the `list` merge in `dedupeByRoot`) therefore
 * goes through the `workspaceRootKey` identity key
 * rather than the raw root string, while the minted `workspaceId` stays the
 * case-sensitive `encodeWorkDirKey` so already-persisted session buckets,
 * `workspaces.json` entries, and session metadata keep resolving. Registered
 * sibling ids remain readable through `IWorkspaceAliases`; deleting one
 * directory removes all of its registered spellings.
 */

import { basename } from 'pathe';

import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';
import { encodeWorkDirKey, workspaceRootKey } from '#/_base/utils/workdir-slug';
import { ErrorCodes, Error2, unwrapErrorCause } from '#/errors';
import { IHostFileSystem } from '#/os/interface/hostFileSystem';

import { IWorkspaceService, type Workspace, type WorkspaceUpdate } from './workspace';
import { collectAliasIds, dedupeByRoot } from './workspaceAlias';
import { IWorkspacePersistence, type WorkspaceCatalog } from './workspacePersistence';

export class WorkspaceService implements IWorkspaceService {
  declare readonly _serviceBrand: undefined;

  private opQueue: Promise<unknown> = Promise.resolve();

  constructor(
    @IWorkspacePersistence private readonly store: IWorkspacePersistence,
    @IHostFileSystem private readonly hostFs: IHostFileSystem,
  ) {}

  list(): Promise<readonly Workspace[]> {
    return this.runExclusive(async () => {
      const catalog = await this.loadCatalog();
      const byId = new Map(catalog.workspaces.map((ws) => [ws.id, ws]));
      return dedupeByRoot(byId);
    });
  }

  get(id: string): Promise<Workspace | undefined> {
    return this.runExclusive(async () => {
      const catalog = await this.loadCatalog();
      return catalog.workspaces.find((ws) => ws.id === id);
    });
  }

  createOrTouch(root: string, name?: string): Promise<Workspace> {
    return this.runExclusive(async () => {
      let stat;
      try {
        stat = await this.hostFs.stat(root);
      } catch (error) {
        const code = (unwrapErrorCause(error) as NodeJS.ErrnoException | undefined)?.code;
        if (code === 'ENOENT' || code === 'ENOTDIR') {
          throw new Error2(ErrorCodes.FS_PATH_NOT_FOUND, `workspace root ${root} does not exist`);
        }
        throw error;
      }
      if (!stat.isDirectory) {
        try {
          stat = await this.hostFs.stat(await this.hostFs.realpath(root));
        } catch {
          // Fall through to the not-a-directory error below.
        }
      }
      if (!stat.isDirectory) {
        throw new Error2(ErrorCodes.FS_PATH_NOT_FOUND, `workspace root ${root} is not a directory`);
      }
      const catalog = await this.loadCatalog();
      const byId = new Map(catalog.workspaces.map((ws) => [ws.id, ws]));
      const id = encodeWorkDirKey(root);
      let existing = byId.get(id);
      if (existing === undefined) {
        // Fold identity-equivalent spellings (`workspaceRootKey`: Windows
        // drive-letter/realpath casing, slash direction) onto the registered
        // entry instead of minting a second id for the same folder. The first
        // matching entry wins wholesale — its id, root, and name are kept;
        // only `lastOpenedAt` advances.
        const rootKey = workspaceRootKey(root);
        for (const entry of byId.values()) {
          if (workspaceRootKey(entry.root) === rootKey) {
            existing = entry;
            break;
          }
        }
      }
      const now = Date.now();
      const ws: Workspace =
        existing !== undefined
          ? { ...existing, lastOpenedAt: now }
          : {
              id,
              root,
              name: name ?? basename(root),
              createdAt: now,
              lastOpenedAt: now,
            };
      byId.set(ws.id, ws);
      await this.store.save({ workspaces: [...byId.values()] });
      return ws;
    });
  }

  update(id: string, patch: WorkspaceUpdate): Promise<Workspace | undefined> {
    return this.runExclusive(async () => {
      const catalog = await this.loadCatalog();
      const existing = catalog.workspaces.find((ws) => ws.id === id);
      if (existing === undefined) return undefined;
      const updated: Workspace = {
        ...existing,
        ...(patch.name !== undefined ? { name: patch.name } : {}),
      };
      await this.store.save({
        workspaces: catalog.workspaces.map((ws) => (ws.id === id ? updated : ws)),
      });
      return updated;
    });
  }

  delete(id: string): Promise<void> {
    return this.runExclusive(async () => {
      const catalog = await this.loadCatalog();
      const root = catalog.workspaces.find((ws) => ws.id === id)?.root;
      if (root === undefined) {
        await this.store.save({
          workspaces: catalog.workspaces.filter((ws) => ws.id !== id),
        });
        return;
      }
      const rootKey = workspaceRootKey(root);
      const aliasIds = new Set(collectAliasIds(catalog.workspaces, root));
      await this.store.save({
        workspaces: catalog.workspaces.filter(
          (ws) => !aliasIds.has(ws.id) && workspaceRootKey(ws.root) !== rootKey,
        ),
      });
    });
  }

  /** Read the current catalog; a missing or malformed file is empty. */
  private async loadCatalog(): Promise<WorkspaceCatalog> {
    return (await this.store.load()) ?? { workspaces: [] };
  }

  private runExclusive<T>(op: () => Promise<T>): Promise<T> {
    const next = this.opQueue.then(op, op);
    this.opQueue = next.then(
      () => {},
      () => {},
    );
    return next;
  }
}

registerScopedService(
  LifecycleScope.App,
  IWorkspaceService,
  WorkspaceService,
  ScopeActivation.OnScopeCreated,
  'workspace',
);
