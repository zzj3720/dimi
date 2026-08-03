/**
 * M3 slice-1 swap-in test: `DIMI_RUST_ENGINE=1` routes the turn through the
 * Rust engine (`rustEngineTurnRunner`). With `DIMI_RUST_ENGINE_SCRIPTED`
 * injecting scripted LLM segments, the full wiring is exercised: user
 * message → Rust engine → events on the bus → context records (step/tool/
 * assistant message) — the same surfaces the TS loop drives.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { IAgentContextMemoryService } from '#/agent/contextMemory/contextMemory';
import { IAgentLoopService } from '#/agent/loop/loop';
import { IRustEngineTurnRunner } from '#/agent/loop/rustEngineTurnRunner';
import {
  agentService,
  createTestAgent,
  permissionModeServices,
  type TestAgentContext,
} from '../../harness';

const RUST_ENGINE = 'DIMI_RUST_ENGINE';
const RUST_ENGINE_SCRIPTED = 'DIMI_RUST_ENGINE_SCRIPTED';

describe('Rust engine turn runner (DIMI_RUST_ENGINE=1)', () => {
  let ctx: TestAgentContext;

  beforeEach(() => {
    process.env[RUST_ENGINE] = '1';
  });

  afterEach(async () => {
    delete process.env[RUST_ENGINE];
    delete process.env[RUST_ENGINE_SCRIPTED];
    try {
      await ctx.dispose();
    } catch {
      // dispose may already have run
    }
  });

  it('routes a text turn through the Rust engine and mirrors it into the context', async () => {
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'thinking', delta: '<rust-think>' },
        { type: 'text', delta: '<rust-answer>' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService); // agent scope must be live
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'Hello Rust' }] });

    const context = ctx.get(IAgentContextMemoryService).get();
    const userMessage = context.find((message) => message.role === 'user');
    expect(userMessage).toBeDefined();
    const assistantMessage = context.find((message) => message.role === 'assistant');
    expect(assistantMessage).toBeDefined();
    const parts = assistantMessage?.content ?? [];
    const textParts = parts.filter((part) => part.type === 'text');
    expect(textParts.map((part) => (part as { text?: string }).text)).toContain('<rust-answer>');
    const thinkParts = parts.filter((part) => part.type === 'think');
    expect(thinkParts.map((part) => (part as { think?: string }).think)).toContain('<rust-think>');
  });

  it('mirrors tool execution into the context', async () => {
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_rust',
          name: 'Bash',
          argumentsPart: '{"command":"echo rust-tool-ok"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'tool done' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run tool' }] });

    const context = ctx.get(IAgentContextMemoryService).get();
    const toolMessage = context.find((message) => message.role === 'tool');
    expect(toolMessage).toBeDefined();
    const toolText = toolMessage?.content
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('');
    expect(toolText).toContain('rust-tool-ok');
    const assistant = context.find((message) => message.role === 'assistant');
    expect(assistant?.toolCalls.length).toBeGreaterThan(0);
  });

  it('steers a running turn into the engine (mid-turn injection)', async () => {
    // Step 1 blocks in Bash so the steer has a deterministic window to land;
    // step 2 is the reply the engine produces after draining the steer.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sleep',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 0.5"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'steered reply' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const promptPromise = ctx.rpc.prompt({ input: [{ type: 'text', text: 'start' }] });
    // Let the turn start and enter the Bash sleep, then steer mid-turn.
    await new Promise((resolve) => setTimeout(resolve, 150));
    const runner = ctx.get(IRustEngineTurnRunner);
    expect(runner.steer({ input: [{ type: 'text', text: 'please continue' }], origin: { kind: 'user' } })).toBe(true);
    await promptPromise;

    const context = ctx.get(IAgentContextMemoryService).get();
    const steerMessage = context.find((message) => message.role === 'user' && message.content
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('') === 'please continue');
    expect(steerMessage).toBeDefined();
    const assistantMessages = context.filter((message) => message.role === 'assistant');
    const lastText = assistantMessages
      .flatMap((message) => message.content)
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('');
    expect(lastText).toContain('steered reply');
    // The steered message precedes the reply that consumed it.
    const steerIndex = context.indexOf(steerMessage!);
    const replyIndex = context.findIndex((message) =>
      message.role === 'assistant' &&
      message.content
        .filter((part) => part.type === 'text')
        .map((part) => (part as { text?: string }).text ?? '')
        .join('')
        .includes('steered reply'),
    );
    expect(steerIndex).toBeGreaterThanOrEqual(0);
    expect(replyIndex).toBeGreaterThan(steerIndex);
  });

  it('falls back to a new turn when steering an idle agent', async () => {
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'text', delta: 'idle steer turn' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);
    // No active turn: steer routes to a fresh Rust turn.
    const result = await ctx.rpc.steer({ input: [{ type: 'text', text: 'hello idle' }] });
    expect(result).toEqual({ turn_id: 0 });
    const context = ctx.get(IAgentContextMemoryService).get();
    const assistant = context.find((message) => message.role === 'assistant');
    expect(assistant).toBeDefined();
  });
});

describe('Rust engine approval flow (manual mode)', () => {
  let ctx: TestAgentContext;

  beforeEach(() => {
    process.env[RUST_ENGINE] = '1';
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_ask',
          name: 'Bash',
          argumentsPart: '{"command":"echo ask-ok"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'approved run done' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
  });

  afterEach(async () => {
    delete process.env[RUST_ENGINE];
    delete process.env[RUST_ENGINE_SCRIPTED];
    try {
      await ctx.dispose();
    } catch {
      // already disposed
    }
  });

  it('pauses for approval and continues on approval', async () => {
    ctx = createTestAgent(); // manual mode by default
    ctx.get(IAgentLoopService);

    const approvalPromise = ctx.takeApprovalRequest();
    const promptPromise = ctx.rpc.prompt({ input: [{ type: 'text', text: 'run approved' }] });

    const approval = await approvalPromise;
    approval.respond({ decision: 'approved' });
    await promptPromise;

    const context = ctx.get(IAgentContextMemoryService).get();
    const toolMessage = context.find((message) => message.role === 'tool');
    expect(toolMessage).toBeDefined();
    const toolText = toolMessage?.content
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('');
    expect(toolText).toContain('ask-ok');
  });

  it('denies the tool when the approval is rejected', async () => {
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);

    const approvalPromise = ctx.takeApprovalRequest();
    const promptPromise = ctx.rpc.prompt({ input: [{ type: 'text', text: 'run rejected' }] });

    const approval = await approvalPromise;
    approval.respond({ decision: 'rejected', feedback: 'no thanks' });
    await promptPromise;

    const context = ctx.get(IAgentContextMemoryService).get();
    const toolMessage = context.find((message) => message.role === 'tool');
    expect(toolMessage).toBeDefined();
    const toolText = toolMessage?.content
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('');
    expect(toolText).toContain('was not run because the user rejected the approval request');
    expect(toolText).toContain('no thanks');
  });
});
