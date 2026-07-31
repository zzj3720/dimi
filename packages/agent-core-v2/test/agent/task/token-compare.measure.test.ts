/**
 * One-shot token comparison harness (not a CI assertion suite).
 * Measures estimateTokens of every LLM generate call for slow/fast tool scenarios.
 *
 * Run:
 *   pnpm --filter @moonshot-ai/agent-core-v2 exec vitest run test/agent/task/token-compare.measure.test.ts
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { IAgentProfileService } from '#/agent/profile/profile';
import { IAgentContextMemoryService } from '#/agent/contextMemory/contextMemory';
import { IAgentTaskService } from '#/agent/task/task';
import { IAgentToolExecutorService } from '#/agent/toolExecutor/toolExecutor';
import { IAgentToolRegistryService } from '#/agent/toolRegistry/toolRegistry';
import { IAgentWaitService } from '#/agent/wait/wait';
import {
  estimateTokens,
  estimateTokensForMessages,
  estimateTokensForTools,
} from '#/kosong/contract/tokens';
import type { ExecutableTool, ExecutableToolResult, ToolExecution } from '#/tool/toolContract';

import { createTestAgent, type TestAgentContext } from '../../harness';

const LABEL = process.env['MEASURE_LABEL'] ?? 'worktree';

class ControllableTool implements ExecutableTool<{ readonly query: string }> {
  readonly name: string;
  readonly description = 'Resolve a lookup after an external completion.';
  readonly parameters = {
    type: 'object',
    properties: { query: { type: 'string' } },
    required: ['query'],
    additionalProperties: false,
  } as const;

  readonly started: Promise<void>;
  private resolveStarted!: () => void;
  private resolveResult!: (result: ExecutableToolResult) => void;
  private readonly result = new Promise<ExecutableToolResult>((resolve) => {
    this.resolveResult = resolve;
  });

  constructor(name = 'SlowLookup') {
    this.name = name;
    this.started = new Promise((resolve) => {
      this.resolveStarted = resolve;
    });
  }

  resolveExecution(): ToolExecution {
    return {
      approvalRule: this.name,
      accesses: [],
      execute: async ({ signal }) => {
        this.resolveStarted();
        return Promise.race([
          this.result,
          new Promise<never>((_resolve, reject) => {
            signal.addEventListener(
              'abort',
              () => {
                reject(signal.reason);
              },
              { once: true },
            );
          }),
        ]);
      },
    };
  }

  complete(output: string): void {
    this.resolveResult({ output });
  }
}

function costOf(call: {
  systemPrompt?: string | undefined;
  tools: readonly { name: string; description: string; parameters: unknown }[];
  history: readonly unknown[];
}) {
  const system = estimateTokens(call.systemPrompt ?? '');
  const tools = estimateTokensForTools(call.tools as never);
  const history = estimateTokensForMessages(call.history as never);
  return {
    system,
    tools,
    history,
    total: system + tools + history,
    historyMessages: call.history.length,
  };
}

function summarizeCalls(calls: readonly ReturnType<typeof costOf>[]) {
  return {
    llmCalls: calls.length,
    perCall: calls,
    sumTotal: calls.reduce((a, c) => a + c.total, 0),
    sumHistory: calls.reduce((a, c) => a + c.history, 0),
    lastHistory: calls.at(-1)?.history ?? 0,
    lastTotal: calls.at(-1)?.total ?? 0,
  };
}

describe('token compare measure', () => {
  let ctx: TestAgentContext;

  beforeEach(async () => {
    ctx = createTestAgent();
    ctx.get(IAgentToolExecutorService);
    ctx.get(IAgentTaskService);
    ctx.get(IAgentWaitService);
    await ctx.rpc.setPermission({ mode: 'yolo' });
  });

  afterEach(async () => {
    await ctx.dispose();
  });

  it(
    'slow tool detaches then completes (async path)',
    async () => {
      const tool = new ControllableTool();
      const registration = ctx.get(IAgentToolRegistryService).register(tool);
      ctx.get(IAgentProfileService).update({ activeToolNames: [tool.name] });
      ctx.mockNextResponse({
        type: 'function',
        id: 'call_slow_lookup',
        name: tool.name,
        arguments: JSON.stringify({ query: 'moon' }),
      });
      ctx.mockNextResponse({ type: 'text', text: 'Observed the completed task notification.' });

      const payload = 'x'.repeat(2000);
      try {
        // Match async-tools e2e: prompt, wait for tool start, then untilTurnEnd
        // (executor detaches after the 3s foreground budget without needing a
        // manual sleep — sleeping first can miss the turn.ended edge).
        await ctx.rpc.prompt({
          input: [{ type: 'text', text: 'Look up moon asynchronously' }],
        });
        await tool.started;
        await ctx.untilTurnEnd();

        const tasks = ctx.get(IAgentTaskService).list(false);
        const task = tasks.find((t) => t.kind === 'tool');

        const afterDetach = summarizeCalls(ctx.llmCalls.map(costOf));
        const contextAfterDetach = estimateTokensForMessages(
          ctx.get(IAgentContextMemoryService).get() as never,
        );

        const completionTurn = ctx.untilTurnEnd();
        tool.complete(payload);
        await completionTurn;

        const afterComplete = summarizeCalls(ctx.llmCalls.map(costOf));
        const contextAfterComplete = estimateTokensForMessages(
          ctx.get(IAgentContextMemoryService).get() as never,
        );

        // Approximate notification size from second-call history delta.
        const notificationApprox =
          (afterComplete.perCall[1]?.history ?? 0) - (afterComplete.perCall[0]?.history ?? 0);

        expect(afterComplete.llmCalls).toBeGreaterThan(afterDetach.llmCalls);

        console.log(
          JSON.stringify(
            {
              measure: 'slow-async',
              label: LABEL,
              payloadBytes: payload.length,
              task: task
                ? {
                    kind: task.kind,
                    status: task.status,
                    detached: 'detached' in task ? task.detached : undefined,
                  }
                : null,
              afterDetach,
              afterComplete,
              contextTokensAfterDetach: contextAfterDetach,
              contextTokensAfterComplete: contextAfterComplete,
              historyGrowthCall1MinusCall0: notificationApprox,
              note: 'estimateTokens heuristic (ASCII≈4 chars/token), not provider billing',
            },
            null,
            2,
          ),
        );
      } finally {
        registration.dispose();
      }
    },
    20_000,
  );

  it('fast tool completes under foreground budget (inline path)', async () => {
    const tool = new ControllableTool('FastLookup');
    const registration = ctx.get(IAgentToolRegistryService).register(tool);
    ctx.get(IAgentProfileService).update({ activeToolNames: [tool.name] });
    ctx.mockNextResponse({
      type: 'function',
      id: 'call_fast_lookup',
      name: tool.name,
      arguments: JSON.stringify({ query: 'moon' }),
    });
    ctx.mockNextResponse({ type: 'text', text: 'Got the tool result inline.' });

    const payload = 'y'.repeat(2000);
    try {
      const promptP = ctx.rpc.prompt({
        input: [{ type: 'text', text: 'Look up moon quickly' }],
      });
      await tool.started;
      tool.complete(payload);
      await promptP;
      await ctx.untilTurnEnd();

      const summary = summarizeCalls(ctx.llmCalls.map(costOf));
      const contextTokens = estimateTokensForMessages(
        ctx.get(IAgentContextMemoryService).get() as never,
      );
      const tasks = ctx.get(IAgentTaskService).list(false);
      const task = tasks.find((t) => t.kind === 'tool');

      expect(summary.llmCalls).toBeGreaterThanOrEqual(2);

      console.log(
        JSON.stringify(
          {
            measure: 'fast-inline',
            label: LABEL,
            payloadBytes: payload.length,
            task: task
              ? {
                  kind: task.kind,
                  status: task.status,
                  detached: 'detached' in task ? task.detached : undefined,
                }
              : null,
            summary,
            contextTokens,
            note: 'estimateTokens heuristic (ASCII≈4 chars/token), not provider billing',
          },
          null,
          2,
        ),
      );
    } finally {
      registration.dispose();
    }
  }, 15_000);
});
