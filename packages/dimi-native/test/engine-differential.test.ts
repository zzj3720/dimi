/**
 * `RustEngine` differential suite — the M3 swap-in socket.
 *
 * Drives the napi `RustEngine` with scripted LLM segments and asserts the
 * engine event batch: the same event shapes the TS loop publishes, which
 * the transcript projection layer folds into wire ops.
 */
import { describe, expect, test } from 'vitest';

import { RustEngine, RustTurnSession } from '#/index';

interface EngineEventBatch {
  events: Array<Record<string, unknown>>;
  outcome: {
    status: string;
    steps: number;
    error?: string;
    errorCode?: string;
    truncated?: boolean;
  };
}

async function runTurn(
  messages: Array<Record<string, unknown>>,
  segments: Array<Array<Record<string, unknown>>>,
  options: { maxStepsPerTurn?: number; cwd?: string } = {},
): Promise<EngineEventBatch> {
  const input = JSON.stringify({
    turnId: 1,
    messages,
    tools: [],
    provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
    maxStepsPerTurn: options.maxStepsPerTurn ?? null,
    cwd: options.cwd ?? '/tmp',
    shell: '/bin/sh',
  });
  // Auto policy: tools execute without approval (approval flows are covered
  // by the agent-core-v2 runner tests).
  const session = new RustTurnSession(
    input,
    JSON.stringify({ mode: 'auto', rules: [], sessionApprovedPatterns: [] }),
    JSON.stringify(segments),
  );
  // The engine streams every event through the per-event callback as it is
  // emitted; the response carries only the progress.
  const events: EngineEventBatch['events'] = [];
  session.setOnEvent((eventJson: string) => {
    events.push(JSON.parse(eventJson) as EngineEventBatch['events'][number]);
  });
  const batch = JSON.parse(await session.run()) as {
    events: EngineEventBatch['events'];
    progress: { status: string; outcome?: EngineEventBatch['outcome'] };
  };
  return {
    events,
    outcome: batch.progress.outcome ?? {
      status: batch.progress.status,
      steps: 0,
    },
  };
}

function userMessage(text: string): Record<string, unknown> {
  return { role: 'user', content: text };
}

function eventNames(batch: EngineEventBatch): string[] {
  return batch.events.map((event) => event['type'] as string);
}

/** Poll the event list for an event of `type` (spawned workers/pollers emit
 *  asynchronously after `run` resolves). */
async function waitForEvent(
  events: Array<Record<string, unknown>>,
  type: string,
  timeoutMs: number,
): Promise<Record<string, unknown>> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const event = events.find((candidate) => candidate['type'] === type);
    if (event !== undefined) return event;
    if (Date.now() >= deadline) throw new Error(`timeout waiting for ${type}`);
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

