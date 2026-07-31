import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { describe, it, expect, afterEach } from 'vitest';

import { buildSessionFixture } from '../fixtures/build';
import {
  isSafeTaskId,
  listBackgroundTasks,
  readTaskOutput,
  taskOutputSizeBytes,
} from '../../src/lib/task-store';

async function writeTask(sessionDir: string, fileName: string, body: unknown): Promise<void> {
  const dir = join(sessionDir, 'tasks');
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, fileName), JSON.stringify(body));
}

describe('task-store', () => {
  let cleanup: (() => Promise<void>) | null = null;
  afterEach(async () => { if (cleanup) await cleanup(); cleanup = null; });

  it('lists current-shape tasks of every kind newest-first', async () => {
    const { sessionDir, cleanup: c } = await buildSessionFixture('sample-main');
    cleanup = c;

    await writeTask(sessionDir, 'bash-aaaaaaaa.json', {
      taskId: 'bash-aaaaaaaa', kind: 'process', description: 'run build',
      command: 'pnpm build', pid: 4242, exitCode: 0, status: 'completed',
      detached: true, startedAt: 1000, endedAt: 2000,
    });
    await writeTask(sessionDir, 'agent-bbbbbbbb.json', {
      taskId: 'agent-bbbbbbbb', kind: 'agent', description: 'explore repo',
      agentId: 'agent-1', subagentType: 'Explore', status: 'running',
      detached: true, startedAt: 3000, endedAt: null,
    });
    await writeTask(sessionDir, 'question-cccccccc.json', {
      taskId: 'question-cccccccc', kind: 'question', description: 'ask user',
      questionCount: 2, status: 'running', detached: false,
      startedAt: 2500, endedAt: null,
    });

    const tasks = await listBackgroundTasks(sessionDir);
    expect(tasks.map((t) => t.taskId)).toEqual([
      'agent-bbbbbbbb', // startedAt 3000
      'question-cccccccc', // 2500
      'bash-aaaaaaaa', // 1000
    ]);
    const proc = tasks.find((t) => t.kind === 'process');
    expect(proc).toMatchObject({ command: 'pnpm build', pid: 4242, exitCode: 0 });
    const question = tasks.find((t) => t.kind === 'question');
    expect(question).toMatchObject({ questionCount: 2, detached: false });
  });

  it('skips bad filenames, corrupt json, and unrecognized records', async () => {
    const { sessionDir, cleanup: c } = await buildSessionFixture('sample-main');
    cleanup = c;
    await writeTask(sessionDir, 'not-a-valid-id.json', { taskId: 'x', kind: 'process' });
    await mkdir(join(sessionDir, 'tasks'), { recursive: true });
    await writeFile(join(sessionDir, 'tasks', 'bash-ffffffff.json'), '{ broken');
    await writeTask(sessionDir, 'bash-99999999.json', { unrelated: true });
    expect(await listBackgroundTasks(sessionDir)).toEqual([]);
  });

  it('returns [] when there is no tasks directory', async () => {
    const { sessionDir, cleanup: c } = await buildSessionFixture('sample-main');
    cleanup = c;
    expect(await listBackgroundTasks(sessionDir)).toEqual([]);
  });

  it('reads output.log byte windows with size + eof', async () => {
    const { sessionDir, cleanup: c } = await buildSessionFixture('sample-main');
    cleanup = c;
    const dir = join(sessionDir, 'tasks', 'bash-12345678');
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, 'output.log'), 'hello world');

    expect(await taskOutputSizeBytes(sessionDir, 'bash-12345678')).toBe(11);

    const head = await readTaskOutput(sessionDir, 'bash-12345678', 0, 5);
    expect(head).toMatchObject({ offset: 0, nextOffset: 5, size: 11, content: 'hello', eof: false });

    // Paging forward from the previous window's nextOffset reaches EOF exactly.
    const tail = await readTaskOutput(sessionDir, 'bash-12345678', head.nextOffset, 100);
    expect(tail).toMatchObject({ offset: 5, nextOffset: 11, size: 11, content: ' world', eof: true });

    const past = await readTaskOutput(sessionDir, 'bash-12345678', 50, 10);
    expect(past).toMatchObject({ content: '', eof: true });
  });

  it('returns an empty window when the log is absent', async () => {
    const { sessionDir, cleanup: c } = await buildSessionFixture('sample-main');
    cleanup = c;
    const w = await readTaskOutput(sessionDir, 'bash-00000000', 0, 100);
    expect(w).toMatchObject({ size: 0, content: '', eof: true });
  });

  it('isSafeTaskId guards traversal', () => {
    expect(isSafeTaskId('bash-1a2b3c4d')).toBe(true);
    expect(isSafeTaskId('agent-deadbeef')).toBe(true);
    expect(isSafeTaskId('../escape')).toBe(false);
    expect(isSafeTaskId('bash')).toBe(false);
    expect(isSafeTaskId('bg_abcd')).toBe(false);
  });
});
