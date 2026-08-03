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
  const batch = JSON.parse(await session.run()) as {
    events: EngineEventBatch['events'];
    progress: { status: string; outcome?: EngineEventBatch['outcome'] };
  };
  return {
    events: batch.events,
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
});