describe('RustEngine minimal closed loop', () => {
  test('single step completes with text', async () => {
    const batch = await runTurn([userMessage('hi')], [
      [
        { type: 'text', delta: 'Hello ' },
        { type: 'text', delta: 'world' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);

    expect(batch.outcome.status).toBe('completed');
    expect(batch.outcome.steps).toBe(1);
    expect(eventNames(batch)).toEqual([
      'turn.started',
      'turn.step.started',
      'assistant.delta',
      'assistant.delta',
      'turn.step.completed',
      'turn.ended',
    ]);
    const started = batch.events[0]!;
    expect(started['prompt']).toBe('hi');
    expect(started['origin']).toEqual({ kind: 'user' });
    const stepCompleted = batch.events[4]!;
    expect(stepCompleted['finishReason']).toBe('end_turn');
    const ended = batch.events[5]!;
    expect(ended['reason']).toBe('completed');
    expect(typeof ended['durationMs']).toBe('number');
  });

  test('tool call runs Bash and loops to completion', async () => {
    const batch = await runTurn([userMessage('run a command')], [
      [
        {
          type: 'tool_call',
          toolCallId: 'call_1',
          name: 'Bash',
          argumentsPart: '{"command":"echo engine-diff-ok"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'done' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);

    expect(batch.outcome.status).toBe('completed');
    expect(batch.outcome.steps).toBe(2);
    const names = eventNames(batch);
    expect(names).toContain('tool.call.started');
    expect(names).toContain('tool.result');
    const result = batch.events.find((event) => event['type'] === 'tool.result');
    expect(String(result?.['output'])).toContain('engine-diff-ok');
    expect(result?.['isError']).toBe(false);
    // The first step completed with finishReason tool_calls.
    const firstStepCompleted = batch.events.find(
      (event) => event['type'] === 'turn.step.completed' && event['step'] === 1,
    );
    expect(firstStepCompleted?.['finishReason']).toBe('tool_use');
  });

  test('tool error surfaces as isError result', async () => {
    const batch = await runTurn([userMessage('fail')], [
      [
        {
          type: 'tool_call',
          toolCallId: 'call_1',
          name: 'Bash',
          argumentsPart: '{"command":"exit 3"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'saw it' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);

    expect(batch.outcome.status).toBe('completed');
    const result = batch.events.find((event) => event['type'] === 'tool.result');
    expect(result?.['isError']).toBe(true);
    expect(String(result?.['output'])).toContain('Command failed with exit code: 3.');
  });

  test('max steps fails the turn', async () => {
    const batch = await runTurn(
      [userMessage('loop')],
      [
        [
          {
            type: 'tool_call',
            toolCallId: 'call_1',
            name: 'Bash',
            argumentsPart: '{"command":"echo x"}',
          },
          { type: 'finish', finishReason: 'tool_calls' },
        ],
        [
          {
            type: 'tool_call',
            toolCallId: 'call_2',
            name: 'Bash',
            argumentsPart: '{"command":"echo y"}',
          },
          { type: 'finish', finishReason: 'tool_calls' },
        ],
      ],
      { maxStepsPerTurn: 1 },
    );

    expect(batch.outcome.status).toBe('failed');
    expect(batch.outcome.errorCode).toBe('LOOP_MAX_STEPS_EXCEEDED');
    const interrupted = batch.events.find((event) => event['type'] === 'turn.step.interrupted');
    expect(interrupted?.['reason']).toBe('max_steps');
    const ended = batch.events[batch.events.length - 1]!;
    expect(ended['type']).toBe('turn.ended');
    expect(ended['reason']).toBe('failed');
  });

  test('thinking deltas are forwarded', async () => {
    const batch = await runTurn([userMessage('think')], [
      [
        { type: 'thinking', delta: 'hmm ' },
        { type: 'thinking', delta: 'hmm2' },
        { type: 'text', delta: 'answer' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);

    const names = eventNames(batch);
    expect(names).toContain('thinking.delta');
    expect(names.filter((name) => name === 'thinking.delta')).toHaveLength(2);
  });

  test('usage rides step completed', async () => {
    const batch = await runTurn([userMessage('usage')], [
      [
        { type: 'text', delta: 'x' },
        {
          type: 'usage',
          promptTokens: 10,
          completionTokens: 5,
          totalTokens: 15,
          promptTokensDetails: { cachedTokens: 2 },
          completionTokensDetails: { reasoningTokens: 1 },
        },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);

    const stepCompleted = batch.events.find((event) => event['type'] === 'turn.step.completed');
    expect(stepCompleted?.['usage']).toEqual({
      inputTokens: 12,
      outputTokens: 5,
      cachedTokens: 2,
    });
  });

  test('streams events to the callback while the turn is still running', async () => {
    // Step 1: Bash `sleep 0.5` — the tool window keeps the turn in flight
    // while the probe checks the callback; step 2: the final text answer.
    const session = new RustTurnSession(
      JSON.stringify({
        turnId: 1,
        messages: [{ role: 'user', content: 'stream' }],
        tools: [],
        provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
        maxStepsPerTurn: null,
        cwd: '/tmp',
        shell: '/bin/sh',
      }),
      JSON.stringify({ mode: 'auto', rules: [], sessionApprovedPatterns: [] }),
      JSON.stringify([
        [
          {
            type: 'tool_call',
            toolCallId: 'call_stream',
            name: 'Bash',
            argumentsPart: '{"command":"sleep 0.5"}',
          },
          { type: 'finish', finishReason: 'tool_calls' },
        ],
        [
          { type: 'text', delta: 'streamed done' },
          { type: 'finish', finishReason: 'stop' },
        ],
      ]),
    );
    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });

    const runPromise = session.run();
    // Probe mid-turn: the engine announces the tool call before the Bash
    // sleep starts, so the callback must already have it — while the tool
    // result (post-sleep) has not arrived yet.
    await new Promise((resolve) => setTimeout(resolve, 150));
    const typesDuringRun = events.map((event) => event['type']);
    expect(typesDuringRun).toContain('turn.started');
    expect(typesDuringRun).toContain('turn.step.started');
    expect(typesDuringRun).toContain('tool.call.started');
    expect(typesDuringRun).not.toContain('tool.result');
    expect(typesDuringRun).not.toContain('turn.ended');

    const batch = JSON.parse(await runPromise) as {
      events: Array<Record<string, unknown>>;
      progress: { status: string; outcome?: { status: string } };
    };
    expect(batch.progress.status).toBe('completed');
    // The response no longer carries the events — they streamed through the
    // callback in emission order (tool.call.started → tool.result →
    // turn.ended).
    expect(batch.events).toEqual([]);
    expect(events.map((event) => event['type'])).toEqual([
      'turn.started',
      'turn.step.started',
      'tool.call.delta',
      'tool.call.started',
      'tool.result',
      'turn.step.completed',
      'turn.step.started',
      'assistant.delta',
      'turn.step.completed',
      'turn.ended',
    ]);
  }, 30_000);

  test('subagent spawns and settles through the event stream with UUID ids', async () => {
    // Step 1: the Agent tool launches a background subagent (stop_turn); the
    // nested turn answers from the shared scripted client (segment 1). The
    // worker settles the task asynchronously after the main turn ended.
    const session = new RustTurnSession(
      JSON.stringify({
        turnId: 1,
        messages: [{ role: 'user', content: 'delegate' }],
        tools: [],
        provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
        maxStepsPerTurn: null,
        cwd: '/tmp',
        shell: '/bin/sh',
      }),
      JSON.stringify({ mode: 'auto', rules: [], sessionApprovedPatterns: [] }),
      JSON.stringify([
        [
          {
            type: 'tool_call',
            toolCallId: 'call_sub',
            name: 'Agent',
            argumentsPart: '{"prompt":"do the thing"}',
          },
          { type: 'finish', finishReason: 'tool_calls' },
        ],
        [
          { type: 'text', delta: 'subagent result text' },
          { type: 'finish', finishReason: 'stop' },
        ],
      ]),
    );
    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });
    const batch = JSON.parse(await session.run()) as { progress: { status: string } };
    expect(batch.progress.status).toBe('completed');

    const started = await waitForEvent(events, 'task.started', 5_000);
    expect(started['kind']).toBe('agent');
    // TS lifecycle shapes: agent-<n> (nextAvailableAgentId), task-<8-uuid-chars>
    // (generateTaskId).
    expect(String(started['agentId'])).toMatch(/^agent-\d+$/);
    expect(String(started['taskId'])).toMatch(/^task-[0-9a-f]{8}$/);
    expect(started['parentToolCallId']).toBe('call_sub');

    const settled = await waitForEvent(events, 'task.settled', 5_000);
    expect(settled['taskId']).toBe(started['taskId']);
    expect(settled['agentId']).toBe(started['agentId']);
    expect(settled['status']).toBe('completed');
    expect(String(settled['output'])).toBe('subagent result text');
  }, 30_000);

  test('backgrounded bash task settles through the event stream', async () => {
    // `sleep 2; echo bg-finished` with timeout 1: the command is moved to
    // the background (task.started) and the detached poller settles it
    // (task.settled) once it exits — the completion notification source for
    // the TS side.
    const session = new RustTurnSession(
      JSON.stringify({
        turnId: 1,
        messages: [{ role: 'user', content: 'run bg' }],
        tools: [],
        provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
        maxStepsPerTurn: null,
        cwd: '/tmp',
        shell: '/bin/sh',
      }),
      JSON.stringify({ mode: 'auto', rules: [], sessionApprovedPatterns: [] }),
      JSON.stringify([
        [
          {
            type: 'tool_call',
            toolCallId: 'call_bg',
            name: 'Bash',
            argumentsPart: '{"command":"sleep 2; echo bg-finished","timeout":1}',
          },
          { type: 'finish', finishReason: 'tool_calls' },
        ],
        [
          { type: 'text', delta: 'bg launched' },
          { type: 'finish', finishReason: 'stop' },
        ],
      ]),
    );
    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });
    const batch = JSON.parse(await session.run()) as { progress: { status: string } };
    expect(batch.progress.status).toBe('completed');

    const started = await waitForEvent(events, 'task.started', 5_000);
    expect(started['kind']).toBe('bash');
    expect(String(started['taskId'])).toMatch(/^bash-[0-9a-f]{8}$/);
    expect(String(started['agentId'])).toBe(started['taskId']);
    expect(typeof started['pid']).toBe('number');

    // The poller settles the task after the command exits (~2s).
    const settled = await waitForEvent(events, 'task.settled', 15_000);
    expect(settled['taskId']).toBe(started['taskId']);
    expect(settled['status']).toBe('completed');
    expect(String(settled['output'])).toContain('bg-finished');
    expect(settled['exitCode']).toBe(0);
  }, 30_000);
});
