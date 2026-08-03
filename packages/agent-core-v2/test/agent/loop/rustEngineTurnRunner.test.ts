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
import { IAgentScopeContext } from '#/agent/scopeContext/scopeContext';
import { IAgentTaskService } from '#/agent/task/task';
import { TaskModel } from '#/agent/task/taskOps';
import { IWireService } from '#/wire/wire';
import { IEventBus } from '#/app/event/eventBus';
import {
  agentService,
  configServices,
  createTestAgent,
  logServices,
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

  it('does not lose a steer landing at the turn boundary (starts a new turn)', async () => {
    // Turn 1 answers immediately. The engine sets its "finished" flag before
    // the completion events are emitted, so a steer observed after turn 1's
    // answer (in the window between the engine's final steer check and the
    // runner clearing activeSession) is refused and falls back to a new
    // turn — never silently dropped into a dead queue. Note: each turn gets
    // a fresh scripted client (segments restart), so the fallback turn
    // replays segment 1; what matters is that a second assistant reply
    // exists and the steer message is recorded exactly once.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'text', delta: 'first answer' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'first' }] });

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('first answer')),
      'first turn answer',
    );
    // The engine has already passed its final steer check: the runner's
    // steer refuses (no steerable turn) even if activeSession is still set.
    const runner = ctx.get(IRustEngineTurnRunner);
    expect(runner.steer({ input: [{ type: 'text', text: 'steer at boundary' }], origin: { kind: 'user' } })).toBe(false);

    // The rpcService fallback starts a new turn with the steer input (or
    // queues it behind the finishing turn) — the message is not dropped.
    const launched = await ctx.rpc.steer({ input: [{ type: 'text', text: 'steer at boundary' }] });
    expect(launched === undefined || launched.turn_id === 1).toBe(true);

    await waitForContext(
      ctx,
      (messages) =>
        messages.filter((message) => message.role === 'assistant').length >= 2,
      'second turn answer',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    // The steer message is recorded exactly once (the failed mid-turn steer
    // must not double-append alongside the fallback turn).
    const steerMessages = context.filter(
      (message) =>
        message.role === 'user' &&
        message.content
          .filter((part) => part.type === 'text')
          .map((part) => (part as { text?: string }).text ?? '')
          .join('') === 'steer at boundary',
    );
    expect(steerMessages).toHaveLength(1);
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
        type === 'subagent.started' ||
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

    // Subagent ids are TS-lifecycle-shaped (agent-<n> / agent-<8-uuid-chars>).
    const spawned = busEvents.find((event) => event['type'] === 'subagent.spawned');
    expect(spawned).toBeDefined();
    expect(String(spawned?.['subagentId'])).toMatch(/^agent-\d+$/);
    expect(spawned?.['parentToolCallId']).toBe('call_sub');
    // TS `emitAgentRunSpawned` parity: parent/caller agent + description are
    // filled (the engine has no profile concept, so subagentName carries the
    // description — documented deviation from TS's profileName).
    expect(spawned?.['parentAgentId']).toBe(ctx.get(IAgentScopeContext).agentId);
    expect(spawned?.['callerAgentId']).toBe(ctx.get(IAgentScopeContext).agentId);
    expect(spawned?.['description']).toBe('test sub');
    expect(spawned?.['subagentName']).toBe('test sub');
    expect(spawned?.['runInBackground']).toBe(false);
    // subagent.started is published right after spawned (mirrorAgentRun parity).
    const started = busEvents.find((event) => event['type'] === 'subagent.started');
    expect(started).toBeDefined();
    expect(started?.['subagentId']).toBe(spawned?.['subagentId']);
    expect(busEvents.some((event) => event['type'] === 'subagent.completed')).toBe(true);
    const completed = busEvents.find((event) => event['type'] === 'subagent.completed');
    expect(completed?.['resultSummary']).toBe('subagent result text');
    // The notification hook event fires with the task-completion shape.
    const notified = busEvents.find((event) => event['type'] === 'task.notified');
    expect(notified?.['notificationType']).toBe('task.completed');
    expect(notified?.['sourceKind']).toBe('background_task');
    // The wire records task.started + task.terminated (their bus events come
    // from the ops' toEvent).
    const taskStarted = busEvents.find((event) => event['type'] === 'task.started');
    expect(taskStarted).toBeDefined();
    expect((taskStarted?.['info'] as { taskId?: string } | undefined)?.taskId).toMatch(
      /^agent-[0-9a-f]{8}$/,
    );
    expect(busEvents.some((event) => event['type'] === 'task.terminated')).toBe(true);
    // The task model holds the settled subagent task (agent kind).
    const taskModel = ctx.get(IWireService).getModel(TaskModel);
    const tasks = [...taskModel.values()];
    const settled = tasks.find((task) => task.status === 'completed');
    expect(settled).toBeDefined();
    expect(settled).toMatchObject({ kind: 'agent', agentId: spawned?.['subagentId'] });
    // The engine task is registered with IAgentTaskService (TaskList /
    // TaskStop parity): the task-service entry carries the SAME wire task id
    // (`agent-<8>`, not a re-generated one), is marked detached, and is no
    // longer listed as active once settled.
    const serviceTasks = ctx.get(IAgentTaskService).list(false);
    const registered = serviceTasks.find((task) => task.taskId === settled?.taskId);
    expect(registered).toBeDefined();
    expect(registered).toMatchObject({ kind: 'agent', status: 'completed', detached: true });
    expect(ctx.get(IAgentTaskService).list(true).some((task) => task.taskId === settled?.taskId)).toBe(false);
    // The idle-path notification launches a notification turn — the XML lands
    // in the context EXACTLY ONCE (P1-1: the old code pre-appended AND let
    // runTurn append it again). No `turn.steer` op is recorded because the
    // main turn had already ended (P1-2: the op is only written when a steer
    // actually lands).
    const taskOriginMessages = ctx
      .get(IAgentContextMemoryService)
      .get()
      .filter((message) => message.origin?.kind === 'task');
    expect(taskOriginMessages).toHaveLength(1);
    // The notification preview mirrors the TS `renderOutputPreviewBlock`
    // wording/attrs (the engine output is the "currently buffered" tail).
    const notificationText = taskOriginMessages[0]!.content
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('');
    expect(notificationText).toContain(
      '<output-preview bytes="20" total_bytes="20" truncated="false">',
    );
    expect(notificationText).toContain(
      'No persisted full output is available; this preview is the currently buffered task output.',
    );
    const steerOps = ctx.snapshots.entries.filter(
      (entry) => entry.type === '[wire]' && entry.event === 'turn.steer',
    );
    expect(steerOps).toHaveLength(0);
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
    // The mid-turn steer path appends the notification message exactly once
    // (P1-1: no pre-append + steer duplicate; no runTurn re-append).
    const taskOriginMessages = ctx
      .get(IAgentContextMemoryService)
      .get()
      .filter((message) => message.origin?.kind === 'task');
    expect(taskOriginMessages).toHaveLength(1);
    const taskModel = ctx.get(IWireService).getModel(TaskModel);
    const tasks = [...taskModel.values()];
    const settled = tasks.find((task) => task.status === 'completed');
    expect(settled).toBeDefined();
    expect(settled).toMatchObject({ kind: 'process' });
  }, 30_000);

  it('TaskStop cancels a backgrounded engine task through the bridge', async () => {
    // `sleep 30` with timeout 1: the command is moved to the background
    // (task.started). TaskStop then cancels it through the registered
    // adapter's forceStop → session.cancelTask — the engine's poller kills
    // the process and settles "killed"; the terminal notification is
    // suppressed (TS TaskStop parity).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_bg_stop',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 30","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'bg launched' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'task.started' || type === 'task.terminated' || type === 'task.notified') {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run bg' }] });
    await waitForContext(
      ctx,
      () => busEvents.some((event) => event['type'] === 'task.started'),
      'bash task start',
    );
    const started = busEvents.find((event) => event['type'] === 'task.started');
    const taskId = (started?.['info'] as { taskId?: string } | undefined)?.taskId;
    expect(taskId).toMatch(/^bash-[0-9a-f]{8}$/);
    expect(started?.['info']).toMatchObject({ kind: 'process' });

    // TaskStopTool parity: the task is live in the service, then suppress the
    // terminal notification and stop it (the engine kills the process).
    const taskService = ctx.get(IAgentTaskService);
    expect(taskService.list(true).some((task) => task.taskId === taskId)).toBe(true);
    await taskService.suppressTerminalNotification(taskId!);
    const result = await taskService.stop(taskId!, 'Stopped by TaskStop');
    expect(result?.status).toBe('killed');

    // The engine settled the task as killed (poller killed the process) and
    // the wire record + the service entry both reflect it.
    await waitForContext(
      ctx,
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === taskId && task.status === 'killed',
        ),
      'bash task killed wire record',
    );
    expect(taskService.list(true).some((task) => task.taskId === taskId)).toBe(false);
    const registered = taskService.list(false).find((task) => task.taskId === taskId);
    expect(registered).toMatchObject({ kind: 'process', status: 'killed', detached: true });
    expect(busEvents.some((event) => event['type'] === 'task.notified')).toBe(false);
  }, 30_000);

  it('TaskStop reaches a task launched by a background subagent after the turn ended', async () => {
    // F1 (review P1): turn 1 launches subagent A (stop_turn ends the turn).
    // A's nested turn backgrounds a bash command (`sleep 30`, timeout 1)
    // whose `task.started` arrives AFTER turn 1 ended — the runner is idle,
    // so `activeSession` is undefined when the adapter is registered, and
    // the engine's per-task cancel signal lives on the OWNING session's task
    // map (`RustTurnSession` creates a fresh map per turn). TaskStop must
    // therefore cancel through the owning session recorded at
    // `task.started`, not through `activeSession`. The test stops B directly
    // through IAgentTaskService (the same entry point the TaskStop tool
    // uses; a real second model turn would replay the scripted segments from
    // cursor 0 and cascade — verified). A stays alive in a foreground Bash
    // so no A completion notification pollutes the assertions.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: main turn 1 — launch subagent A (stop_turn ends the turn).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sub_a',
          name: 'Agent',
          argumentsPart: '{"prompt":"spawn bg bash","description":"sub A"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: subagent A's nested turn — background a bash command (its
      // `task.started` fires ~1s later, after turn 1 already ended).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_nested_bg',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 30","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2: subagent A's nested turn — block in a foreground Bash so A
      // stays alive until the test disposes (no completion notification).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_nested_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 30"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (
        type === 'subagent.spawned' ||
        type === 'task.started' ||
        type === 'task.terminated' ||
        type === 'task.notified'
      ) {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn worker bash' }] });
    await waitForContext(
      ctx,
      () =>
        busEvents.some(
          (event) =>
            event['type'] === 'task.started' &&
            (event['info'] as { kind?: string } | undefined)?.kind === 'process',
        ),
      'worker-launched bash task start',
    );
    // Subagent A's own start (agent kind) fires first, mid-turn; the
    // worker-launched bash start (process kind) fires ~1s later, after turn
    // 1 ended — select the bash one.
    const started = busEvents.find(
      (event) =>
        event['type'] === 'task.started' &&
        (event['info'] as { kind?: string } | undefined)?.kind === 'process',
    );
    const taskId = (started?.['info'] as { taskId?: string } | undefined)?.taskId;
    expect(taskId).toMatch(/^bash-[0-9a-f]{8}$/);
    // The launching subagent (agent kind) also rode the lifecycle events.
    expect(busEvents.some((event) => event['type'] === 'subagent.spawned')).toBe(true);

    // TaskStopTool parity: suppress the terminal notification, then stop the
    // worker-launched task. The adapter's forceStop must reach the OWNING
    // session's task map (recorded at task.started), not the idle runner.
    const taskService = ctx.get(IAgentTaskService);
    expect(taskService.list(true).some((task) => task.taskId === taskId)).toBe(true);
    await taskService.suppressTerminalNotification(taskId!);
    const result = await taskService.stop(taskId!, 'Stopped by TaskStop');
    expect(result?.status).toBe('killed');

    // The engine settled the worker-launched task as killed (its poller
    // killed the process) — the wire record reflects it, proving the cancel
    // reached the owning session.
    await waitForContext(
      ctx,
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === taskId && task.status === 'killed',
        ),
      'worker-launched bash task killed wire record',
    );
    expect(taskService.list(true).some((task) => task.taskId === taskId)).toBe(false);
    const registered = taskService.list(false).find((task) => task.taskId === taskId);
    expect(registered).toMatchObject({ kind: 'process', status: 'killed', detached: true });
    // No completion notification for the stopped task (suppressed), and A is
    // still blocked — nothing fired.
    expect(busEvents.some((event) => event['type'] === 'task.notified')).toBe(false);
  }, 30_000);

  it('streams live output into TaskOutput while a backgrounded bash runs', async () => {
    // F2 (review nit): the engine emits `task.output` deltas as the poller
    // drains a backgrounded command (`mid` at ~2s, `done` at ~4s), and the
    // runner forwards them to the adapter's sink — so TaskOutput shows
    // partial output BEFORE the settle (TS ProcessTask parity), not only at
    // settle. The main turn stays alive (seg 1 blocks ~3.5s) so the
    // completion notification steers into the running turn and no
    // notification turn (which would replay segments) is created.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: background a command that produces output while running.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_bg_stream',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2; echo mid; sleep 2; echo done","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: block the turn past the settle (~4s) so the notification
      // steers into the running turn.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 3.5"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2: final answer.
      [{ type: 'text', delta: 'stream turn done' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'task.started' || type === 'task.terminated') {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run bg stream' }] });
    await waitForContext(
      ctx,
      () => busEvents.some((event) => event['type'] === 'task.started'),
      'bash task start',
    );
    const started = busEvents.find((event) => event['type'] === 'task.started');
    const taskId = (started?.['info'] as { taskId?: string } | undefined)?.taskId;
    expect(taskId).toMatch(/^bash-[0-9a-f]{8}$/);

    // The engine streams `mid` at ~2s; the task settles only at ~4s (after
    // `done`), so `mid` MUST be readable while the task is still active.
    const taskService = ctx.get(IAgentTaskService);
    let sawMidWhileRunning = false;
    for (let i = 0; i < 600; i++) {
      const output = await taskService.readOutput(taskId!);
      if (output.includes('mid')) {
        sawMidWhileRunning = taskService
          .list(true)
          .some((task) => task.taskId === taskId);
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    expect(sawMidWhileRunning).toBe(true);

    // After the settle the full output is present (the settle appended only
    // the not-yet-streamed tail — no duplication).
    await waitForContext(
      ctx,
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === taskId && task.status === 'completed',
        ),
      'bash task completed wire record',
    );
    const output = await taskService.readOutput(taskId!);
    expect(output).toContain('mid');
    expect(output).toContain('done');
  }, 30_000);

  it('keeps TaskOutput consistent when a backgrounded bash printed before the timeout', async () => {
    // F1 (review P1): a command that prints BEFORE the timeout is
    // backgrounded with its foreground output emitted as the FIRST
    // `task.output` delta, so the streamed deltas are a true prefix of the
    // settle output and the adapter's settle-tail arithmetic stays correct.
    // Without the prefix the retained buffer dropped `start` and duplicated
    // the post-timeout tail (`middle\niddle\n`).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: background a command that prints before AND after the
      // timeout (`start` at ~0s, timeout at 1s, `middle` at ~2s, exit ~3s).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_bg_fg',
          name: 'Bash',
          argumentsPart: '{"command":"echo start; sleep 2; echo middle; sleep 1","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: block the turn past the settle (~3s) so the notification
      // steers into the running turn.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 3.5"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2: final answer.
      [{ type: 'text', delta: 'fg turn done' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'task.started' || type === 'task.terminated') {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run fg bg' }] });
    await waitForContext(
      ctx,
      () => busEvents.some((event) => event['type'] === 'task.started'),
      'bash task start',
    );
    const started = busEvents.find((event) => event['type'] === 'task.started');
    const taskId = (started?.['info'] as { taskId?: string } | undefined)?.taskId;
    expect(taskId).toMatch(/^bash-[0-9a-f]{8}$/);
    const taskService = ctx.get(IAgentTaskService);

    // After the settle the retained buffer must hold the FULL output
    // (`start` from the foreground prefix, `middle` from the post-timeout
    // stream) — exactly once each, no duplicated tail.
    await waitForContext(
      ctx,
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === taskId && task.status === 'completed',
        ),
      'bash task completed wire record',
    );
    const output = await taskService.readOutput(taskId!);
    expect(output).toBe('start\nmiddle\n');
  }, 30_000);

  it('honors the configured killGracePeriodMs for engine TaskStop', async () => {
    // F3 (review nit): the runner wires the `task` config section's
    // `killGracePeriodMs` into the engine turn input, and the engine's bash
    // poller waits that long between SIGTERM and SIGKILL. With 300ms
    // configured, a TaskStop on a SIGTERM-ignoring backgrounded command
    // force-kills and settles the wire record well under the 5s default
    // (a wiring failure would leave the engine waiting ~5s before SIGKILL).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_bg_grace',
          name: 'Bash',
          argumentsPart: '{"command":"trap \'\' TERM; sleep 30","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        {
          type: 'tool_call',
          toolCallId: 'call_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 0.2"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'grace turn done' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([
      permissionModeServices('auto'),
      configServices(() => ({ providers: {}, task: { killGracePeriodMs: 300 } })),
    ]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'task.started' || type === 'task.terminated') {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run grace' }] });
    await waitForContext(
      ctx,
      () => busEvents.some((event) => event['type'] === 'task.started'),
      'bash task start',
    );
    const started = busEvents.find((event) => event['type'] === 'task.started');
    const taskId = (started?.['info'] as { taskId?: string } | undefined)?.taskId;
    expect(taskId).toMatch(/^bash-[0-9a-f]{8}$/);

    const taskService = ctx.get(IAgentTaskService);
    await taskService.suppressTerminalNotification(taskId!);
    const result = await taskService.stop(taskId!, 'Stopped by TaskStop');
    expect(result?.status).toBe('killed');

    // The ENGINE settle (which dispatches the wire record) lands only after
    // the engine's own grace + SIGKILL + drain — ~0.6s at 300ms, ~5.25s at
    // the hardcoded 5s default. A 4s budget distinguishes the two.
    let wireKilled = false;
    for (let i = 0; i < 400; i++) {
      if (
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === taskId && task.status === 'killed',
        )
      ) {
        wireKilled = true;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    expect(wireKilled).toBe(true);
    expect(taskService.list(true).some((task) => task.taskId === taskId)).toBe(false);
  }, 30_000);

  it('delivers the TS-parity recovery line for a failed subagent notification', async () => {
    // Segment 0: the main turn asks the Agent tool to spawn a subagent
    // (stop_turn ends the main turn immediately). Segment 1: the nested
    // turn is filtered by the (scripted) provider → the subagent fails and
    // the notification body carries the TS `buildAgentTaskNotificationBody`
    // recovery block, including the `run_in_background` guidance line.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sub_fail',
          name: 'Agent',
          argumentsPart: '{"prompt":"sub work","description":"failing sub"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'finish', finishReason: 'filtered' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'subagent.failed' || type === 'task.notified') {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn failing sub' }] });
    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.origin?.kind === 'task' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('Add run_in_background=true to keep it backgrounded')),
      'failed-subagent notification with recovery line',
    );
    expect(busEvents.some((event) => event['type'] === 'subagent.failed')).toBe(true);
    const failed = busEvents.find((event) => event['type'] === 'subagent.failed');
    expect(String(failed?.['subagentId'])).toMatch(/^agent-\d+$/);
    // TS `buildAgentTaskNotificationBody` parity: the failed-subagent body
    // reports the reason and appends the recovery block (resume by agent_id,
    // keep backgrounded with run_in_background).
    const notificationText = ctx
      .get(IAgentContextMemoryService)
      .get()
      .filter((message) => message.origin?.kind === 'task')
      .map((message) =>
        message.content
          .filter((part) => part.type === 'text')
          .map((part) => (part as { text?: string }).text ?? '')
          .join(''),
      )
      .join('\n');
    expect(notificationText).toContain('failing sub failed.');
    expect(notificationText).toContain(
      'To recover or continue this subagent, call Agent(resume=',
    );
    expect(notificationText).toContain('Use agent_id');
    expect(notificationText).toContain(
      'Add run_in_background=true to keep it backgrounded, or omit it to take the result inline in the current turn.',
    );
  }, 30_000);

  it('cancels background work and drops late settles when the agent is disposed', async () => {
    // The main turn launches a background subagent whose nested turn blocks in
    // Bash (`sleep 2`). Disposing the agent while the subagent is still
    // running must not throw or log errors (P1-5): the runner closes every
    // Rust session — the EventSink stops forwarding and the worker observes
    // `is_closed` and skips the settle — so no `task.terminated` op /
    // notification fires into the disposed runner.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sub',
          name: 'Agent',
          argumentsPart: '{"prompt":"sub work"}',
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
    ]);
    const errors: Array<{ message: string; payload?: unknown }> = [];
    const logger = {
      error: (message: string, payload?: unknown) => {
        errors.push({ message, payload });
      },
      warn: () => undefined,
      info: () => undefined,
      debug: () => undefined,
      child: () => logger,
    };
    ctx = createTestAgent([permissionModeServices('auto')], logServices(logger));
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (
        type === 'subagent.spawned' ||
        type === 'task.started' ||
        type === 'task.terminated' ||
        type === 'task.notified'
      ) {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn bg' }] });
    // Wait until the subagent is actually running (its nested Bash sleeps),
    // then tear the agent down mid-flight.
    await waitForContext(
      ctx,
      () => busEvents.some((event) => event['type'] === 'subagent.spawned'),
      'subagent spawn',
    );
    await ctx.dispose();

    // Give the nested turn (sleep 2) time to settle into the closed session.
    await new Promise((resolve) => setTimeout(resolve, 2_500));
    expect(errors).toEqual([]);
    // No terminal task records / notification reached the disposed runner.
    expect(busEvents.some((event) => event['type'] === 'task.terminated')).toBe(false);
    expect(busEvents.some((event) => event['type'] === 'task.notified')).toBe(false);
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
