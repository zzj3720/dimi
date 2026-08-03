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
import { TaskModel } from '#/agent/task/taskOps';
import { IWireService } from '#/wire/wire';
import { IEventBus } from '#/app/event/eventBus';
import {
  agentService,
  createTestAgent,
  permissionModeServices,
  type TestAgentContext,
} from '../../harness';

const RUST_ENGINE = 'DIMI_RUST_ENGINE';
const RUST_ENGINE_SCRIPTED = 'DIMI_RUST_ENGINE_SCRIPTED';

/**
 * Poll the agent context until `predicate` holds (the Rust runner launches
 * turns asynchronously — the RPC resolves at launch, TS `launched` parity —
 * so assertions on post-turn context must wait for the engine to finish).
 */
async function waitForContext(
  ctx: TestAgentContext,
  predicate: (messages: ReturnType<IAgentContextMemoryService['get']>) => boolean,
  what: string,
): Promise<void> {
  // 600 × 10ms: subagent/background flows settle after ~2.6s (spawn +
  // nested turn + notification), so a 2s window flakes.
  for (let i = 0; i < 600; i++) {
    if (predicate(ctx.get(IAgentContextMemoryService).get())) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timeout waiting for ${what}`);
}

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

    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.role === 'assistant'),
      'assistant message',
    );
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

    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.role === 'tool'),
      'tool message',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    // Exactly one tool message per call, carrying the REAL output (the fold
    // builds it from tool.result — no placeholder duplicates).
    const toolMessages = context.filter((message) => message.role === 'tool');
    expect(toolMessages).toHaveLength(1);
    const toolText = toolMessages[0]!.content
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('');
    expect(toolText).toContain('rust-tool-ok');
    expect(toolText).not.toContain('ran in the Rust engine');
    // The assistant message carries the tool call with full arguments.
    const assistant = context.find((message) => message.role === 'assistant');
    expect(assistant?.toolCalls.length).toBe(1);
    const call = assistant?.toolCalls[0];
    expect(call?.name).toBe('Bash');
    expect(JSON.parse(call?.arguments ?? '{}')).toEqual({ command: 'echo rust-tool-ok' });
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

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'user' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('please continue')) &&
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('steered reply')),
      'steer message + reply',
    );
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
    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.role === 'assistant'),
      'assistant message',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    const assistant = context.find((message) => message.role === 'assistant');
    expect(assistant).toBeDefined();
  });

  it('compacts the context mid-turn when the model window is crossed', async () => {    // Call 1 = the engine's compaction round (summary); call 2 = the step.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'text', delta: 'engine compaction summary' },
        { type: 'finish', finishReason: 'stop' },
      ],
      [
        { type: 'text', delta: 'answer after compaction' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    // A 2000-token window: 85% trigger = 1700 tokens.
    ctx.configure({ modelCapabilities: {
      image_in: false,
      video_in: false,
      audio_in: false,
      thinking: false,
      tool_use: true,
      max_context_tokens: 2000,
      dynamically_loaded_tools: false,
    } });
    ctx.get(IAgentLoopService);

    // Seed ~1830 tokens of assistant/tool exchanges (only the summary keeps).
    const blob = 'z'.repeat(300);
    const contextService = ctx.get(IAgentContextMemoryService);
    contextService.append({ role: 'user', content: [{ type: 'text', text: 'u2' }], toolCalls: [], origin: { kind: 'user' }, id: 'u2-id' });
    for (let i = 0; i < 20; i++) {
      contextService.append({ role: 'assistant', content: [{ type: 'text', text: `a${i}${'y'.repeat(60)}` }], toolCalls: [], origin: { kind: 'system_trigger', name: 'seed' }, id: `a${i}-id` });
      contextService.append({ role: 'tool', content: [{ type: 'text', text: blob }], toolCalls: [], origin: { kind: 'system_trigger', name: 'seed' }, id: `t${i}-id`, toolCallId: `c${i}` });
    }
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'continue' }] });

    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.origin?.kind === 'compaction_summary'),
      'compaction summary message',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    // The compaction summary message carries the prefix; tool blobs are gone.
    const summaryMessage = context.find((message) => message.origin?.kind === 'compaction_summary');
    expect(summaryMessage).toBeDefined();
    const allText = context
      .flatMap((message) => message.content)
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('\n');
    expect(allText).toContain('engine compaction summary');
    expect(allText).not.toContain(blob);
    // The post-compaction step answer is present.
    expect(allText).toContain('answer after compaction');
    // Recent user input survives the compaction.
    expect(allText).toContain('u2');
  });

  it('surfaces subagent task lifecycle events (spawned/completed + notification)', async () => {
    // Segment 0: the main turn asks the Agent tool to spawn a subagent
    // (stop_turn: true ends the main turn immediately). Segment 1: the
    // nested subagent turn blocks in Bash — the subagent settles ~2s later,
    // deterministically AFTER the main turn teardown, so the completion
    // notification takes the idle path (a notification turn). Segment 2: the
    // nested turn's final answer. The subagent settling must reach the TS
    // side as task wire ops + bus events + a task notification message.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sub',
          name: 'Agent',
          argumentsPart: '{"prompt":"sub work","description":"test sub"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        {
          type: 'tool_call',
          toolCallId: 'call_nested_sleep',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'subagent result text' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (
        type === 'subagent.spawned' ||
        type === 'subagent.completed' ||
        type === 'subagent.failed' ||
        type === 'task.notified' ||
        type === 'task.started' ||
        type === 'task.terminated'
      ) {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn a subagent' }] });
    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.origin?.kind === 'task' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('subagent result text')),
      'task completion notification message',
    );

    // Subagent ids are TS-lifecycle-shaped (agent-<n> / task-<8-uuid-chars>).
    const spawned = busEvents.find((event) => event['type'] === 'subagent.spawned');
    expect(spawned).toBeDefined();
    expect(String(spawned?.['subagentId'])).toMatch(/^agent-\d+$/);
    expect(spawned?.['parentToolCallId']).toBe('call_sub');
    expect(busEvents.some((event) => event['type'] === 'subagent.completed')).toBe(true);
    const completed = busEvents.find((event) => event['type'] === 'subagent.completed');
    expect(completed?.['resultSummary']).toBe('subagent result text');
    // The notification hook event fires with the task-completion shape.
    const notified = busEvents.find((event) => event['type'] === 'task.notified');
    expect(notified?.['notificationType']).toBe('task.completed');
    expect(notified?.['sourceKind']).toBe('background_task');
    // The wire records task.started + task.terminated (their bus events come
    // from the ops' toEvent).
    expect(busEvents.some((event) => event['type'] === 'task.started')).toBe(true);
    expect(busEvents.some((event) => event['type'] === 'task.terminated')).toBe(true);
    // The task model holds the settled subagent task (agent kind).
    const taskModel = ctx.get(IWireService).getModel(TaskModel);
    const tasks = [...taskModel.values()];
    const settled = tasks.find((task) => task.status === 'completed');
    expect(settled).toBeDefined();
    expect(settled).toMatchObject({ kind: 'agent', agentId: spawned?.['subagentId'] });
  }, 30_000);

  it('delivers a backgrounded bash task completion notification into the running turn', async () => {
    // The Bash call with timeout 1 moves `sleep 2; echo bg-done` to the
    // background (task.started). The turn continues; the next scripted call
    // blocks in Bash long enough for the background task to settle (~2s),
    // so the completion notification (task.settled) steers into the RUNNING
    // turn and is drained into the following LLM request (segment 2).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_bg',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2; echo bg-done","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        {
          type: 'tool_call',
          toolCallId: 'call_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 1.5"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'notification acknowledged' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (
        type === 'task.notified' ||
        type === 'task.started' ||
        type === 'task.terminated'
      ) {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run bg' }] });
    await waitForContext(
      ctx,
      (messages) =>
        messages.some(
          (message) =>
            message.role === 'assistant' &&
            message.content
              .filter((part) => part.type === 'text')
              .map((part) => (part as { text?: string }).text ?? '')
              .join('')
              .includes('notification acknowledged'),
        ),
      'notification-turn reply',
    );

    // The background task rode the standard task lifecycle records.
    const started = busEvents.find((event) => event['type'] === 'task.started');
    expect(started).toBeDefined();
    expect((started?.['info'] as { kind?: string } | undefined)?.kind).toBe('process');
    expect(
      (started?.['info'] as { taskId?: string } | undefined)?.taskId,
    ).toMatch(/^bash-[0-9a-f]{8}$/);
    expect(busEvents.some((event) => event['type'] === 'task.terminated')).toBe(true);
    // The completion notification reached the model: task.notified fired and
    // the task-origin notification message sits in the context (the
    // assistant reply 'notification acknowledged' consumed it).
    const notified = busEvents.find((event) => event['type'] === 'task.notified');
    expect(notified?.['notificationType']).toBe('task.completed');
    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.origin?.kind === 'task' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('Background process completed')),
      'bash completion notification message',
    );
    const taskModel = ctx.get(IWireService).getModel(TaskModel);
    const tasks = [...taskModel.values()];
    const settled = tasks.find((task) => task.status === 'completed');
    expect(settled).toBeDefined();
    expect(settled).toMatchObject({ kind: 'process' });
  }, 30_000);

  it('queues a prompt behind the running turn and runs it after', async () => {
    // Turn 1 blocks in Bash; turn 2 is queued and answers once turn 1 ends.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_queue_sleep',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 0.5"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'queued turn answer' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const first = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'first' }] });
    expect(first).toEqual({ turn_id: 0 });
    // Turn 1 is running (Bash sleep): a second prompt queues.
    await new Promise((resolve) => setTimeout(resolve, 150));
    const second = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'second' }] });
    expect(second).toBeUndefined();

    // Turn 1 finishes, then turn 2 runs and answers.
    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('queued turn answer')),
      'queued turn answer',
    );
  });

  it('cancels the running turn (turn.ended cancelled)', async () => {
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_cancel_sleep',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 5"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const ended = new Promise<{ reason?: string }>((resolve) => {
      const disposable = ctx.get(IEventBus).subscribe((event) => {
        if ((event as { type?: string }).type === 'turn.ended') {
          disposable.dispose();
          resolve(event as { reason?: string });
        }
      });
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'cancel me' }] });
    await new Promise((resolve) => setTimeout(resolve, 150)); // enter Bash sleep
    const runner = ctx.get(IRustEngineTurnRunner);
    expect(runner.cancel()).toBe(true);
    const turnEnded = await ended;
    expect(turnEnded.reason).toBe('cancelled');
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

    await waitForContext(ctx, (messages) => messages.some((m) => m.role === 'tool'), 'tool message');
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

    await waitForContext(ctx, (messages) => messages.some((m) => m.role === 'tool'), 'tool message');
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
