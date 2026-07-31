/**
 * `workspace` domain (L2) — `IWorkspacePersistence` contract.
 *
 * Domain-specific persistence Store for the known-workspaces catalog. It hides
 * the on-disk `<homeDir>/workspaces.json` layout and serialization concerns
 * (ISO ↔ epoch-ms, record ↔ array) from the workspace service. The generic
 * `IAtomicDocumentStore` it builds on stays schema-agnostic.
 */

import { createDecorator, type ServiceIdentifier } from '#/_base/di/instantiation';

import type { Workspace } from './workspace';

export interface PersistedWorkspaceEntry {
  readonly root: string;
  readonly name: string;
  readonly created_at: string;
  readonly last_opened_at: string;
}

export interface PersistedWorkspaceFile {
  readonly version: number;
  readonly workspaces: Record<string, PersistedWorkspaceEntry>;
}

export interface WorkspaceCatalog {
  readonly workspaces: readonly Workspace[];
}

export interface IWorkspacePersistence {
  readonly _serviceBrand: undefined;

  load(): Promise<WorkspaceCatalog | undefined>;
  save(catalog: WorkspaceCatalog): Promise<void>;
}

export const IWorkspacePersistence: ServiceIdentifier<IWorkspacePersistence> =
  createDecorator<IWorkspacePersistence>('workspacePersistence');
