/**
 * `workspace` domain (L2) — alias-folding pure helpers.
 *
 * One physical folder can arrive under several id spellings (Windows
 * drive-letter casing, slash direction, and typed-vs-realpath variants).
 * These helpers enumerate or collapse registered spellings without owning
 * state. Shared by `WorkspaceService` and `WorkspaceAliasesService`.
 */

import { encodeWorkDirKey, workspaceRootKey } from '#/_base/utils/workdir-slug';

import type { Workspace } from './workspace';

/**
 * Every registered id identifying `root`'s directory. Read-only — ids and
 * session buckets are never rewritten.
 */
export function collectAliasIds(
  workspaces: readonly Workspace[],
  root: string,
): string[] {
  const rootKey = workspaceRootKey(root);
  const ids: string[] = [];
  const seen = new Set<string>();
  const add = (alias: string): void => {
    if (seen.has(alias)) return;
    seen.add(alias);
    ids.push(alias);
  };
  for (const ws of workspaces) {
    if (workspaceRootKey(ws.root) === rootKey) add(ws.id);
  }
  return ids;
}

/**
 * Collapse registered workspaces that identify the same directory. The
 * persisted catalog can contain multiple entries for the same folder because
 * Windows roots differ by casing or slash spelling. Entries merge on the
 * `workspaceRootKey` identity key; prefer the entry whose id matches the
 * canonical key computed from its root so current sessions resolve and the
 * same folder is not listed twice.
 */
export function dedupeByRoot(byId: ReadonlyMap<string, Workspace>): Workspace[] {
  const byRoot = new Map<string, Workspace>();
  for (const ws of byId.values()) {
    const rootKey = workspaceRootKey(ws.root);
    const existing = byRoot.get(rootKey);
    if (existing === undefined) {
      byRoot.set(rootKey, ws);
      continue;
    }
    const canonicalId = encodeWorkDirKey(ws.root);
    if (existing.id !== canonicalId && ws.id === canonicalId) {
      byRoot.set(rootKey, ws);
    }
  }
  return [...byRoot.values()];
}
