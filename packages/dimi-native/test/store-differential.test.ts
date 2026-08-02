/**
 * dimi-store ↔ packages/transcript differential suite (M1).
 *
 * The same op batches and item lists run through the TS implementation
 * (`@dimi-agent/transcript`) and the Rust bridge; snapshots and page
 * results must be byte-identical. The cold-rebuild differential (which
 * needs the engine's `reduceContextTranscript`) lives in kap-server's test
 * suite.
 *
 * Skips itself when the native binding is not built (same policy as the
 * other suites).
 */
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

import {
  AgentTranscript,
  paginateTurns as tsPaginateTurns,
  type TurnPageQuery,
} from '@dimi-agent/transcript';

import { paginateTurns, RustAgentTranscript } from '#/index';

const bindingPath = fileURLToPath(new URL('../dist/dimi_bridge.node', import.meta.url));
const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

type Json = unknown;

function applyAllTs(agent: { apply(ops: Json): { accepted?: Json; gap?: Json } }, batches: Json[]) {
  let gap: Json | undefined;
  let acceptedTotal = 0;
  for (const batch of batches) {
    const result = agent.apply(batch);
    acceptedTotal += (result.accepted as unknown[] | undefined)?.length ?? 0;
    gap = result.gap;
  }
  return { acceptedTotal, gap };
}

function applyAllRust(agent: { apply(opsJson: string): string }, batches: Json[]) {
  let gap: Json | undefined;
  let acceptedTotal = 0;
  for (const batch of batches) {
    const result = JSON.parse(agent.apply(JSON.stringify(batch))) as {
      accepted?: Json;
      gap?: Json;
    };
    acceptedTotal += (result.accepted as unknown[] | undefined)?.length ?? 0;
    gap = result.gap ?? undefined;
  }
  return { acceptedTotal, gap };
}

const turnA = {
  op: 'turn.upsert',
  turn: {
    kind: 'turn',
    turnId: 't0',
    ordinal: 0,
    state: 'running',
    origin: { kind: 'user' },
    prompt: 'hello',
  },
};

const turnACompleted = {
  op: 'turn.upsert',
  turn: {
    kind: 'turn',
    turnId: 't0',
    ordinal: 0,
    state: 'completed',
    origin: { kind: 'user' },
    prompt: 'hello',
    startedAt: '2026-08-01T10:00:00.000Z',
    endedAt: '2026-08-01T10:00:01.000Z',
  },
};

const stepA = {
  op: 'step.upsert',
  turnId: 't0',
  step: {
    kind: 'step',
    stepId: 't0.1',
    turnId: 't0',
    ordinal: 1,
    state: 'running',
  },
};

const frameA = {
  op: 'frame.upsert',
  turnId: 't0',
  stepId: 't0.1',
  frame: {
    kind: 'text',
    frameId: 't0.1.f1',
    role: 'assistant',
    text: 'Hel',
  },
};

const appendHello = (offset: number, text: string) => ({
  op: 'append',
  target: { type: 'frame', turnId: 't0', stepId: 't0.1', frameId: 't0.1.f1' },
  offset,
  text,
});

const taskUpsert = {
  op: 'task.upsert',
  task: {
    taskId: 'task_1',
    kind: 'shell',
    state: 'running',
    detached: false,
    outputTail: '',
  },
};

const markerUpsert = (beforeTurn?: number) => ({
  op: 'marker.upsert',
  item: { kind: 'marker', markerId: 'm1', marker: 'goal', payload: { objective: 'x' } },
  ...(beforeTurn === undefined ? {} : { beforeTurn }),
});

