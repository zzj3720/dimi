/**
 * `workspaceAliases` domain (L2) — workspace id-spelling resolution contract.
 *
 * Defines the App-scoped `IWorkspaceAliases`: the read-side counterpart to the
 * workspace write-path folding. One physical folder may be registered under
 * several id spellings; this service enumerates them so readers can query
 * every sibling session bucket at once. App-scoped.
 */

import { createDecorator, type ServiceIdentifier } from '#/_base/di/instantiation';

export interface IWorkspaceAliases {
  readonly _serviceBrand: undefined;

  /**
   * Every registered id whose `workspaceRootKey` identity matches `id`.
   * Read-only — ids and session buckets are never rewritten. An unknown `id`
   * resolves to `[id]` so callers keep their existing not-found semantics.
   */
  resolveAliasIds(id: string): Promise<readonly string[]>;
}

export const IWorkspaceAliases: ServiceIdentifier<IWorkspaceAliases> =
  createDecorator<IWorkspaceAliases>('workspaceAliases');
