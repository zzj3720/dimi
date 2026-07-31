/**
 * `workspaceAliases` domain (L2) — `IWorkspaceAliases` implementation.
 *
 * Resolves every registered id spelling of one physical directory by folding
 * the raw catalog with `workspaceRootKey`. The raw catalog is required because
 * `IWorkspaceService.list` collapses sibling spellings to one representative.
 * Read-only: no id or session bucket is rewritten here. Bound at App scope.
 */

import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';
import { IWorkspaceService } from '#/app/workspace/workspace';
import { collectAliasIds } from '#/app/workspace/workspaceAlias';
import { IWorkspacePersistence } from '#/app/workspace/workspacePersistence';

import { IWorkspaceAliases } from './workspaceAliases';

export class WorkspaceAliasesService implements IWorkspaceAliases {
  declare readonly _serviceBrand: undefined;

  constructor(
    @IWorkspaceService private readonly workspaces: IWorkspaceService,
    @IWorkspacePersistence private readonly store: IWorkspacePersistence,
  ) {}

  async resolveAliasIds(id: string): Promise<readonly string[]> {
    const entry = await this.workspaces.get(id);
    // Unknown ids stay singletons so callers keep their not-found semantics.
    if (entry === undefined) return [id];
    const catalog = (await this.store.load()) ?? { workspaces: [] };
    return collectAliasIds(catalog.workspaces, entry.root);
  }
}

registerScopedService(
  LifecycleScope.App,
  IWorkspaceAliases,
  WorkspaceAliasesService,
  ScopeActivation.OnScopeCreated,
  'workspaceAliases',
);
