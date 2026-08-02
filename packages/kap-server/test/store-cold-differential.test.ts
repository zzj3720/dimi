/**
 * Cold-rebuild differential suite (M1): the engine's TS three-stage rebuild
 * vs the Rust `dimi-store` bridge.
 *
 * The cold path (transcriptService.readColdSnapshot / SnapshotReader) is:
 *
 *   readWireRecords(wire.jsonl)
 *     → reduceContextTranscript   (agent-core-v2, context.* records → messages)
 *     → groupMessagesIntoSnapshot (@dimi-agent/transcript, messages → turn tree)
 *     → foldWireRecordFacts       (@dimi-agent/transcript, non-context facts)
 *
 * `dimi-store` implements the same pipeline in Rust (`coldRebuild`). Each
 * record stream runs through both sides and the resulting snapshots must be
 * deep-equal — field names, key order (JSON stringify), markers, taskrefs,
 * attachments, meta and todos included.
 *
 * Skips itself when the native binding is not built (same policy as the
 * dimi-native suites); build with
 * `pnpm --filter @dimi-agent/dimi-native run build:native`.
 */
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

import { reduceContextTranscript, type WireRecord } from '@dimi-agent/agent-core-v2';
import { foldWireRecordFacts, groupMessagesIntoSnapshot } from '@dimi-agent/transcript';

import { coldRebuild } from '@dimi-agent/dimi-native';

const bindingPath = fileURLToPath(
  new URL('../../dimi-native/dist/dimi_bridge.node', import.meta.url),
);
const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

type Json = unknown;

/** TS three-stage rebuild — mirrors `readColdSnapshot` exactly. */
function tsColdRebuild(records: WireRecord[]): Json {
  const messages = [...reduceContextTranscript(records).entries];
  const base = groupMessagesIntoSnapshot(messages);
  return foldWireRecordFacts(records, base);
}

/** Rust bridge cold rebuild. */
function rustColdRebuild(records: WireRecord[]): Json {
  return JSON.parse(coldRebuild(JSON.stringify(records))) as Json;
}

/**
 * Compare at the wire level: `JSON.stringify` of both sides must be equal —
 * key order included. (Deep-equality would also flag TS's
 * `{ goal: undefined, modes: undefined }` meta keys, which JSON drops, so
 * the object-level comparison is intentionally byte-oriented.)
 */
function expectSameWire(ts: Json, rust: Json): void {
  expect(JSON.stringify(rust)).toBe(JSON.stringify(ts));
}

const T0 = 1_720_000_000_000;

/** A rich mixed stream: media, tool exchange with deferred message, vacuous
 * step settlement, compaction, undo, clear, tasks, interactions, todos,
 * goal/plan/swarm meta. */
