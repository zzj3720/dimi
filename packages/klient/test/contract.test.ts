/**
 * Scenario: runtime validation at Klient wire-contract boundaries.
 *
 * Exercises the session-creation and plugin-manifest schemas directly with no
 * external collaborators. Run with `pnpm --filter @moonshot-ai/klient exec
 * vitest run test/contract.test.ts`.
 */

import { describe, expect, it } from 'vitest';

import { pluginManifestSchema } from '../src/contract/global/plugins.js';
import { createSessionOptionsSchema } from '../src/contract/session/lifecycle.js';
import { agentTaskInfoSchema } from '../src/contract/agent/rpc.js';

type McpTimeoutField = 'startupTimeoutMs' | 'toolTimeoutMs';

const timeoutCases = [
  {
    surface: 'session creation',
    parse: (field: McpTimeoutField, value: number) =>
      createSessionOptionsSchema.safeParse({
        workDir: '/tmp/example',
        mcpServers: {
          example: { transport: 'stdio', command: 'node', [field]: value },
        },
      }),
  },
  {
    surface: 'plugin manifests',
    parse: (field: McpTimeoutField, value: number) =>
      pluginManifestSchema.safeParse({
        name: 'example',
        mcpServers: {
          example: { transport: 'stdio', command: 'node', [field]: value },
        },
      }),
  },
].flatMap(({ surface, parse }) => [
  { surface, field: 'startupTimeoutMs' as const, parse },
  { surface, field: 'toolTimeoutMs' as const, parse },
]);

describe('MCP timeout contract validation', () => {
  it.each(timeoutCases)('accepts the maximum $field for $surface', ({ field, parse }) => {
    expect(parse(field, 2_147_483_647).success).toBe(true);
  });

  it.each(timeoutCases)('rejects an above-maximum $field for $surface', ({ field, parse }) => {
    expect(parse(field, 2_147_483_648).success).toBe(false);
  });
});

describe('agent task contract validation', () => {
  it('accepts a durable generic tool task returned by agentTaskService.list', () => {
    expect(
      agentTaskInfoSchema.safeParse({
        taskId: 'tool-aaaaaaaa',
        kind: 'tool',
        description: 'Running SlowLookup',
        status: 'running',
        detached: true,
        startedAt: Date.now(),
        endedAt: null,
        turnId: 2,
        toolCallId: 'call_slow_lookup',
        toolName: 'SlowLookup',
        autoWaitTimeoutSeconds: 20,
      }).success,
    ).toBe(true);
  });
});
