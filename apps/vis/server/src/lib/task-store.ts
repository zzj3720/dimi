// apps/vis/server/src/lib/task-store.ts
//
// Read-only reader for background tasks, persisted by agent-core under each
// spawning agent's homedir at `<agentDir>/tasks/<taskId>.json`
// (+ `tasks/<taskId>/output.log`) — NOT the session root. Callers pass the
// agent homedir (`<session>/agents/<id>`).
//
// The visualizer never writes these files; it mirrors the runtime's on-disk
// layout for reading only:
//   - the same `VALID_TASK_ID` guard, so a corrupt / hand-edited filename
//     cannot turn a log path into a traversal primitive;

import { open, readdir, readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';

import type { BackgroundTaskInfo } from './agent-record-types';

/** Task id format: `{prefix}-{8 chars of [0-9a-z]}`. Mirror of agent-core's
 *  `VALID_TASK_ID` (background/persist.ts). Enforced before deriving any
 *  output path so neither `../` nor a legacy `bg_<hex>` id can escape. */
const VALID_TASK_ID = /^[a-z0-9]+(?:-[a-z0-9]+)*-[0-9a-z]{8}$/;

export function isSafeTaskId(id: string): boolean {
  return VALID_TASK_ID.test(id);
}

function tasksDirOf(agentDir: string): string {
  return join(agentDir, 'tasks');
}

function taskOutputFile(agentDir: string, taskId: string): string {
  if (!VALID_TASK_ID.test(taskId)) {
    throw new Error(`Invalid task id: "${taskId}"`);
  }
  return join(tasksDirOf(agentDir), taskId, 'output.log');
}

/**
 * Enumerate all persisted background tasks for a session, normalized to the
 * current `BackgroundTaskInfo` shape and sorted newest-first by start time.
 *
 * Silently skips: filenames that don't match `VALID_TASK_ID`, files that fail
 * to read/parse, and records that do not use the current task shape.
 */
export async function listBackgroundTasks(
  agentDir: string,
): Promise<BackgroundTaskInfo[]> {
  const dir = tasksDirOf(agentDir);
  let entries: import('node:fs').Dirent[];
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  const out: BackgroundTaskInfo[] = [];
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith('.json')) continue;
    const id = entry.name.slice(0, -'.json'.length);
    if (!VALID_TASK_ID.test(id)) continue;
    let parsed: unknown;
    try {
      parsed = JSON.parse(await readFile(join(dir, entry.name), 'utf8'));
    } catch {
      continue;
    }
    if (!isReadablePersistedTask(parsed)) continue;
    out.push(parsed);
  }
  // Newest first; tasks with no start time sort last.
  out.sort((a, b) => (b.startedAt ?? 0) - (a.startedAt ?? 0));
  return out;
}

/** Byte size of a task's `output.log` (0 when absent or unreadable). */
export async function taskOutputSizeBytes(
  agentDir: string,
  taskId: string,
): Promise<number> {
  try {
    return (await stat(taskOutputFile(agentDir, taskId))).size;
  } catch {
    return 0;
  }
}

export interface TaskOutputWindow {
  /** Byte offset this window starts at (clamped to >= 0). */
  offset: number;
  /** Byte offset immediately after this window — pass it as the next
   *  `offset` to page forward. Exact (server-computed bytesRead), so paging
   *  never drifts even if a window boundary splits a multi-byte char. */
  nextOffset: number;
  /** Total byte size of the log on disk. */
  size: number;
  /** UTF-8 decoded window content. */
  content: string;
  /** True when this window reaches EOF. */
  eof: boolean;
}

/**
 * Read a byte window of a task's `output.log`.
 *
 * Reads at most `maxBytes` bytes starting at byte `offset`. A window past EOF
 * is clamped to whatever remains; an offset at/after EOF yields empty content.
 * Mirrors agent-core's `readTaskOutputBytes` so large logs page identically.
 */
export async function readTaskOutput(
  agentDir: string,
  taskId: string,
  offset: number,
  maxBytes: number,
): Promise<TaskOutputWindow> {
  const start = Math.max(0, Math.trunc(offset));
  const limit = Math.max(0, Math.trunc(maxBytes));
  let handle;
  try {
    handle = await open(taskOutputFile(agentDir, taskId), 'r');
  } catch {
    return { offset: start, nextOffset: start, size: 0, content: '', eof: true };
  }
  try {
    const size = (await handle.stat()).size;
    if (limit === 0 || start >= size) {
      return { offset: start, nextOffset: start, size, content: '', eof: start >= size };
    }
    const length = Math.min(limit, size - start);
    const buffer = Buffer.allocUnsafe(length);
    const { bytesRead } = await handle.read(buffer, 0, length, start);
    const content = buffer.toString('utf-8', 0, bytesRead);
    const nextOffset = start + bytesRead;
    return { offset: start, nextOffset, size, content, eof: nextOffset >= size };
  } catch {
    return { offset: start, nextOffset: start, size: 0, content: '', eof: true };
  } finally {
    await handle.close();
  }
}

function isReadablePersistedTask(obj: unknown): obj is BackgroundTaskInfo {
  return isRecord(obj) && typeof obj['taskId'] === 'string';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