const mixedStream: WireRecord[] = [
  // Journal envelope — skipped by the reducer.
  { type: 'metadata', protocol_version: '3', created_at: T0 },
  // User turn with a media attachment.
  {
    type: 'context.append_message',
    time: T0,
    message: {
      role: 'user',
      content: [
        { type: 'text', text: 'look at this' },
        { type: 'image', source: { kind: 'url', url: 'https://example.test/a.png' } },
      ],
      toolCalls: [],
    },
  },
  // Assistant step with a tool call + text, then the tool result.
  { type: 'context.append_loop_event', time: T0 + 100, event: { type: 'step.begin', uuid: 's1' } },
  {
    type: 'context.append_loop_event',
    time: T0 + 200,
    event: {
      type: 'tool.call',
      stepUuid: 's1',
      toolCallId: 'call_1',
      name: 'shell',
      args: { command: 'ls' },
      extras: { interactive: false },
    },
  },
  // While a tool result is pending this user message is deferred.
  {
    type: 'context.append_message',
    time: T0 + 250,
    message: { role: 'user', content: [{ type: 'text', text: 'deferred' }], toolCalls: [] },
  },
  {
    type: 'context.append_loop_event',
    time: T0 + 300,
    event: { type: 'content.part', stepUuid: 's1', part: { type: 'text', text: 'Running ls' } },
  },
  { type: 'context.append_loop_event', time: T0 + 400, event: { type: 'step.end', uuid: 's1' } },
  {
    type: 'context.append_loop_event',
    time: T0 + 500,
    event: { type: 'tool.result', toolCallId: 'call_1', result: { output: 'a.png', isError: false } },
  },
  // Assistant reply.
  {
    type: 'context.append_message',
    time: T0 + 600,
    message: { role: 'assistant', content: [{ type: 'text', text: 'Done' }], toolCalls: [] },
  },
  // A wholly-vacuous step — dropped at settle time.
  { type: 'context.append_loop_event', time: T0 + 700, event: { type: 'step.begin', uuid: 's2' } },
  {
    type: 'context.append_loop_event',
    time: T0 + 710,
    event: { type: 'content.part', stepUuid: 's2', part: { type: 'text', text: ' ' } },
  },
  { type: 'context.append_loop_event', time: T0 + 720, event: { type: 'step.end', uuid: 's2' } },
  // Compaction summary.
  {
    type: 'context.apply_compaction',
    time: T0 + 800,
    summary: 'Earlier turns summarized.',
    contextSummary: 'Earlier turns summarized.',
    compactedCount: 3,
    tokensBefore: 100,
    tokensAfter: 20,
    keptUserMessageCount: 2,
    keptHeadUserMessageCount: 1,
  },
  // Undo removes one user message.
  { type: 'context.undo', time: T0 + 900, count: 1 },
  // Clear wipes the folded history; later messages start a fresh floor.
  { type: 'context.clear', time: T0 + 1000 },
  {
    type: 'context.append_message',
    time: T0 + 1100,
    message: { role: 'user', content: [{ type: 'text', text: 'after clear' }], toolCalls: [] },
  },
  // Tasks: started + terminated, then a still-running subagent.
  {
    type: 'task.started',
    time: T0 + 1200,
    info: {
      taskId: 'task_1',
      kind: 'process',
      status: 'running',
      detached: false,
      description: 'run tests',
      agentId: 'main',
      startedAt: T0 + 1150,
    },
  },
  {
    type: 'task.terminated',
    time: T0 + 1300,
    info: {
      taskId: 'task_1',
      kind: 'process',
      status: 'completed',
      detached: false,
      agentId: 'main',
      startedAt: T0 + 1150,
      endedAt: T0 + 1290,
    },
    outputTail: 'ok',
  },
  {
    type: 'task.started',
    time: T0 + 1400,
    info: {
      taskId: 'task_2',
      kind: 'agent',
      status: 'running',
      detached: true,
      description: 'sub',
      agentId: 'sub-1',
    },
  },
  // Interactions: approved approval, answered question, a user_tool request
  // that must NOT project, and one left pending (crash → cancelled).
  {
    type: 'interaction.request',
    time: T0 + 1500,
    id: 'ia_1',
    kind: 'approval',
    toolCallId: 'call_2',
    request: { toolCallId: 'call_2', command: 'rm -rf /tmp/x' },
  },
  { type: 'interaction.resolved', time: T0 + 1600, id: 'ia_1', response: { decision: 'approved' } },
  {
    type: 'interaction.request',
    time: T0 + 1700,
    id: 'ia_2',
    kind: 'question',
    request: { question: 'Are you sure?' },
  },
  { type: 'interaction.resolved', time: T0 + 1800, id: 'ia_2', response: 'yes' },
  {
    type: 'interaction.request',
    time: T0 + 1850,
    id: 'ia_skip',
    kind: 'user_tool',
    toolCallId: 'call_9',
    request: { toolCallId: 'call_9', command: 'ignored' },
  },
  {
    type: 'interaction.request',
    time: T0 + 1900,
    id: 'ia_3',
    kind: 'approval',
    toolCallId: 'call_3',
    request: { toolCallId: 'call_3', command: 'rm' },
  },
  // Todo store update (malformed entries are dropped).
  {
    type: 'tools.update_store',
    time: T0 + 2000,
    key: 'todo',
    value: [
      { title: 'a', status: 'pending' },
      { title: 'b', status: 'done' },
      { title: 'bad', status: 'other' },
      'not-an-object',
    ],
  },
  // Goal lifecycle: create then update.
  { type: 'goal.create', time: T0 + 2100, objective: 'Ship it', completionCriterion: 'green' },
  {
    type: 'goal.update',
    time: T0 + 2200,
    status: 'paused',
    tokensUsed: 42,
    budgetLimits: { tokenBudget: 1000 },
  },
  // Plan mode with a revision, then exit.
  { type: 'plan_mode.enter', time: T0 + 2300 },
  {
    type: 'plan.revision',
    time: T0 + 2400,
    id: 'plan_1',
    version: 1,
    path: 'agents/main/plan/plan_1/v1.md',
    sha256: 'abc',
    bytes: 42,
  },
  { type: 'plan_mode.exit', time: T0 + 2500 },
  // Swarm mode round-trip.
  { type: 'swarm_mode.enter', time: T0 + 2600 },
  { type: 'swarm_mode.exit', time: T0 + 2700 },
];

suite('cold rebuild: TS three-stage vs Rust dimi-store', () => {
  test('mixed stream', () => {
    const ts = tsColdRebuild(mixedStream);
    const rust = rustColdRebuild(mixedStream);
    expectSameWire(ts, rust);
  });

  test('plain conversation with string timestamps', () => {
    // The engine always writes epoch-ms numbers, but the fold's
    // `recordTimeIso` also passes ISO strings through (defensive read path),
    // so the wire input may carry them — the TS `WireRecord` type only
    // models the engine-written form.
    const records: Array<Omit<WireRecord, 'time'> & { time?: number | string }> = [
      {
        type: 'context.append_message',
        time: '2026-08-01T10:00:00.000Z',
        message: { role: 'user', content: [{ type: 'text', text: 'hi' }], toolCalls: [] },
      },
      { type: 'context.append_loop_event', event: { type: 'step.begin', uuid: 's1' } },
      {
        type: 'context.append_loop_event',
        event: { type: 'content.part', stepUuid: 's1', part: { type: 'text', text: 'hello' } },
      },
      { type: 'context.append_loop_event', event: { type: 'step.end', uuid: 's1' } },
      {
        type: 'task.started',
        time: '2026-08-01T10:00:02.000Z',
        info: { taskId: 'task_x', kind: 'question', status: 'running', startedAt: 1000 },
      },
    ];
    const ts = tsColdRebuild(records as WireRecord[]);
    const rust = rustColdRebuild(records as WireRecord[]);
    expectSameWire(ts, rust);
  });

  test('empty journal', () => {
    const ts = tsColdRebuild([]);
    const rust = rustColdRebuild([]);
    expectSameWire(ts, rust);
  });

  test('goal cleared + plan cancelled leaves meta empty', () => {
    const records: WireRecord[] = [
      { type: 'goal.create', time: T0, objective: 'x' },
      { type: 'goal.clear', time: T0 + 100 },
      { type: 'plan_mode.enter', time: T0 + 200 },
      { type: 'plan_mode.cancel', time: T0 + 300 },
    ];
    const ts = tsColdRebuild(records);
    const rust = rustColdRebuild(records);
    expectSameWire(ts, rust);
  });
});
