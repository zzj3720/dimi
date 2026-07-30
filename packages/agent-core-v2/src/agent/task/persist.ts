/**
 * `task` domain (L5) — `AgentTaskPersistence`, the per-agent
 * persistence helper behind `AgentTaskService`.
 *
 * Persists task state (`<taskId>.json`) and raw task output (`output.log`)
 * through the `storage` access-pattern stores (`IAtomicDocumentStore` for
 * atomic whole-document state, `IFileSystemStorageService` byte primitives for ordered
 * output append), addressed under the owning agent's storage scope
 * (`<sessionScope>/agents/<agentId>/tasks/…`) so the domain never touches the
 * filesystem and each agent reads back exactly its own records. Task ids are
 * validated before use as path segments. Not scope-bound; constructed by
 * `AgentTaskService`.
 */

import { join } from 'pathe';

import type { IAtomicDocumentStore } from '#/persistence/interface/atomicDocumentStore';
import type { IFileSystemStorageService } from '#/persistence/interface/storage';

import type { AgentTaskInfo } from './types';

const VALID_TASK_ID: RegExp = /^[a-z0-9]+(?:-[a-z0-9]+)*-[0-9a-z]{8}$/;

const TASKS_SCOPE = 'tasks';
const OUTPUT_LOG_KEY = 'output.log';
const JSON_SUFFIX = '.json';

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

type PersistedTask = AgentTaskInfo;

export interface AgentTaskStoredOutputSnapshot {
  readonly outputPath: string;
  readonly outputSizeBytes: number;
  readonly previewBytes: number;
  readonly truncated: boolean;
  readonly preview: string;
}

function validateTaskId(taskId: string): void {
  if (!VALID_TASK_ID.test(taskId)) {
    throw new Error(`Invalid task id: "${taskId}"`);
  }
}

export class AgentTaskPersistence {
  constructor(
    private readonly agentDir: string,
    private readonly agentScope: string,
    private readonly docs: IAtomicDocumentStore,
    private readonly bytes: IFileSystemStorageService,
  ) {}

  private tasksScope(): string {
    return `${this.agentScope}/${TASKS_SCOPE}`;
  }

  private taskOutputScope(taskId: string): string {
    validateTaskId(taskId);
    return `${this.agentScope}/${TASKS_SCOPE}/${taskId}`;
  }

  taskOutputFile(taskId: string): string {
    validateTaskId(taskId);
    return join(this.agentDir, TASKS_SCOPE, taskId, OUTPUT_LOG_KEY);
  }

  async writeTask(task: PersistedTask): Promise<void> {
    validateTaskId(task.taskId);
    await this.docs.set(this.tasksScope(), `${task.taskId}${JSON_SUFFIX}`, task);
  }

  async readTask(taskId: string): Promise<PersistedTask | undefined> {
    validateTaskId(taskId);
    const key = `${taskId}${JSON_SUFFIX}`;
    const task = await this.docs.get<PersistedTask>(this.tasksScope(), key);
    return isReadablePersistedTask(task) ? task : undefined;
  }

  async appendTaskOutput(taskId: string, chunk: string): Promise<void> {
    if (chunk.length === 0) return;
    await this.bytes.append(this.taskOutputScope(taskId), OUTPUT_LOG_KEY, textEncoder.encode(chunk));
  }

  async taskOutputSizeBytes(taskId: string): Promise<number> {
    return (await this.readTaskOutputData(taskId))?.byteLength ?? 0;
  }

  async taskOutputExists(taskId: string): Promise<boolean> {
    return (await this.readTaskOutputData(taskId)) !== undefined;
  }

  async readTaskOutputBytes(taskId: string, offset: number, maxBytes: number): Promise<string> {
    const start = Math.max(0, Math.trunc(offset));
    const limit = Math.max(0, Math.trunc(maxBytes));
    if (limit === 0) return '';
    const output = await this.readTaskOutputData(taskId);
    if (output === undefined || start >= output.byteLength) return '';
    const end = Math.min(output.byteLength, start + limit);
    return textDecoder.decode(output.subarray(start, end));
  }

  async readTaskOutputSnapshot(
    taskId: string,
    maxPreviewBytes: number,
  ): Promise<AgentTaskStoredOutputSnapshot | undefined> {
    const output = await this.readTaskOutputData(taskId);
    if (output === undefined) return undefined;
    const previewLimit = Math.max(0, Math.trunc(maxPreviewBytes));
    const previewBytes = Math.min(previewLimit, output.byteLength);
    const previewOffset = output.byteLength - previewBytes;
    return {
      outputPath: this.taskOutputFile(taskId),
      outputSizeBytes: output.byteLength,
      previewBytes,
      truncated: previewOffset > 0,
      preview: textDecoder.decode(output.subarray(previewOffset)),
    };
  }

  async listTasks(): Promise<readonly PersistedTask[]> {
    const keys = (await this.docs.list(this.tasksScope())).toSorted();
    const tasks: PersistedTask[] = [];
    for (const key of keys) {
      if (!key.endsWith(JSON_SUFFIX)) continue;
      const id = key.slice(0, -JSON_SUFFIX.length);
      if (!VALID_TASK_ID.test(id)) continue;
      let task: PersistedTask | undefined;
      try {
        task = await this.docs.get<PersistedTask>(this.tasksScope(), key);
      } catch {
        continue;
      }
      if (task === undefined || !isReadablePersistedTask(task)) continue;
      tasks.push(task);
    }
    return tasks.toSorted((a, b) => a.taskId.localeCompare(b.taskId));
  }

  private readTaskOutputData(taskId: string): Promise<Uint8Array | undefined> {
    return this.bytes.read(this.taskOutputScope(taskId), OUTPUT_LOG_KEY);
  }
}

function isReadablePersistedTask(obj: unknown): obj is PersistedTask {
  return isRecord(obj) && typeof obj['taskId'] === 'string';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
