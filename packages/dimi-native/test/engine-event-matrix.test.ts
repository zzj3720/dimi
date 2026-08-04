/**
 * Engine event-sequence golden matrix (A3+A4 architecture review).
 *
 * The engine differential suite used to cover only auto-mode simple paths —
 * every complex path (approval pause/resume, deny, cancel races, stop_turn +
 * skipped siblings, session-scope approvals) lived outside any machine gate,
 * so transcript/event-order defects were only ever found by the next human
 * review round. This suite drives the napi `RustTurnSession` through those
 * paths with scripted segments and pins the COMPLETE ordered event-type
 * sequence (plus key payloads) for each — the golden matrix.
 *
 * The sequences below mirror the TS loop's event stream (verified against
 * `loopService`/`toolExecutorService` semantics in the P1/P2 review rounds);
 * any divergence in emission order or missing/duplicate events turns red.
 */
import { describe, expect, test } from 'vitest';

import { RustTurnSession } from '#/index';

interface EngineEventBatch {
  events: Array<Record<string, unknown>>;
  outcome: {
    status: string;
    steps: number;
    error?: string;
    errorCode?: string;
  };
}

interface ApprovalFlow {
  approvals: Array<Record<string, unknown>>;
  events: Array<Record<string, unknown>>;
  outcome: EngineEventBatch['outcome'];
}

type ApprovalDecision = Record<string, unknown>;

function toolCallSegment(calls: Array<{ id: string; name: string; args: string }>) {
  return [
    ...calls.map((call) => ({
      type: 'tool_call',
      toolCallId: call.id,
      name: call.name,
      argumentsPart: call.args,
    })),
    { type: 'finish', finishReason: 'tool_calls' },
  ];
}

async function runApprovalTurn(
  messages: Array<Record<string, unknown>>,
  segments: Array<Array<Record<string, unknown>>>,
  options: {
    policy?: Record<string, unknown>;
    decisions?: ApprovalDecision[];
    onApproval?: (approval: Record<string, unknown>) => void;
  } = {},
): Promise<ApprovalFlow> {
  const session = new RustTurnSession(
    JSON.stringify({
      turnId: 1,
      messages,
      tools: [],
      provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
      maxStepsPerTurn: null,
      cwd: '/tmp',
      shell: '/bin/sh',
    }),
    JSON.stringify(options.policy ?? { mode: 'manual', rules: [], sessionApprovedPatterns: [] }),
    JSON.stringify(segments),
    'test-registry',
  );
  const events: Array<Record<string, unknown>> = [];
  session.setOnEvent((eventJson: string) => {
    events.push(JSON.parse(eventJson) as Record<string, unknown>);
  });
  const approvals: Array<Record<string, unknown>> = [];
  let progress = JSON.parse(await session.run()) as {
    progress: { status: string; approval?: Record<string, unknown>; outcome?: EngineEventBatch['outcome'] };
  };
  const decisions = [...(options.decisions ?? [])];
  while (progress.progress.status === 'needsApproval') {
    const approval = progress.progress.approval!;
    approvals.push(approval);
    options.onApproval?.(approval);
    const decision = decisions.shift() ?? { decision: 'approved' };
    progress = JSON.parse(await session.resume(JSON.stringify(decision))) as typeof progress;
  }
  return {
    approvals,
    events,
    outcome: progress.progress.outcome ?? { status: progress.progress.status, steps: 0 },
  };
}

function eventNames(flow: ApprovalFlow): string[] {
  return flow.events.map((event) => event['type'] as string);
}

/** Structural loop events only — `tool.progress` (Bash status updates) is a
 *  tool-implementation detail, not loop structure; it is asserted separately. */
function structuralNames(flow: ApprovalFlow): string[] {
  return eventNames(flow).filter((name) => name !== 'tool.progress');
}

function userMessage(text: string): Record<string, unknown> {
  return { role: 'user', content: text };
}

const BASH = (id: string, command: string) => ({
  id,
  name: 'Bash',
  args: JSON.stringify({ command }),
});
const READ = (id: string, path: string) => ({
  id,
  name: 'Read',
  args: JSON.stringify({ path }),
});

