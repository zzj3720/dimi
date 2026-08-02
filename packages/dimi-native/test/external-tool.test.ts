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
    session.registerExternalTool('Lookup', (payloadJson: string) => {
      called += 1;
      const payload = JSON.parse(payloadJson) as { requestId: string };
      session.completeToolCall(
        payload.requestId,
        JSON.stringify({
          toolCallId: payload.requestId,
          toolName: 'Lookup',
          output: 'moon is 384400km away',
          isError: false,
          stopTurn: false,
          updates: [],
        }),
      );
    });

    const batch = JSON.parse(await session.run()) as {
      events: Array<Record<string, unknown>>;
      progress: { status: string };
    };
    expect(batch.progress.status).toBe('completed');
    expect(called).toBe(1);
    const result = batch.events.find((event) => event['type'] === 'tool.result');
    expect(result?.['output']).toContain('384400');
    expect(result?.['isError']).toBe(false);
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

    const batch = JSON.parse(await session.run()) as {
      events: Array<Record<string, unknown>>;
      progress: { status: string };
    };
    expect(batch.progress.status).toBe('completed');
    const result = batch.events.find((event) => event['type'] === 'tool.result');
    expect(result?.['isError']).toBe(true);
    expect(String(result?.['output'])).toContain('not found');
  }, 30_000);
});
