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
