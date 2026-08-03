/**
 * Coverage for Rust-engine behaviors that are implemented but previously had
 * no runner-side test backing (audit-driven). Like `rustEngineTurnRunner.test.ts`,
 * these suites drive the REAL `RustTurnSession` (napi binding) with
 * `DIMI_RUST_ENGINE_SCRIPTED` scripted LLM segments, so the full wiring —
 * engine events → bus → context records → wire ops — is exercised.
 *
 * This file exists separately because `rustEngineTurnRunner.test.ts` is owned
 * by a parallel change; do not add tests for these behaviors there.
 *
 * All seven behaviors below were verified implemented (no skips):
 *   1. queued-turn cancellation (runner.cancel(turnId) marks the queued entry
 *      cancelled and the queue skips it),
 *   2. TaskStop sibling isolation (per-task CancelSignal),
 *   3. concurrent background-task completion notifications (per-settle
 *      delivery, nothing lost),
 *   4. `tool.call.delta` protocol events (engine emits one delta per streamed
 *      arguments fragment),
 *   5. compaction gated by `max_input_tokens` (the runner's compaction
 *      window is `max_input_tokens ?? max_context_tokens`),
 *   6. nested subagent turns can use Bash (shared tool registry) and the
 *      outcome mirrors into the main context,
 *   7. provider-filtered main turn ends `failed` with the PROVIDER_FILTERED
 *      error code.
 *
 * NOTE on tests 2/3: the parent brief suggested two Agent subagent calls for
 * concurrent background work, but the engine's Agent tool hardcodes
 * `stop_turn: true` and the engine loop breaks on the FIRST stop_turn tool
 * result — two Agent calls from one turn is impossible — and a subagent that
 * settles while the runner is idle launches a notification turn that REPLAYS
 * the scripted segments from cursor 0 (each `RustTurnSession` owns a fresh
 * scripted client), which cascades. The engine's actual backgroundable path is
 * Bash-with-timeout (`run_in_background` is rejected in tool.rs): the command
 * is moved to the background when it exceeds `timeout`. Two backgrounded bash
 * commands exercise the same per-task `CancelSignal` (`AgentTasks`) and the
 * same per-settle notification delivery (`deliverTaskNotification`) the brief
 * targeted, deterministically.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { IAgentContextMemoryService } from '#/agent/contextMemory/contextMemory';
import { IAgentLoopService } from '#/agent/loop/loop';
import { IRustEngineTurnRunner } from '#/agent/loop/rustEngineTurnRunner';
import { IAgentProfileService } from '#/agent/profile/profile';
import { IAgentTaskService } from '#/agent/task/task';
import { TaskModel } from '#/agent/task/taskOps';
import { IWireService } from '#/wire/wire';
import { IEventBus } from '#/app/event/eventBus';
import {
  createTestAgent,
  permissionModeServices,
  type TestAgentContext,
} from '../../harness';

const DIMI_LEGACY = 'DIMI_LEGACY';
const RUST_ENGINE_SCRIPTED = 'DIMI_RUST_ENGINE_SCRIPTED';

/**
 * Poll the agent context until `predicate` holds (the Rust runner launches
 * turns asynchronously — the RPC resolves at launch — so post-turn context
 * assertions must wait for the engine to finish).
 */
async function waitForContext(
  ctx: TestAgentContext,
  predicate: (messages: ReturnType<IAgentContextMemoryService['get']>) => boolean,
  what: string,
): Promise<void> {
  for (let i = 0; i < 600; i++) {
    if (predicate(ctx.get(IAgentContextMemoryService).get())) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timeout waiting for ${what}`);
}

/** Poll an arbitrary predicate (bus events / wire model). */
async function waitFor(predicate: () => boolean, what: string, timeoutMs = 6_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timeout waiting for ${what}`);
}

/** Join the text parts of a message (the TS fold keeps content parts). */
function messageText(message: {
  readonly content: ReadonlyArray<{ readonly type: string; readonly text?: string }>;
}): string {
  return message.content
    .filter((part) => part.type === 'text')
    .map((part) => part.text ?? '')
    .join('');
}

