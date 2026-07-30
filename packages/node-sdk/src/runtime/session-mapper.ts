/**
 * Pure mapping between the runtime session index and the public SDK session
 * shape. The index carries no filesystem facts, so the SDK
 * `SessionSummary`'s `workDir` / `sessionDir` come in as pre-resolved
 *   `SessionSummaryFacts` (the caller derives them from `ISessionContext`,
 *   `IBootstrapService.sessionDir`, and the workspace catalog);
 * the remaining difference is the `custom` ↔ `metadata` field name.
 */
import type { SessionSummary as RuntimeSessionSummary } from '@moonshot-ai/agent-core-v2';

import { resolve, win32 } from 'node:path';

import type { JsonObject, SessionSummary } from '#/types';

/**
 * Normalize Windows-shaped paths through `win32` and fold to forward slashes;
 * resolve every other path against the process cwd.
 */
export function normalizeWorkDir(workDir: string): string {
  if (/^[A-Za-z]:[\\/]/.test(workDir) || /^[\\/]{2}[^\\/]+[\\/][^\\/]+/.test(workDir)) {
    return win32.resolve(workDir).replaceAll('\\', '/');
  }
  return resolve(workDir);
}

/** Public SDK fields the runtime index does not carry, resolved by the caller. */
export interface SessionSummaryFacts {
  readonly workDir: string;
  readonly sessionDir: string;
  readonly additionalDirs?: readonly string[];
}

export function runtimeSummaryToSessionSummary(
  summary: RuntimeSessionSummary,
  facts: SessionSummaryFacts,
): SessionSummary {
  return {
    id: summary.id,
    title: summary.title,
    lastPrompt: summary.lastPrompt,
    workDir: facts.workDir,
    sessionDir: facts.sessionDir,
    createdAt: summary.createdAt,
    updatedAt: summary.updatedAt,
    archived: summary.archived,
    metadata: summary.custom as JsonObject | undefined,
    additionalDirs: facts.additionalDirs,
  };
}