suite('dimi-store ↔ packages/transcript differential', () => {
  test('apply: full turn construction stream converges byte-exactly', () => {
    const batches: Json[] = [
      [turnA],
      [stepA],
      [frameA],
      [appendHello(3, 'lo world')],
      [appendHello(10, '!')],
      [taskUpsert],
      [turnACompleted],
    ];

    const ts = new AgentTranscript('main');
    const rust = new RustAgentTranscript('main');
    const tsResult = applyAllTs(ts as never, batches);
    const rustResult = applyAllRust(rust as never, batches);

    expect(rustResult.gap ?? null).toEqual(tsResult.gap ?? null);
    expect(rustResult.acceptedTotal).toBe(tsResult.acceptedTotal);

    const tsSnapshot = JSON.stringify(ts.snapshot());
    const rustSnapshot = rust.snapshot();
    expect(JSON.parse(rustSnapshot), 'snapshot mismatch').toEqual(JSON.parse(tsSnapshot));
  });

  test('apply: append idempotency and gap parity', () => {
    const batches: Json[] = [
      [turnA],
      [stepA],
      [frameA],
      [appendHello(3, 'lo world')],
      // Duplicate delivery of the same chunk → no change on both sides.
      [appendHello(3, 'lo world')],
      // Offset beyond local length → gap on both sides, state unchanged.
      [appendHello(99, 'stale')],
      // Diverged overlap → gap, never a silent rewrite.
      [appendHello(2, 'ZZZ')],
    ];

    const ts = new AgentTranscript('main');
    const rust = new RustAgentTranscript('main');
    const tsResult = applyAllTs(ts as never, batches);
    const rustResult = applyAllRust(rust as never, batches);

    expect(rustResult.gap ?? null).toEqual(tsResult.gap ?? null);
    expect(rustResult.acceptedTotal).toBe(tsResult.acceptedTotal);
    expect(JSON.parse(rust.snapshot())).toEqual(JSON.parse(JSON.stringify(ts.snapshot())));
  });

  test('apply: marker beforeTurn anchoring and task append creation', () => {
    const batches: Json[] = [
      [turnA],
      [{ op: 'turn.upsert', turn: { kind: 'turn', turnId: 't1', ordinal: 1, state: 'queued', origin: { kind: 'user' } } }],
      [markerUpsert(1)],
      [markerUpsert()],
      [{ op: 'append', target: { type: 'task', taskId: 'task_new' }, offset: 0, text: 'tail' }],
    ];

    const ts = new AgentTranscript('main');
    const rust = new RustAgentTranscript('main');
    applyAllTs(ts as never, batches);
    applyAllRust(rust as never, batches);
    expect(JSON.parse(rust.snapshot())).toEqual(JSON.parse(JSON.stringify(ts.snapshot())));
  });

  test('apply: meta.merge null-clears modes and shallow-merges agent', () => {
    const batches: Json[] = [
      [
        {
          op: 'meta.merge',
          meta: {
            modes: { plan: { reviewPath: '/p.md', version: 1 }, swarm: { trigger: 'x' } },
            agent: { model: 'gpt-x', contextTokens: 10 },
          },
        },
      ],
      [
        {
          op: 'meta.merge',
          meta: {
            modes: { plan: null },
            agent: { contextTokens: 20 },
          },
        },
      ],
    ];

    const ts = new AgentTranscript('main');
    const rust = new RustAgentTranscript('main');
    applyAllTs(ts as never, batches);
    applyAllRust(rust as never, batches);
    expect(JSON.parse(rust.snapshot())).toEqual(JSON.parse(JSON.stringify(ts.snapshot())));
  });

  test('apply: items.remove cascades interactions', () => {
    const batches: Json[] = [
      [turnA],
      [stepA],
      [
        {
          op: 'frame.upsert',
          turnId: 't0',
          stepId: 't0.1',
          frame: {
            kind: 'tool',
            frameId: 't0.1.call_1',
            toolCallId: 'call_1',
            name: 'bash',
            state: 'done',
          },
        },
      ],
      [
        {
          op: 'interaction.upsert',
          interaction: {
            interactionId: 'i_1',
            interactionKind: 'approval',
            toolCallId: 'call_1',
            state: 'pending',
          },
        },
      ],
      [{ op: 'items.remove', ids: ['t0'] }],
    ];

    const ts = new AgentTranscript('main');
    const rust = new RustAgentTranscript('main');
    applyAllTs(ts as never, batches);
    applyAllRust(rust as never, batches);
    expect(JSON.parse(rust.snapshot())).toEqual(JSON.parse(JSON.stringify(ts.snapshot())));
  });

  test('apply: reset replaces the whole state', () => {
    const resetOp = {
      op: 'reset',
      agentId: 'main',
      snapshot: {
        items: [],
        tasks: [],
        interactions: [],
        attachments: [],
        todos: [],
        prompts: [],
        meta: { goal: { objective: 'g', status: 'active' } },
        hasMoreOlder: true,
      },
    };
    const ts = new AgentTranscript('main');
    const rust = new RustAgentTranscript('main');
    applyAllTs(ts as never, [[turnA], [resetOp]]);
    applyAllRust(rust as never, [[turnA], [resetOp]]);
    expect(JSON.parse(rust.snapshot())).toEqual(JSON.parse(JSON.stringify(ts.snapshot())));
  });

  test('pagination: older/newer pages match', () => {
    const itemsJson = rustBuildItems();
    const rustItems = JSON.parse(itemsJson) as unknown[];

    const queries: TurnPageQuery[] = [
      { pageSize: 1 },
      { pageSize: 2 },
      { beforeTurn: 't2', pageSize: 2 },
      { afterTurn: 't0', pageSize: 1 },
    ];
    for (const query of queries) {
      const ts = JSON.stringify(tsPaginateTurns(rustItems as never, query));
      const rust = paginateTurns(itemsJson, JSON.stringify(query));
      expect(JSON.parse(rust), `page mismatch for ${JSON.stringify(query)}`).toEqual(JSON.parse(ts));
    }
  });
});

/** Three turns plus leading/trailing markers, via the Rust store itself. */
function rustBuildItems(): string {
  const rust = new RustAgentTranscript('main');
  rust.apply(
    JSON.stringify([
      { op: 'marker.upsert', item: { kind: 'marker', markerId: 'm0', marker: 'head' } },
      { op: 'turn.upsert', turn: { kind: 'turn', turnId: 't0', ordinal: 0, state: 'completed', origin: { kind: 'user' } } },
      { op: 'turn.upsert', turn: { kind: 'turn', turnId: 't1', ordinal: 1, state: 'completed', origin: { kind: 'user' } } },
      { op: 'turn.upsert', turn: { kind: 'turn', turnId: 't2', ordinal: 2, state: 'completed', origin: { kind: 'user' } } },
      { op: 'marker.upsert', item: { kind: 'marker', markerId: 'm1', marker: 'tail' } },
    ]),
  );
  return JSON.stringify(JSON.parse(rust.snapshot()).items);
}

if (!nativeAvailable) {
  console.warn(
    '[dimi-native] native binding not built — store differential suite skipped. Run `pnpm --filter @dimi-agent/dimi-native run build:native` to enable it.',
  );
}