describe('Rust engine implemented-but-untested behaviors', () => {
  let ctx: TestAgentContext;

  beforeEach(() => {
    delete process.env[DIMI_LEGACY];
  });

  afterEach(async () => {
    delete process.env[DIMI_LEGACY];
    delete process.env[RUST_ENGINE_SCRIPTED];
    try {
      await ctx.dispose();
    } catch {
      // dispose may already have run
    }
  });

  it('cancels a queued turn by id (never starts; the running turn completes)', async () => {
    // Turn 0 blocks in Bash (`sleep 1`); turn 1 queues behind it.
    // `runner.cancel(1)` marks the queued entry cancelled and dispatches the
    // `turn.cancel` op; when turn 0 finishes, the queue skips the cancelled
    // entry, so turn 1 never emits `turn.started` / never runs.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_q',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 1"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const started: Array<{ turnId: number }> = [];
    const ended: Array<{ turnId: number }> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'turn.started') started.push(event as { turnId: number });
      if (type === 'turn.ended') ended.push(event as { turnId: number });
    });

    const first = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'first' }] });
    expect(first).toEqual({ turn_id: 0 });
    // Let turn 0 enter its Bash sleep, then queue turn 1.
    await new Promise((resolve) => setTimeout(resolve, 150));
    const second = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'second' }] });
    expect(second).toBeUndefined();

    // Cancel the QUEUED turn by id (the active turn is 0, so this must not
    // touch the session).
    const runner = ctx.get(IRustEngineTurnRunner);
    expect(runner.cancel(1)).toBe(true);

    // The queued-cancel dispatches the `turn.cancel` wire op with the id
    // (the op is a persisted marker — `cancelTurn`'s model apply is a no-op
    // without a `target`, which is the runner's shape).
    await waitFor(
      () =>
        ctx.snapshots.entries.some(
          (entry) =>
            entry.type === '[wire]' &&
            entry.event === 'turn.cancel' &&
            (entry.args as { turnId?: number } | undefined)?.turnId === 1,
        ),
      'turn.cancel wire op for turn 1',
    );

    // Turn 0 finishes (~1s Bash), then the queue drains.
    await waitFor(() => ended.some((event) => event.turnId === 0), 'turn 0 ended');
    // A would-be turn 1 would launch immediately after; give it a window.
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(started.some((event) => event.turnId === 1)).toBe(false);
    expect(ended.some((event) => event.turnId === 1)).toBe(false);
    // Turn 1 never contributed loop events (no step.begin for turn '1').
    const turn1LoopEvents = ctx.snapshots.entries.filter(
      (entry) =>
        entry.type === '[wire]' &&
        entry.event === 'context.append_loop_event' &&
        (entry.args as { event?: { turnId?: string } } | undefined)?.event?.turnId === '1',
    );
    expect(turn1LoopEvents).toHaveLength(0);
    // The only assistant message is turn 0's tool-call message (Bash sleep):
    // no assistant TEXT was streamed and turn 1 never ran.
    const assistantMessages = ctx.get(IAgentContextMemoryService).get().filter(
      (message) => message.role === 'assistant',
    );
    expect(assistantMessages).toHaveLength(1);
    expect(messageText(assistantMessages[0]!)).toBe('');
    // Exactly one turn.cancel op was dispatched for turn 1.
    const cancelOps = ctx.snapshots.entries.filter(
      (entry) => entry.type === '[wire]' && entry.event === 'turn.cancel',
    );
    expect(cancelOps).toHaveLength(1);
    expect(cancelOps[0]?.args).toMatchObject({ turnId: 1 });
  });

  it('TaskStop kills one background task while its sibling keeps running (per-task cancel)', async () => {
    // Two concurrent backgrounded bash commands (Bash + timeout < duration
    // moves each to the background): A = `sleep 30` (never finishes), B =
    // `sleep 2; echo sibling-ok` (completes ~2s). The main turn then blocks
    // (seg 2) so B's completion notification steers into the RUNNING turn (no
    // notification turn / segment replay). TaskStop A must cancel ONLY A's
    // CancelSignal (engineTaskAdapter → session.cancelTask → the task's own
    // signal) — B continues and its TaskOutput stays readable.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: background A (`sleep 30`).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_a',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 30","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: background B (completes ~2s after start).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_b',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2; echo sibling-ok","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2: block the turn past B's settle (~3s) so the notification
      // steers into the running turn.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 4"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'sibling turn done' }, { type: 'finish', finishReason: 'stop' }],
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

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run two bg' }] });
    // Both backgrounded bash tasks registered (kind process, in launch order:
    // A first, B second).
    await waitFor(
      () =>
        busEvents.filter(
          (event) =>
            event['type'] === 'task.started' &&
            (event['info'] as { kind?: string } | undefined)?.kind === 'process',
        ).length >= 2,
      'two backgrounded bash task starts',
    );
    const started = busEvents.filter(
      (event) =>
        event['type'] === 'task.started' &&
        (event['info'] as { kind?: string } | undefined)?.kind === 'process',
    );
    const aId = (started[0]?.['info'] as { taskId?: string } | undefined)?.taskId;
    const bId = (started[1]?.['info'] as { taskId?: string } | undefined)?.taskId;
    expect(aId).toMatch(/^bash-[0-9a-f]{8}$/);
    expect(bId).toMatch(/^bash-[0-9a-f]{8}$/);

    // TaskStop A (TaskStopTool parity: suppress its terminal notification,
    // then stop through the registered adapter → the engine's per-task
    // cancel). B is a sibling with its OWN cancel signal — it must survive.
    const taskService = ctx.get(IAgentTaskService);
    expect(taskService.list(true).some((task) => task.taskId === aId)).toBe(true);
    expect(taskService.list(true).some((task) => task.taskId === bId)).toBe(true);
    await taskService.suppressTerminalNotification(aId!);
    const result = await taskService.stop(aId!, 'Stopped by TaskStop');
    expect(result?.status).toBe('killed');

    // A settles killed; B settles completed with its real output readable.
    await waitFor(
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === aId && task.status === 'killed',
        ),
      'A killed wire record',
    );
    await waitFor(
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === bId && task.status === 'completed',
        ),
      'B completed wire record',
    );
    // TaskOutput is readable for the surviving sibling (streamed + settle).
    expect(await taskService.readOutput(bId!)).toContain('sibling-ok');
    expect(taskService.list(true).some((task) => task.taskId === aId)).toBe(false);
    // Only B's completion notification was delivered (A's was suppressed).
    const notified = busEvents.filter((event) => event['type'] === 'task.notified');
    expect(notified).toHaveLength(1);
    expect(notified[0]?.['notificationType']).toBe('task.completed');
    expect(notified[0]?.['sourceId']).toBe(bId);
  }, 30_000);

  it('delivers a completion notification for every concurrently settled background task', async () => {
    // Two backgrounded bash commands settle while the main turn is blocked
    // (seg 2): A at ~2s, B at ~3s. Each settle is delivered independently
    // (deliverTaskNotification → steer into the running turn), so BOTH
    // notifications arrive — the runner never drops a settle because another
    // one is being delivered.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_one',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2; echo one-done","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        {
          type: 'tool_call',
          toolCallId: 'call_two',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 3; echo two-done","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Block the turn past both settles (~4s) so both notifications steer
      // into the running turn (no notification turn / segment replay).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 4.5"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'concurrent turn done' }, { type: 'finish', finishReason: 'stop' }],
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

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run two bg' }] });
    await waitFor(
      () =>
        busEvents.filter(
          (event) =>
            event['type'] === 'task.started' &&
            (event['info'] as { kind?: string } | undefined)?.kind === 'process',
        ).length >= 2,
      'two backgrounded bash task starts',
    );
    const started = busEvents.filter(
      (event) =>
        event['type'] === 'task.started' &&
        (event['info'] as { kind?: string } | undefined)?.kind === 'process',
    );
    const aId = (started[0]?.['info'] as { taskId?: string } | undefined)?.taskId;
    const bId = (started[1]?.['info'] as { taskId?: string } | undefined)?.taskId;
    expect(aId).toMatch(/^bash-[0-9a-f]{8}$/);
    expect(bId).toMatch(/^bash-[0-9a-f]{8}$/);

    // Both tasks complete and BOTH notifications are delivered (one per
    // settle), each with its own output preview in the context.
    await waitFor(
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].filter(
          (task) => task.status === 'completed' && task.kind === 'process',
        ).length >= 2,
      'both backgrounded tasks completed',
    );
    await waitFor(
      () => busEvents.filter((event) => event['type'] === 'task.notified').length >= 2,
      'two completion notifications',
    );
    const notified = busEvents.filter((event) => event['type'] === 'task.notified');
    expect(notified.map((event) => event['notificationType'])).toEqual([
      'task.completed',
      'task.completed',
    ]);
    expect(new Set(notified.map((event) => event['sourceId']))).toEqual(new Set([aId, bId]));
    // Both task-origin notification messages are mirrored into the context
    // (one per settle — each carries its own output preview).
    await waitForContext(
      ctx,
      (messages) =>
        messages.filter((message) => message.origin?.kind === 'task').length >= 2,
      'two task notification messages',
    );
    const notificationText = ctx
      .get(IAgentContextMemoryService)
      .get()
      .filter((message) => message.origin?.kind === 'task')
      .map((message) => messageText(message))
      .join('\n');
    expect(notificationText).toContain('one-done');
    expect(notificationText).toContain('two-done');
    // Both tasks' outputs are readable via TaskOutput.
    const taskService = ctx.get(IAgentTaskService);
    expect(await taskService.readOutput(aId!)).toContain('one-done');
    expect(await taskService.readOutput(bId!)).toContain('two-done');
  }, 30_000);

  it('streams tool-call argument fragments as tool.call.delta protocol events', async () => {
    // The scripted client forwards every `tool_call` stream event; the engine
    // emits one `tool.call.delta` per arguments fragment and the full parsed
    // call on `tool.call.started`. The runner publishes the deltas on the bus
    // (the transcript projection accumulates them), and the context mirror
    // carries the CONCATENATED full arguments.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_delta',
          name: 'Bash',
          argumentsPart: '{"command":"echo ',
        },
        { type: 'tool_call', toolCallId: 'call_delta', argumentsPart: 'delta-ok"}' },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const deltas: Array<{ toolCallId?: string; name?: string; argumentsPart?: string }> = [];
    const startedCalls: Array<{ toolCallId?: string; name?: string; args?: unknown }> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'tool.call.delta') deltas.push(event as typeof deltas[number]);
      if (type === 'tool.call.started') startedCalls.push(event as typeof startedCalls[number]);
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run delta tool' }] });
    await waitForContext(ctx, (messages) => messages.some((message) => message.role === 'tool'), 'tool message');

    // One delta per streamed fragment, in arrival order, same call id.
    expect(deltas).toHaveLength(2);
    expect(deltas[0]).toMatchObject({
      toolCallId: 'call_delta',
      name: 'Bash',
      argumentsPart: '{"command":"echo ',
    });
    expect(deltas[1]).toMatchObject({
      toolCallId: 'call_delta',
      argumentsPart: 'delta-ok"}',
    });
    // Concatenating the deltas reconstructs the full arguments.
    const fullArgs = deltas.map((delta) => delta.argumentsPart ?? '').join('');
    expect(JSON.parse(fullArgs)).toEqual({ command: 'echo delta-ok' });
    // The full parsed call lands on tool.call.started.
    expect(startedCalls).toHaveLength(1);
    expect(startedCalls[0]).toMatchObject({ toolCallId: 'call_delta', name: 'Bash' });
    expect(startedCalls[0]?.args).toEqual({ command: 'echo delta-ok' });
    // The context mirror carries the full (concatenated) arguments and the
    // real tool result.
    const context = ctx.get(IAgentContextMemoryService).get();
    const assistant = context.find((message) => message.role === 'assistant');
    expect(assistant?.toolCalls).toHaveLength(1);
    expect(JSON.parse(assistant?.toolCalls[0]?.arguments ?? '{}')).toEqual({
      command: 'echo delta-ok',
    });
    const toolMessage = context.find((message) => message.role === 'tool');
    expect(messageText(toolMessage!)).toContain('delta-ok');
  });

  it('compacts against the max_input_tokens window (not max_context_tokens)', async () => {
    // The runner's compaction window is `max_input_tokens ?? max_context_tokens`
    // (rustEngineTurnRunner.maxContextTokens). No product path populates
    // `max_input_tokens` on `ModelCapability` today (the catalog's
    // `modelCapabilities(model)` only sets max_context_tokens, and
    // `scaleModelCapabilityContext` never creates it), so this test injects
    // it at the profile boundary — a test-side stub of `profile.data()` that
    // returns a capability with a SMALL max_input_tokens (2000 → 85% trigger
    // = 1700) and a LARGE max_context_tokens (50000). A ~1830-token history
    // must trigger the engine's compaction. If the runner wrongly fell back
    // to max_context_tokens, 1830 < 42500 → no compaction → the summary
    // assertion times out.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Call 1 = the engine's compaction round (summary); call 2 = the step.
      [
        { type: 'text', delta: 'input-cap compaction summary' },
        { type: 'finish', finishReason: 'stop' },
      ],
      [
        { type: 'text', delta: 'answer after input-cap compaction' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.configure({
      modelCapabilities: {
        image_in: false,
        video_in: false,
        audio_in: false,
        thinking: false,
        tool_use: true,
        max_context_tokens: 50000,
        dynamically_loaded_tools: false,
      },
    });
    ctx.get(IAgentLoopService);
    // Test-side injection of `max_input_tokens` (see comment above).
    const profile = ctx.get(IAgentProfileService);
    const originalData = profile.data.bind(profile);
    profile.data = () => {
      const data = originalData();
      return {
        ...data,
        modelCapabilities: {
          ...data.modelCapabilities,
          max_context_tokens: 50000,
          max_input_tokens: 2000,
        },
      };
    };

    // Seed ~1830 tokens of assistant/tool exchanges (only the summary keeps).
    const blob = 'z'.repeat(300);
    const contextService = ctx.get(IAgentContextMemoryService);
    contextService.append({
      role: 'user',
      content: [{ type: 'text', text: 'u2' }],
      toolCalls: [],
      origin: { kind: 'user' },
      id: 'u2-id',
    });
    for (let i = 0; i < 20; i++) {
      contextService.append({
        role: 'assistant',
        content: [{ type: 'text', text: `a${i}${'y'.repeat(60)}` }],
        toolCalls: [],
        origin: { kind: 'system_trigger', name: 'seed' },
        id: `a${i}-id`,
      });
      contextService.append({
        role: 'tool',
        content: [{ type: 'text', text: blob }],
        toolCalls: [],
        origin: { kind: 'system_trigger', name: 'seed' },
        id: `t${i}-id`,
        toolCallId: `c${i}`,
      });
    }
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'continue' }] });

    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.origin?.kind === 'compaction_summary'),
      'compaction summary message',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    // The compaction summary message carries the summary; tool blobs are gone.
    const summaryMessage = context.find((message) => message.origin?.kind === 'compaction_summary');
    expect(summaryMessage).toBeDefined();
    const allText = context
      .flatMap((message) => message.content)
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('\n');
    expect(allText).toContain('input-cap compaction summary');
    expect(allText).not.toContain(blob);
    // The post-compaction step answer is present.
    expect(allText).toContain('answer after input-cap compaction');
    // The wire recorded the full-compaction lifecycle ops.
    const wireOps = ctx.snapshots.entries.filter((entry) => entry.type === '[wire]');
    expect(wireOps.some((entry) => entry.event === 'full_compaction.begin')).toBe(true);
    expect(wireOps.some((entry) => entry.event === 'full_compaction.complete')).toBe(true);
  }, 15_000);

  it('lets a nested subagent turn use Bash and mirrors the outcome into the main context', async () => {
    // The subagent's nested turn calls Bash (shared tool registry) and then
    // answers; the nested turn's assistant deltas stream out as `task.output`,
    // and the settle delivers the completion notification into the main
    // context (the runner-visible mirror: nested tool events themselves stay
    // engine-local — only assistant text + the settle output cross the
    // bridge; the engine-level fact that the nested Bash actually executed is
    // pinned by `agent_nested_turn_can_use_bash` in crates/dimi-engine).
    // The nested Bash is slow (sleep 2) so the first notification's assertions
    // run well before the idle-path notification turn replays seg 0 and
    // spawns a second subagent (~2s later; dispose kills it).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: main turn — spawn the subagent (stop_turn ends the turn).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sub_bash',
          name: 'Agent',
          argumentsPart: '{"prompt":"run nested bash","description":"nested sub"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: the subagent's nested turn — Bash (foreground).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_nested_bash',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2; echo nested-bash-ok"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2: the subagent's final answer.
      [
        { type: 'text', delta: 'bash echoed: nested-bash-ok' },
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
        type === 'task.started' ||
        type === 'task.terminated' ||
        type === 'task.notified'
      ) {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn nested bash' }] });
    // The subagent's completion notification mirrors into the context with
    // the subagent's output (which consumed the nested Bash result).
    await waitForContext(
      ctx,
      (messages) =>
        messages.some(
          (message) =>
            message.origin?.kind === 'task' && messageText(message).includes('nested-bash-ok'),
        ),
      'subagent completion notification message',
    );

    const spawned = busEvents.find((event) => event['type'] === 'subagent.spawned');
    expect(spawned).toBeDefined();
    const subagentId = String(spawned?.['subagentId']);
    expect(subagentId).toMatch(/^agent-\d+$/);
    // The nested turn RAN Bash and the subagent completed (a nested Bash
    // failure would surface through the settle output / failed status).
    const completed = busEvents.find((event) => event['type'] === 'subagent.completed');
    expect(completed).toBeDefined();
    expect(String(completed?.['subagentId'])).toBe(subagentId);
    expect(String(completed?.['resultSummary'])).toContain('bash echoed: nested-bash-ok');
    // The wire task record settled completed.
    await waitFor(
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.kind === 'agent' && task.status === 'completed',
        ),
      'subagent completed wire record',
    );
    const settledTask = [...ctx.get(IWireService).getModel(TaskModel).values()].find(
      (task) => task.kind === 'agent' && task.status === 'completed',
    );
    expect(settledTask).toMatchObject({ agentId: subagentId });
    // The streamed nested-turn text is readable via TaskOutput.
    const taskService = ctx.get(IAgentTaskService);
    expect(await taskService.readOutput(settledTask!.taskId)).toContain('bash echoed: nested-bash-ok');
    // Exactly one notification message mirrored into the context at this
    // point (the idle-path replay's second subagent settles ~2s later).
    const taskOriginMessages = ctx
      .get(IAgentContextMemoryService)
      .get()
      .filter((message) => message.origin?.kind === 'task');
    expect(taskOriginMessages).toHaveLength(1);
    expect(messageText(taskOriginMessages[0]!)).toContain('nested-bash-ok');
    const notified = busEvents.find((event) => event['type'] === 'task.notified');
    expect(notified?.['notificationType']).toBe('task.completed');
  }, 30_000);

  it('fails the main turn when the provider filters the response (PROVIDER_FILTERED)', async () => {
    // A `filtered` finish reason (provider safety block) ends the turn as
    // FAILED with the PROVIDER_FILTERED code (engine.rs
    // `finish_turn_with_error`): the `turn.ended` event carries reason
    // 'failed' + the error, and the runner surfaces the TS `error` bus event
    // (TS failLoopStep parity).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'finish', finishReason: 'filtered' }],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'turn.ended' || type === 'error' || type === 'turn.step.completed') {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    // The prompt still resolves at launch (TS launched parity).
    const launched = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run filtered' }] });
    expect(launched).toEqual({ turn_id: 0 });

    await waitFor(
      () =>
        busEvents.some(
          (event) => event['type'] === 'turn.ended' && event['reason'] === 'failed',
        ),
      'failed turn.ended',
    );
    const turnEnded = busEvents.find(
      (event) => event['type'] === 'turn.ended' && event['reason'] === 'failed',
    );
    expect(turnEnded?.['error']).toMatchObject({
      message: 'Provider safety policy blocked the response.',
      code: 'PROVIDER_FILTERED',
    });
    // The TS-parity `error` bus event carries the code.
    const errorEvent = busEvents.find((event) => event['type'] === 'error');
    expect(errorEvent?.['code']).toBe('PROVIDER_FILTERED');
    expect(String(errorEvent?.['message'])).toContain('safety policy');
    // The step completed with the normalized 'filtered' finish reason.
    const stepCompleted = busEvents.find((event) => event['type'] === 'turn.step.completed');
    expect(stepCompleted?.['finishReason']).toBe('filtered');
    // No assistant output was produced.
    const context = ctx.get(IAgentContextMemoryService).get();
    expect(context.some((message) => message.role === 'assistant')).toBe(false);
  });
});