describe('engine event-sequence golden matrix', () => {
  test('approval pause/resume keeps the step open and completes it after the batch', async () => {
    // Bash asks (manual), Read auto-approves. The paused step must emit its
    // TurnStepCompleted only after the resumed batch resolves, and exactly
    // once per call (started after the decision).
    const flow = await runApprovalTurn(
      [userMessage('run both')],
      [
        toolCallSegment([BASH('call_bash', 'echo hi'), READ('call_read', '/tmp/a.txt')]),
        [{ type: 'text', delta: 'done' }, { type: 'finish', finishReason: 'stop' }],
      ],
      { policy: { mode: 'manual', rules: [], sessionApprovedPatterns: [] } },
    );
    expect(flow.outcome.status).toBe('completed');
    expect(flow.approvals.map((a) => a['toolCallId'])).toEqual(['call_bash']);
    expect(structuralNames(flow)).toEqual([
      'turn.started',
      'turn.step.started',
      'tool.call.delta',
      'tool.call.delta',
      // pause: no started until the decision
      'tool.call.started', // call_bash (resume approved)
      'tool.result',
      'tool.call.started', // call_read (sibling, auto-approve)
      'tool.result',
      'turn.step.completed', // step 1 closes with tool_use
      'turn.step.started', // step 2
      'assistant.delta',
      'turn.step.completed',
      'turn.ended',
    ]);
    const step1 = flow.events.find(
      (e) => e['type'] === 'turn.step.completed' && e['step'] === 1,
    );
    expect(step1?.['finishReason']).toBe('tool_use');
  });

  test('rejected approval announces the call once, after the decision, before the result', async () => {
    const flow = await runApprovalTurn(
      [userMessage('run both')],
      [
        toolCallSegment([BASH('call_bash', 'echo hi'), READ('call_read', '/tmp/a.txt')]),
        [{ type: 'text', delta: 'done' }, { type: 'finish', finishReason: 'stop' }],
      ],
      {
        policy: { mode: 'manual', rules: [], sessionApprovedPatterns: [] },
        decisions: [{ decision: 'rejected', feedback: 'no thanks' }],
      },
    );
    expect(flow.outcome.status).toBe('completed');
    const names = structuralNames(flow);
    const bashStarted = names.indexOf('tool.call.started');
    const bashResult = names.indexOf('tool.result');
    expect(bashStarted).toBeGreaterThanOrEqual(0);
    expect(bashResult).toBeGreaterThan(bashStarted);
    // Exactly one started for the rejected call.
    expect(names.filter((n) => n === 'tool.call.started').length).toBe(2); // bash + read sibling
    const bashResultEvent = flow.events[names.indexOf('tool.result')];
    expect(bashResultEvent?.['isError']).toBe(true);
  });

  test('cancel during a pending approval finishes cancelled and interrupts the step', async () => {
    const session = new RustTurnSession(
      JSON.stringify({
        turnId: 1,
        messages: [userMessage('run both')],
        tools: [],
        provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
        maxStepsPerTurn: null,
        cwd: '/tmp',
        shell: '/bin/sh',
      }),
      JSON.stringify({ mode: 'manual', rules: [], sessionApprovedPatterns: [] }),
      JSON.stringify([
        toolCallSegment([BASH('call_bash_1', 'echo 1'), BASH('call_bash_2', 'echo 2')]),
        [{ type: 'text', delta: 'done' }, { type: 'finish', finishReason: 'stop' }],
      ]),
      'test-registry',
    );
    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });
    const first = JSON.parse(await session.run()) as {
      progress: { status: string; approval?: Record<string, unknown>; outcome?: EngineEventBatch['outcome'] };
    };
    expect(first.progress.status).toBe('needsApproval');
    session.cancel();
    const second = JSON.parse(await session.resume(JSON.stringify({ decision: 'cancelled' }))) as {
      progress: { status: string; outcome?: EngineEventBatch['outcome'] };
    };
    expect(second.progress.outcome?.status).toBe('cancelled');
    expect(events.map((e) => e['type'])).toEqual([
      'turn.started',
      'turn.step.started',
      'tool.call.delta',
      'tool.call.delta',
      'turn.step.interrupted', // the paused step is interrupted (aborted)
      'turn.ended',
    ]);
    const interrupted = events.find((e) => e['type'] === 'turn.step.interrupted');
    expect(interrupted?.['reason']).toBe('aborted');
  });

  test('user deny rule emits started + error result and the sibling still runs', async () => {
    const flow = await runApprovalTurn(
      [userMessage('run both')],
      [
        toolCallSegment([BASH('call_bash', 'echo hi'), READ('call_read', '/tmp/a.txt')]),
        [{ type: 'text', delta: 'done' }, { type: 'finish', finishReason: 'stop' }],
      ],
      {
        policy: {
          mode: 'manual',
          rules: [{ decision: 'deny', scope: 'user', pattern: 'Bash', reason: 'blocked' }],
          sessionApprovedPatterns: [],
        },
      },
    );
    expect(flow.outcome.status).toBe('completed');
    expect(structuralNames(flow)).toEqual([
      'turn.started',
      'turn.step.started',
      'tool.call.delta',
      'tool.call.delta',
      'tool.call.started',
      'tool.result', // Bash denied
      'tool.call.started',
      'tool.result', // Read approved
      'turn.step.completed',
      'turn.step.started',
      'assistant.delta',
      'turn.step.completed',
      'turn.ended',
    ]);
    const bashResult = flow.events.find(
      (e) => e['type'] === 'tool.result' && e['toolCallId'] === 'call_bash',
    );
    expect(bashResult?.['isError']).toBe(true);
    expect(String(bashResult?.['output'])).toContain('denied by permission rule. Reason: blocked');
  });

  test('stop_turn in a batch synthesizes announced skipped siblings and closes the turn', async () => {
    // WaitFor with a missing agent_id errors with stop_turn=true (the only
    // native tool that stops the turn without an approval), so the Read
    // sibling is skipped and announced (P2-1).
    const flow = await runApprovalTurn(
      [userMessage('run both')],
      [
        toolCallSegment([
          { id: 'call_wait', name: 'WaitFor', args: '{"timeout_seconds":1}' },
          READ('call_read', '/tmp/a.txt'),
        ]),
      ],
      { policy: { mode: 'auto', rules: [], sessionApprovedPatterns: [] } },
    );
    expect(flow.outcome.status).toBe('completed');
    expect(structuralNames(flow)).toEqual([
      'turn.started',
      'turn.step.started',
      'tool.call.delta',
      'tool.call.delta',
      'tool.call.started',
      'tool.result', // WaitFor errors + stops the turn
      'tool.call.started', // skipped sibling announced (P2-1)
      'tool.result', // skipped sibling synthetic result
      'turn.step.completed', // end_turn
      'turn.ended',
    ]);
    const skipped = flow.events.find(
      (e) => e['type'] === 'tool.result' && e['toolCallId'] === 'call_read',
    );
    expect(String(skipped?.['output'])).toBe(
      'Tool skipped because a previous tool call stopped the turn.',
    );
  });

  test('session-scope approval auto-approves the same tool later in the turn', async () => {
    // P1-6: after approving Bash for the session, the second Bash call in
    // the same batch must not re-ask. The runner calls addSessionApproval
    // before resuming; drive it explicitly here.
    const session = new RustTurnSession(
      JSON.stringify({
        turnId: 1,
        messages: [userMessage('run both')],
        tools: [],
        provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
        maxStepsPerTurn: null,
        cwd: '/tmp',
        shell: '/bin/sh',
      }),
      JSON.stringify({ mode: 'manual', rules: [], sessionApprovedPatterns: [] }),
      JSON.stringify([
        toolCallSegment([BASH('call_bash_1', 'echo 1'), BASH('call_bash_2', 'echo 2')]),
        [{ type: 'text', delta: 'done' }, { type: 'finish', finishReason: 'stop' }],
      ]),
      'test-registry',
    );
    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });
    const first = JSON.parse(await session.run()) as {
      progress: { status: string; approval?: Record<string, unknown>; outcome?: EngineEventBatch['outcome'] };
    };
    expect(first.progress.status).toBe('needsApproval');
    expect(first.progress.approval?.['toolCallId']).toBe('call_bash_1');
    // The runner calls addSessionApproval before resuming (P1-6).
    session.addSessionApproval('Bash');
    const second = JSON.parse(await session.resume(JSON.stringify({ decision: 'approved' }))) as {
      progress: { status: string; outcome?: EngineEventBatch['outcome'] };
    };
    expect(second.progress.outcome?.status).toBe('completed');
    // Only ONE approval surfaced — the second Bash auto-approved via the
    // session pattern (if it re-asked, the turn would have paused again).
    expect(events.filter((e) => e['type'] === 'tool.call.started').length).toBe(2);
    expect(events.filter((e) => e['type'] !== 'tool.progress').map((e) => e['type'])).toEqual([
      'turn.started',
      'turn.step.started',
      'tool.call.delta',
      'tool.call.delta',
      'tool.call.started',
      'tool.result',
      'tool.call.started',
      'tool.result',
      'turn.step.completed',
      'turn.step.started',
      'assistant.delta',
      'turn.step.completed',
      'turn.ended',
    ]);
  });

  test('max-steps failure emits no step.interrupted (TS parity)', async () => {
    const session = new RustTurnSession(
      JSON.stringify({
        turnId: 1,
        messages: [userMessage('loop')],
        tools: [],
        provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
        maxStepsPerTurn: 1,
        cwd: '/tmp',
        shell: '/bin/sh',
      }),
      JSON.stringify({ mode: 'auto', rules: [], sessionApprovedPatterns: [] }),
      JSON.stringify([
        toolCallSegment([BASH('call_1', 'echo x')]),
        toolCallSegment([BASH('call_2', 'echo y')]),
      ]),
      'test-registry',
    );
    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });
    const batch = JSON.parse(await session.run()) as {
      progress: { status: string; outcome?: EngineEventBatch['outcome'] };
    };
    expect(batch.progress.outcome?.status).toBe('failed');
    expect(batch.progress.outcome?.errorCode).toBe('LOOP_MAX_STEPS_EXCEEDED');
    expect(events.filter((e) => e['type'] !== 'tool.progress').map((e) => e['type'])).toEqual([
      'turn.started',
      'turn.step.started',
      'tool.call.delta',
      'tool.call.started',
      'tool.result',
      'turn.step.completed',
      'turn.ended',
    ]);
  });
});
