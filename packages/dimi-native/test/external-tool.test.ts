/**
 * TS external tool injection — the MCP/plugin/skills bridge (slice 5).
 *
 * A TS-side tool is registered on the RustTurnSession; the engine routes
 * the tool call to the napi callback, the callback runs the TS tool and
 * completes via `completeToolCall`.
 */
import { describe, expect, test } from 'vitest';

import { RustTurnSession } from '#/index';

function makeSession(segments: Array<Array<Record<string, unknown>>>): RustTurnSession {
  const session = new RustTurnSession(
    JSON.stringify({
      turnId: 1,
      messages: [{ role: 'user', content: 'use lookup' }],
      tools: [],
      provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'test-model' },
      maxStepsPerTurn: null,
      cwd: '/tmp',
      shell: '/bin/sh',
    }),
    JSON.stringify({ mode: 'auto', rules: [], sessionApprovedPatterns: [] }),
    JSON.stringify(segments),
    'test-registry',
  );
  return session;
}

describe('TS external tool bridge', () => {
  test('a registered TS tool is executed by the engine', async () => {
    const session = makeSession([
      [
        {
          type: 'tool_call',
          toolCallId: 'lk',
          name: 'Lookup',
          argumentsPart: '{"query":"moon"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'found it' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    let called = 0;
    session.registerExternalTool(
      'Lookup',
      'Look up facts',
      JSON.stringify({ type: 'object', properties: { query: { type: 'string' } } }),
      (payloadJson: string) => {
        called += 1;
        const payload = JSON.parse(payloadJson) as { requestId: string; toolCallId?: string };
        session.completeToolCall(
          payload.requestId,
          JSON.stringify({
            // The LLM's streamed tool-call id must round-trip (P1-4): the
            // wire tool.result references it, not the ext-N slot key.
            toolCallId: payload.toolCallId ?? payload.requestId,
            toolName: 'Lookup',
            output: 'moon is 384400km away',
            isError: false,
            stopTurn: false,
            updates: [],
          }),
        );
      },
    );

    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });

    const batch = JSON.parse(await session.run()) as {
      events: Array<Record<string, unknown>>;
      progress: { status: string };
    };
    expect(batch.progress.status).toBe('completed');
    expect(batch.events).toEqual([]);
    expect(called).toBe(1);
    const result = events.find((event) => event['type'] === 'tool.result');
    expect(result?.['output']).toContain('384400');
    expect(result?.['isError']).toBe(false);
    // The tool.result references the LLM's tool_call_id, not ext-N.
    expect(result?.['toolCallId']).toBe('lk');
  }, 30_000);

  test('a missing external tool reports not found', async () => {
    const session = makeSession([
      [
        {
          type: 'tool_call',
          toolCallId: 'nope',
          name: 'NoSuchTool',
          argumentsPart: '{}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'recovered' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);

    const events: Array<Record<string, unknown>> = [];
    session.setOnEvent((eventJson: string) => {
      events.push(JSON.parse(eventJson) as Record<string, unknown>);
    });

    const batch = JSON.parse(await session.run()) as {
      events: Array<Record<string, unknown>>;
      progress: { status: string };
    };
    expect(batch.progress.status).toBe('completed');
    expect(batch.events).toEqual([]);
    const result = events.find((event) => event['type'] === 'tool.result');
    expect(result?.['isError']).toBe(true);
    expect(String(result?.['output'])).toContain('not found');
  }, 30_000);
});
