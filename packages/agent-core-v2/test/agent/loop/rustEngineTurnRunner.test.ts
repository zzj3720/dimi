/**
 * M3 slice-1 swap-in test: `DIMI_RUST_ENGINE=1` routes the turn through the
 * Rust engine (`rustEngineTurnRunner`). With `DIMI_RUST_ENGINE_SCRIPTED`
 * injecting scripted LLM segments, the full wiring is exercised: user
 * message → Rust engine → events on the bus → context records (step/tool/
 * assistant message) — the same surfaces the TS loop drives.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { RustTurnSession } from '@dimi-agent/dimi-native';

import { COMPLETION_REVIEW_REMINDER } from '#/agent/completion/completion';
import { IAgentContextMemoryService } from '#/agent/contextMemory/contextMemory';
import { IAgentLoopService } from '#/agent/loop/loop';
import { IRustEngineTurnRunner, RustEngineTurnRunner } from '#/agent/loop/rustEngineTurnRunner';
import { IAgentProfileService } from '#/agent/profile/profile';
import { IAgentScopeContext } from '#/agent/scopeContext/scopeContext';
import { IAgentTaskService } from '#/agent/task/task';
import { TaskModel } from '#/agent/task/taskOps';
import { IAgentToolActivationService } from '#/agent/toolActivation/toolActivation';
import { IAgentToolPolicyService } from '#/agent/toolPolicy/toolPolicy';
import { IAgentToolRegistryService } from '#/agent/toolRegistry/toolRegistry';
import { IAgentToolResultTruncationService } from '#/agent/toolResultTruncation/toolResultTruncation';
import type { ExecutableTool } from '#/tool/toolContract';
import { IWireService } from '#/wire/wire';
import { IEventBus } from '#/app/event/eventBus';
import {
  agentService,
  configServices,
  createTestAgent,
  externalHookServices,
  InMemoryWireRecordPersistence,
  logServices,
  permissionModeServices,
  type TestAgentContext,
} from '../../harness';

// The Rust engine is the default runtime (CLI `--legacy` sets DIMI_LEGACY=1
// to keep the TS loop); these suites run against the default path.
const DIMI_LEGACY = 'DIMI_LEGACY';
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

describe('Rust engine turn runner (default)', () => {
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
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('<rust-answer>')),
      'assistant message with <rust-answer>',
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

  it('advertises Agent/AgentOutput/WaitFor defs to the model', async () => {
    // The Rust-native tools are registered executor-first on the engine side
    // (no LLM-facing def), so the model cannot see them in the request
    // `tools` field. The runner must push their defs through the bridge.
    //
    // The request `tools` field itself is NOT directly observable from the
    // TS side: the TS harness's scripted-generate records LLM requests only
    // for the TS loop (llmRequester), not the Rust path, and the engine's
    // scripted client (`ScriptedLlmClient`) ignores the request. The chain
    // from these calls to the request is engine/bridge-side — the bridge's
    // `engine_tools()` converts the registry written by `registerNativeToolDef`
    // and `run()`/`resume()` re-sync it into every request — and is pinned by
    // the engine's own `updated_tools_are_advertised_in_subsequent_requests`
    // unit test. What the runner test CAN assert truthfully: (a) exactly the
    // three native defs are pushed (no Bash, no extras, one call each), (b)
    // each def is well-formed, and (c) the calls go through the REAL napi
    // method without throwing and with no registration error logged — a
    // bridge rejection (unknown native name / malformed JSON / busy lock)
    // would now fail the turn explicitly (P2-1), not hang.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'text', delta: '<defs-answer>' }, { type: 'finish', finishReason: 'stop' }],
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
    ctx = createTestAgent([logServices(logger)]);
    ctx.get(IAgentLoopService);

    // Spy on the bridge entry point: the runner calls the wrapper method once
    // per native tool with its LLM-facing def. The call goes through to the
    // real napi method so the def lands in the engine registry.
    const recorded: Array<{ name: string; description: string; parametersJson: string }> = [];
    const proto = RustTurnSession.prototype as unknown as {
      registerNativeToolDef?: (name: string, description: string, parametersJson: string) => void;
    };
    const original = proto.registerNativeToolDef;
    proto.registerNativeToolDef = function (name, description, parametersJson) {
      recorded.push({ name, description, parametersJson });
      original?.call(this, name, description, parametersJson);
    };
    try {
      await ctx.rpc.prompt({ input: [{ type: 'text', text: 'Hello' }] });
      await waitForContext(
        ctx,
        (messages) => messages.some((message) => message.role === 'assistant'),
        'assistant message',
      );
    } finally {
      proto.registerNativeToolDef = original;
    }

    // Exactly the three async tools — one def each, never Bash, nothing else.
    const names = recorded.map((record) => record.name).sort();
    expect(names).toEqual(['Agent', 'AgentOutput', 'WaitFor']);
    // The real napi accepted every def (the spy calls through): a rejected
    // def would have failed the turn, and P2-1 would surface it as a logged
    // registration error — assert none fired.
    expect(errors).toEqual([]);
    const byName = new Map(recorded.map((record) => [record.name, record]));
    for (const name of ['Agent', 'AgentOutput', 'WaitFor']) {
      const def = byName.get(name);
      expect(def, `${name} def advertised to the engine`).toBeDefined();
      expect(def!.description.length).toBeGreaterThan(0);
      const parameters = JSON.parse(def!.parametersJson) as { type?: unknown; properties?: unknown };
      expect(parameters.type).toBe('object');
      expect(parameters.properties).toBeDefined();
    }
    // P1-2: WaitFor's def must match the ENGINE implementation. The TS
    // waitForTool.ts schema (`reason`/`timeout_seconds`, no `agent_id`)
    // describes the TS user-wait semantics, but the engine waits for a
    // background SUBAGENT task by `agent_id` — advertising the TS schema made
    // the model call without `agent_id`, which the engine then resolved to an
    // empty id and parked for the full 60s timeout. The runner must advertise
    // `agent_id` as required and describe the subagent-wait boundary honestly
    // (user-wait / notification-wake semantics are NOT implemented).
    const waitForDef = byName.get('WaitFor')!;
    const waitForParameters = JSON.parse(waitForDef.parametersJson) as {
      type?: unknown;
      properties?: Record<string, unknown>;
      required?: string[];
    };
    expect(waitForParameters.required).toEqual(['agent_id']);
    expect(waitForParameters.properties).toHaveProperty('agent_id');
    expect(waitForParameters.properties).toHaveProperty('timeout_seconds');
    expect(waitForDef.description).toContain('subagent');
    expect(waitForDef.description).not.toContain('waits on the current agent');
  });

  it('fails the turn explicitly instead of hanging when a native def registration throws', async () => {
    // P2-1: `registerNativeToolDef` is a synchronous napi call. A throw
    // (unknown native name / malformed parameter JSON / registry lock busy)
    // used to escape `runTurnNow`, get swallowed by `startQueuedTurn`'s
    // `.catch(() => undefined)` and leave the turn hanging forever (the
    // prompt op was already recorded). The runner must log the failure with
    // the tool name and fail the turn so the queue advances — the next prompt
    // starts a fresh turn instead of queueing behind a dead one.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'text', delta: 'second turn answer' }, { type: 'finish', finishReason: 'stop' }],
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
    ctx = createTestAgent([logServices(logger)]);
    ctx.get(IAgentLoopService);

    const proto = RustTurnSession.prototype as unknown as {
      registerNativeToolDef?: (name: string, description: string, parametersJson: string) => void;
    };
    const original = proto.registerNativeToolDef;
    let throwOnce = true;
    proto.registerNativeToolDef = function (name, description, parametersJson) {
      // Simulate a napi-layer rejection (e.g. "no native tool registered with
      // name" / invalid parameters JSON / registry lock busy) for the FIRST
      // registration only — the first turn fails at it, and the second turn
      // (fresh session, re-registration) must succeed to prove the runner is
      // not stuck.
      if (throwOnce) {
        throwOnce = false;
        throw new Error(`simulated registerNativeToolDef failure for ${name}`);
      }
      original?.call(this, name, description, parametersJson);
    };
    try {
      const first = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'first' }] });
      expect(first).toEqual({ turn_id: 0 });
      // The first turn failed at registration: the failure is logged with the
      // tool name (P2-1) before the queue is released.
      await waitForContext(ctx, () => errors.length > 0, 'registration failure log');
      expect(errors[0]!.message).toContain('[rustEngineTurnRunner] failed to register native tool def');
      expect(['Agent', 'AgentOutput', 'WaitFor']).toContain(
        (errors[0]!.payload as { name?: string } | undefined)?.name,
      );
      expect(String((errors[0]!.payload as { error?: unknown } | undefined)?.error)).toContain(
        'simulated registerNativeToolDef failure',
      );

      // The runner is NOT stuck: the second prompt starts a fresh turn (id 1)
      // instead of queueing behind the dead first turn (a hang would return
      // `undefined` here and never produce an assistant message).
      const second = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'second' }] });
      expect(second).toEqual({ turn_id: 1 });
      await waitForContext(
        ctx,
        (messages) =>
          messages.some((message) =>
            message.role === 'assistant' &&
            message.content
              .filter((part) => part.type === 'text')
              .map((part) => (part as { text?: string }).text ?? '')
              .join('')
              .includes('second turn answer')),
        'second turn answer',
      );
    } finally {
      proto.registerNativeToolDef = original;
    }
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
    // Agent no longer stops the caller's turn (same-turn continue parity):
    // Segment 0 spawns the subagent and the main turn KEEPS GOING. Segment 1
    // is the main turn's own blocking Bash (`sleep 2`) — it keeps the turn
    // alive while the subagent runs in the background. Segment 2 is the
    // subagent's nested turn (the shared scripted cursor hands the remaining
    // segments to the worker): it answers immediately, so the completion
    // notification folds into the RUNNING main turn (mid-turn steer), not an
    // idle notification turn. The subagent settling must reach the TS side
    // as task wire ops + bus events + a task notification message.
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
    // The completion folds into the RUNNING main turn (the main turn is
    // still blocked in Bash when the subagent settles): the notification XML
    // lands in the context EXACTLY ONCE (P1-1: the steer path appends it once
    // and the engine drains the steer into its next request — no pre-append
    // + steer duplicate, no runTurn re-append). Because the steer actually
    // lands, a `turn.steer` op IS recorded (P1-2: the op is only written
    // when a steer lands — the old stop_turn flow had no live turn to steer,
    // so it recorded zero ops and used an idle notification turn instead).
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
    // The mid-turn steer recorded exactly one `turn.steer` op (the subagent
    // completed while the main turn was still running).
    expect(steerOps).toHaveLength(1);
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
    // Agent no longer stops the caller's turn (same-turn continue parity):
    // Seg 0 spawns the subagent and the main turn KEEPS GOING. Seg 1 is the
    // main turn's own blocking Bash (`sleep 2`) — during it, the worker's
    // nested turn consumes the NEXT scripted segment, so Seg 2 (filtered by
    // the (scripted) provider) is what the SUBAGENT sees → the subagent
    // fails and the notification body carries the TS
    // `buildAgentTaskNotificationBody` recovery block, including the
    // `run_in_background` guidance line. Seg 3 is the main turn's text reply
    // after the sleep. The failure lands mid-turn, so the notification
    // steers into the running turn instead of launching a notification turn.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: main turn — spawn the subagent.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sub_fail',
          name: 'Agent',
          argumentsPart: '{"prompt":"sub work","description":"failing sub"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: main turn — block so the failing subagent's settle steers
      // into this running turn.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_block',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2: the subagent's nested turn (consumed while the main blocks) —
      // filtered by the provider.
      [{ type: 'finish', finishReason: 'filtered' }],
      // Seg 3: main turn — text reply ends the turn.
      [{ type: 'text', delta: 'spawned failing sub' }, { type: 'finish', finishReason: 'stop' }],
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

  it('settles a TaskStopped subagent killed mid-stream with the streamed output (wire + TaskOutput + notification)', async () => {
    // F3 (adversarial review nit): engine-level tests cover killed subagents
    // settling with streamed output, but no runner-side test asserts the wire
    // `task.terminated` outputTail / the TaskOutput retained buffer / the
    // notification body for a KILLED subagent that streamed text. Agent no
    // longer stops the caller's turn, so Seg 1 is the main turn's own text
    // reply (it ends right after the launch); Seg 2 is the subagent's nested
    // turn — it streams text, then blocks in a foreground Bash so the
    // TaskStop lands mid-run. The adapter streams the deltas into the
    // task-service sink live, and the engine settle carries the same text, so
    // outputTail == retained buffer == notification preview all show the
    // streamed text (delta == settle on the killed path).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: main turn — spawn the subagent.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_sub_stream',
          name: 'Agent',
          argumentsPart: '{"prompt":"stream then block","description":"streaming sub"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: main turn — text reply ends the turn; the subagent keeps
      // running in the background.
      [{ type: 'text', delta: 'streaming sub launched' }, { type: 'finish', finishReason: 'stop' }],
      // Seg 2: the subagent's nested turn — stream text, then block in a
      // foreground Bash so the test can TaskStop it mid-run.
      [
        { type: 'text', delta: 'part one ' },
        { type: 'text', delta: 'part two' },
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

    // Capture the dispatched wire ops: the `task.terminated` outputTail is
    // fold-only (taskOps.ts) — it never enters TaskModel — so assert it
    // straight off the dispatch.
    const wire = ctx.get(IWireService);
    const dispatched: Array<{ type: string; payload: Record<string, unknown> }> = [];
    const originalDispatch = wire.dispatch.bind(wire);
    wire.dispatch = ((...ops) => {
      for (const op of ops) {
        dispatched.push({ type: op.type, payload: op.payload as Record<string, unknown> });
      }
      originalDispatch(...ops);
    }) as typeof wire.dispatch;

    const busEvents: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'task.started' || type === 'task.terminated' || type === 'task.notified') {
        busEvents.push(event as Record<string, unknown>);
      }
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn streaming sub' }] });
    // The subagent is live once its `task.started` (kind agent) arrives.
    await waitForContext(
      ctx,
      () =>
        dispatched.some(
          (op) =>
            op.type === 'task.started' &&
            (op.payload['info'] as { kind?: string } | undefined)?.kind === 'agent',
        ),
      'subagent task.started wire op',
    );
    const started = dispatched.find(
      (op) =>
        op.type === 'task.started' &&
        (op.payload['info'] as { kind?: string } | undefined)?.kind === 'agent',
    );
    const taskId = (started?.payload['info'] as { taskId?: string } | undefined)?.taskId;
    expect(taskId).toMatch(/^agent-[0-9a-f]{8}$/);

    // Wait until the streamed deltas reached the task-service sink (by then
    // the nested turn is blocked in its foreground Bash — TaskStop lands
    // after the text has streamed, so the killed settle carries it).
    const taskService = ctx.get(IAgentTaskService);
    for (let i = 0; i < 600; i++) {
      if ((await taskService.readOutput(taskId!)).includes('part two')) break;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    expect(await taskService.readOutput(taskId!)).toContain('part two');

    // TaskStop parity: kill through the service entry (the adapter bridges
    // into session.cancelTask → the engine's per-task cancel → the nested
    // turn aborts and the worker settles "killed"). No notification
    // suppression, so the terminal notification still fires. A CUSTOM reason
    // proves the engine's killed settle carries the TS reason end-to-end
    // (F3.5: the wire stopReason must not be the engine's hardcoded default).
    const result = await taskService.stop(taskId!, 'user abort');
    expect(result?.status).toBe('killed');

    // The wire `task.terminated` carries the info and the outputTail; the
    // streamed text is small so the tail is the FULL output (delta == settle
    // byte-for-byte on the killed path).
    await waitForContext(
      ctx,
      () =>
        dispatched.some(
          (op) =>
            op.type === 'task.terminated' &&
            (op.payload['info'] as { taskId?: string } | undefined)?.taskId === taskId,
        ),
      'subagent task.terminated wire op',
    );
    const terminated = dispatched.find(
      (op) =>
        op.type === 'task.terminated' &&
        (op.payload['info'] as { taskId?: string } | undefined)?.taskId === taskId,
    );
    expect(terminated?.payload['info']).toMatchObject({
      taskId,
      kind: 'agent',
      status: 'killed',
      stopReason: 'user abort',
    });
    expect((terminated?.payload['info'] as { agentId?: string } | undefined)?.agentId).toMatch(
      /^agent-\d+$/,
    );
    expect(terminated?.payload['outputTail']).toBe('part one part two');

    // The wire record (TaskModel) reflects the killed settle.
    await waitForContext(
      ctx,
      () =>
        [...ctx.get(IWireService).getModel(TaskModel).values()].some(
          (task) => task.taskId === taskId && task.status === 'killed',
        ),
      'subagent killed wire record',
    );
    expect(taskService.list(false).some((task) => task.taskId === taskId)).toBe(true);

    // TaskOutput retained buffer: the live-streamed deltas plus the settle
    // tail equal the full streamed text exactly once (no duplication).
    expect(await taskService.readOutput(taskId!)).toBe('part one part two');

    // The terminal notification fired for the kill and its output preview
    // carries the streamed text into the context.
    const notified = busEvents.find((event) => event['type'] === 'task.notified');
    expect(notified?.['notificationType']).toBe('task.killed');
    await waitForContext(
      ctx,
      (messages) =>
        messages.some(
          (message) =>
            message.origin?.kind === 'task' &&
            message.content
              .filter((part) => part.type === 'text')
              .map((part) => (part as { text?: string }).text ?? '')
              .join('')
              .includes('part one part two'),
        ),
      'killed-subagent notification with streamed output preview',
    );
    expect(busEvents.some((event) => event['type'] === 'task.terminated')).toBe(true);
  }, 30_000);

  it('cancels background work and drops late settles when the agent is disposed', async () => {
    // The main turn launches a background subagent whose nested turn blocks in
    // Bash (`sleep 10`). Disposing the agent while the subagent is still
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
          argumentsPart: '{"command":"sleep 10"}',
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
    // Snapshot the events already delivered BEFORE the dispose: the shared
    // scripted client is a single cursor, so the subagent may settle early
    // (empty segment) or the main turn may consume the nested segment — both
    // are legitimate pre-dispose events. The dispose contract is that NO NEW
    // terminal event lands AFTER the teardown.
    const beforeDispose = busEvents.length;
    await ctx.dispose();

    // Give any in-flight nested work time to settle into the closed session.
    await new Promise((resolve) => setTimeout(resolve, 2_500));
    expect(errors).toEqual([]);
    const afterDispose = busEvents.slice(beforeDispose);
    // No terminal task records / notification reached the disposed runner.
    expect(afterDispose.some((event) => event['type'] === 'task.terminated')).toBe(false);
    expect(afterDispose.some((event) => event['type'] === 'task.notified')).toBe(false);
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

describe('Rust vs legacy routing', () => {
  afterEach(() => {
    delete process.env['DIMI_LEGACY'];
  });

  it('uses the Rust engine by default', () => {
    delete process.env['DIMI_LEGACY'];
    expect(RustEngineTurnRunner.isEnabled()).toBe(true);
  });

  it('keeps the TS loop under --legacy (DIMI_LEGACY=1)', () => {
    process.env['DIMI_LEGACY'] = '1';
    expect(RustEngineTurnRunner.isEnabled()).toBe(false);
  });
});

describe('Rust engine approval flow (manual mode)', () => {
  let ctx: TestAgentContext;

  beforeEach(() => {
    delete process.env[DIMI_LEGACY];
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
    delete process.env[DIMI_LEGACY];
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

  it('auto-approves the same tool later in the turn after a session-scope approval', async () => {
    // P1-6 (review): approving a tool for the session must be honored by the
    // SAME turn — the resumed batch's second call to the same tool runs
    // without a second approval request (TS session-approval-history reads
    // live; the engine's frozen policy is updated via addSessionApproval).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_ask_1',
          name: 'Bash',
          argumentsPart: '{"command":"echo one"}',
        },
        {
          type: 'tool_call',
          toolCallId: 'call_ask_2',
          name: 'Bash',
          argumentsPart: '{"command":"echo two"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'session approved done' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);

    const approvalPromise = ctx.takeApprovalRequest();
    const promptPromise = ctx.rpc.prompt({ input: [{ type: 'text', text: 'run session' }] });

    const approval = await approvalPromise;
    approval.respond({ decision: 'approved', scope: 'session' });
    await promptPromise;

    // Both batch calls ran — the second was auto-approved by the session
    // pattern (if it re-asked, promptPromise would never resolve).
    await waitForContext(
      ctx,
      (messages) => messages.filter((m) => m.role === 'tool').length >= 2,
      'two tool messages',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    const toolMessages = context.filter((message) => message.role === 'tool');
    expect(toolMessages.length).toBe(2);
    const text = toolMessages
      .flatMap((m) =>
        (m.content ?? [])
          .filter((part) => part.type === 'text')
          .map((part) => (part as { text?: string }).text ?? ''),
      )
      .join('');
    expect(text).toContain('one');
    expect(text).toContain('two');
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

  it('injects the completion-review reminder after a long tool-free turn and forces AllDone', async () => {
    // 10 tool-call steps keep the turn running; the text-only reply at step
    // 11 crosses the short-turn threshold, so the engine must inject the
    // completion-review reminder and keep the turn alive until the model
    // calls AllDone (TS loopContinuationService parity). The runner mirrors
    // the injected reminder into the context as a system_trigger message.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      ...Array.from({ length: 10 }, (_, i) => [
        {
          type: 'tool_call',
          toolCallId: `call_work_${i}`,
          name: 'Bash',
          argumentsPart: `{"command":"echo work-${i}"}`,
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ]),
      [{ type: 'text', delta: 'Everything is verified.' }, { type: 'finish', finishReason: 'stop' }],
      [
        { type: 'tool_call', toolCallId: 'call_done', name: 'AllDone', argumentsPart: '{}' },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    // AllDone is contributed only when the profile is runnable
    // (profileName set); the completion-review gate mirrors that.
    ctx.get(IAgentProfileService).update({ profileName: 'agent' });
    await ctx.get(IAgentToolActivationService).activate();
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'Complete the task' }] });

    // The AllDone tool result lands in the context only when the engine ran
    // the final step — without the injection the turn ends at the tool-free
    // reply and this never happens.
    await waitForContext(
      ctx,
      (messages) =>
        messages.some(
          (message) =>
            message.role === 'tool' &&
            message.content
              .filter((part) => part.type === 'text')
              .map((part) => (part as { text?: string }).text ?? '')
              .join('')
              .includes('All work is complete.'),
        ),
      'AllDone tool result',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    // Exactly one completion-review reminder was injected (TS parity) and it
    // carries the shared reminder text.
    const reminders = context.filter(
      (message) =>
        message.origin?.kind === 'system_trigger' && message.origin.name === 'completion_review',
    );
    expect(reminders).toHaveLength(1);
    // P2-4: the mirrored reminder must be wrapped in `<system-reminder>`
    // markers — TS `AgentSystemReminderService.appendSystemReminder` parity —
    // never injected as bare text (the engine wraps the configured reminder
    // and the runner mirrors it; an already-wrapped reminder is left alone).
    const reminderText = reminders[0]!.content
      .filter((part) => part.type === 'text')
      .map((part) => (part as { text?: string }).text ?? '')
      .join('');
    expect(reminderText).toBe(
      `<system-reminder>\n${COMPLETION_REVIEW_REMINDER.trim()}\n</system-reminder>`,
    );
    expect(JSON.stringify(reminders)).toContain(COMPLETION_REVIEW_REMINDER.trim());
    // The review was forced: an assistant step calling AllDone exists.
    const allDoneCall = context.find(
      (message) =>
        message.role === 'assistant' && message.toolCalls.some((call) => call.name === 'AllDone'),
    );
    expect(allDoneCall).toBeDefined();
  }, 30_000);

  it('ends a short turn on a text-only reply without the completion reminder', async () => {
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'text', delta: 'Done — here is the answer.' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentProfileService).update({ profileName: 'agent' });
    await ctx.get(IAgentToolActivationService).activate();
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'What is 2+2?' }] });

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
              .includes('Done — here is the answer.'),
        ),
      'short text reply',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    const reminders = context.filter(
      (message) =>
        message.origin?.kind === 'system_trigger' && message.origin.name === 'completion_review',
    );
    expect(reminders).toHaveLength(0);
    // A quick answer ends the turn directly: exactly one assistant reply.
    expect(context.filter((message) => message.role === 'assistant')).toHaveLength(1);
  });

  it('rejects AllDone mixed with another tool through the engine (tool-side validation)', async () => {
    // The completion review only prompts the model to call AllDone; the
    // AllDone tool itself validates the round (mixed-call / background-task
    // rejection). The engine forwards the step's full tool-call batch so the
    // TS tool can reject a mixed round — and the sibling Bash call still runs.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'tool_call', toolCallId: 'call_done_mixed', name: 'AllDone', argumentsPart: '{}' },
        {
          type: 'tool_call',
          toolCallId: 'call_bash_mixed',
          name: 'Bash',
          argumentsPart: '{"command":"echo mixed-ok"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'after mixed round' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentProfileService).update({ profileName: 'agent' });
    await ctx.get(IAgentToolActivationService).activate();
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'Finish after probing' }] });

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
              .includes('after mixed round'),
        ),
      'post-mixed reply',
    );
    const context = ctx.get(IAgentContextMemoryService).get();
    const text = JSON.stringify(context);
    // The AllDone call was rejected by the TOOL (mixed round) — the engine
    // did not silently accept it — and the sibling Bash call still ran.
    expect(text).toContain('AllDone must be the only tool call in its round.');
    expect(text).toContain('mixed-ok');
  });

  it('records per-step usage: a usage-less step gets zeros, not the previous step (TS parity)', async () => {
    // TS `finishStep` writes the CURRENT LLM response's usage on every
    // step.end (llmRequesterService starts each request at emptyUsage), so a
    // step with no engine usage must NOT carry the previous step's numbers
    // forward. Step 1 reports 100 prompt / 50 completion; step 2 reports none.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_usage',
          name: 'Bash',
          argumentsPart: '{"command":"echo usage"}',
        },
        { type: 'usage', promptTokens: 100, completionTokens: 50 },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'usage done' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    const persistence = new InMemoryWireRecordPersistence();
    ctx = createTestAgent([permissionModeServices('auto')], { persistence });
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'measure usage' }] });

    await waitForContext(
      ctx,
      (messages) => messages.filter((message) => message.role === 'assistant').length >= 2,
      'two assistant messages',
    );
    const stepEnds = persistence.records.filter(
      (record) =>
        record.type === 'context.append_loop_event' &&
        (record as { event?: { type?: string } }).event?.type === 'step.end',
    ) as unknown as Array<{
      event: {
        type: 'step.end';
        step: number;
        usage: { inputOther: number; output: number; inputCacheRead: number; inputCacheCreation: number };
      };
    }>;
    expect(stepEnds).toHaveLength(2);
    // Step 1: the engine's usage (100 prompt / 50 completion) converted to
    // the four-component TS shape.
    expect(stepEnds[0]!.event.usage).toEqual({
      inputOther: 100,
      output: 50,
      inputCacheRead: 0,
      inputCacheCreation: 0,
    });
    // Step 2: no engine usage → zeros (never the previous step's numbers).
    expect(stepEnds[1]!.event.usage).toEqual({
      inputOther: 0,
      output: 0,
      inputCacheRead: 0,
      inputCacheCreation: 0,
    });
  });

  it('records streamed parts in stream order, not merged think-before-text (TS parity)', async () => {
    // TS `appendResponseContent` iterates the provider message content in
    // arrival order — [text, think, text] stays [text, think, text]. The
    // runner must not reorder deltas into a merged think-then-text pair.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'text', delta: 'first text' },
        { type: 'thinking', delta: 'middle think' },
        { type: 'text', delta: 'last text' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'interleave' }] });

    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.role === 'assistant'),
      'assistant message',
    );
    const assistant = ctx
      .get(IAgentContextMemoryService)
      .get()
      .find((message) => message.role === 'assistant')!;
    const parts = assistant.content.filter(
      (part) => part.type === 'text' || part.type === 'think',
    );
    expect(
      parts.map((part) =>
        part.type === 'text'
          ? `text:${(part as { text?: string }).text}`
          : `think:${(part as { think?: string }).think}`,
      ),
    ).toEqual(['text:first text', 'think:middle think', 'text:last text']);
  });

  it('publishes a failed-turn error with a name (TS toDimiErrorPayload parity)', async () => {
    // TS `turn.ended` failed errors go through `toDimiErrorPayload`, which
    // always carries the error class name. The engine payload is
    // `{ message, code }` — the runner must fill `name` before publishing.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'error', message: 'provider exploded' }],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);
    const errors: Array<Record<string, unknown>> = [];
    const turnEnded: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'error') {
        errors.push(event as Record<string, unknown>);
      } else if (type === 'turn.ended') {
        turnEnded.push(event as Record<string, unknown>);
      }
    });
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'fail me' }] });
    for (let i = 0; i < 600 && errors.length === 0; i += 1) {
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    expect(errors.length).toBeGreaterThan(0);
    expect(errors[0]!['message']).toBe('provider exploded');
    expect(errors[0]!['name']).toBeDefined();
    // P1-5 (adversarial review): the `turn.ended` event itself also carries a
    // full DimiErrorPayload (name/retryable), not the engine's {message,code}.
    const ended = turnEnded.find((e) => e['reason'] === 'failed');
    expect(ended).toBeDefined();
    const endedError = ended!['error'] as Record<string, unknown>;
    expect(endedError['name']).toBeDefined();
    expect(endedError['retryable']).toBe(false);
  });

  it('maps PROVIDER_FILTERED to ProviderFilteredError with retryable false (P1-5)', async () => {
    // A content-filter finish fails the turn with code PROVIDER_FILTERED;
    // the runner maps it to the TS class name and the registry's
    // retryable: false verdict (provider.filtered is not retryable).
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'text', delta: 'filtered' },
        { type: 'finish', finishReason: 'content_filter' },
      ],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);
    const turnEnded: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      if ((event as { type?: string }).type === 'turn.ended') {
        turnEnded.push(event as Record<string, unknown>);
      }
    });
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'filter me' }] });
    for (let i = 0; i < 600 && turnEnded.length === 0; i += 1) {
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    const ended = turnEnded.find((e) => e['reason'] === 'failed');
    expect(ended).toBeDefined();
    const endedError = ended!['error'] as Record<string, unknown>;
    expect(endedError['name']).toBe('ProviderFilteredError');
    expect(endedError['code']).toBe('PROVIDER_FILTERED');
    expect(endedError['retryable']).toBe(false);
  });

  it('streams external-tool updates as tool.progress (TS dispatchToolProgress parity)', async () => {
    const stubTool: ExecutableTool = {
      name: 'StubProgress',
      description: 'emits progress updates',
      parameters: { type: 'object', properties: {} },
      resolveExecution: () => ({
        isError: false,
        approvalRule: 'allow',
        execute: async ({ onUpdate }) => {
          onUpdate?.({ kind: 'progress', text: 'halfway' });
          onUpdate?.({ kind: 'status', text: 'finished' });
          return { output: 'stub done', isError: false };
        },
      }),
    };
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_progress',
          name: 'StubProgress',
          argumentsPart: '{}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'after progress' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentToolRegistryService).register(stubTool);
    ctx.get(IAgentLoopService);
    const progress: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      if ((event as { type?: string }).type === 'tool.progress') {
        progress.push(event as Record<string, unknown>);
      }
    });
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run progress tool' }] });

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('after progress')),
      'post-progress reply',
    );
    expect(progress.length).toBeGreaterThanOrEqual(2);
    expect(progress[0]!['toolCallId']).toBe('call_progress');
    expect(progress[0]!['update']).toMatchObject({ kind: 'progress', text: 'halfway' });
    expect(progress[1]!['update']).toMatchObject({ kind: 'status', text: 'finished' });
  });

  it('flushes completed-response text when a step is interrupted mid-tool (TS parity)', async () => {
    // TS `appendResponseContent` lands the step's text the moment its LLM
    // response completes (before tools run). When the turn is cancelled
    // during the tool phase, that text must survive in the context; the
    // runner must flush on `turn.step.interrupted`, not only on
    // `turn.step.completed`.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'text', delta: 'partial-before-tool' },
        {
          type: 'tool_call',
          toolCallId: 'call_slow_interrupt',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2"}',
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
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'interrupt me' }] });
    await new Promise((resolve) => setTimeout(resolve, 200)); // text emitted, Bash sleeping
    const runner = ctx.get(IRustEngineTurnRunner);
    expect(runner.cancel()).toBe(true);
    expect((await ended).reason).toBe('cancelled');
    const context = ctx.get(IAgentContextMemoryService).get();
    expect(JSON.stringify(context)).toContain('partial-before-tool');
  });

  it('consults tool-result truncation for external tool results (P1-7 parity)', async () => {
    const truncationCalls: Array<Record<string, unknown>> = [];
    const truncationService: IAgentToolResultTruncationService = {
      _serviceBrand: undefined,
      truncateForModel: async (input) => {
        truncationCalls.push({ toolName: input.toolName, toolCallId: input.toolCallId });
        return { ...input.result, output: 'TRUNCATED-PREVIEW' };
      },
    };
    const stubTool: ExecutableTool = {
      name: 'StubBig',
      description: 'returns an oversized result',
      parameters: { type: 'object', properties: {} },
      resolveExecution: () => ({
        isError: false,
        approvalRule: 'allow',
        execute: async () => ({ output: 'x'.repeat(100_000), isError: false }),
      }),
    };
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'tool_call', toolCallId: 'call_big', name: 'StubBig', argumentsPart: '{}' },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'after big' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([
      permissionModeServices('auto'),
      agentService(IAgentToolResultTruncationService, truncationService),
    ]);
    ctx.get(IAgentToolRegistryService).register(stubTool);
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run big tool' }] });

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('after big')),
      'post-big reply',
    );
    expect(truncationCalls).toHaveLength(1);
    expect(truncationCalls[0]!['toolName']).toBe('StubBig');
    expect(truncationCalls[0]!['toolCallId']).toBe('call_big');
    // The truncated preview flows through to the wire tool.result.
    const context = JSON.stringify(ctx.get(IAgentContextMemoryService).get());
    expect(context).toContain('TRUNCATED-PREVIEW');
  });

  it('applies PreToolUse veto + PostToolUse to external tools (P1-8 parity)', async () => {
    // TS `fireBeforeExecute` veto settles the call synthetically WITHOUT
    // executing it or firing PostToolUse; a non-vetoed call runs and fires
    // PostToolUse. Both tools in one round pin the two paths.
    const calls: Array<{ event: string; toolName?: string }> = [];
    const hookRunner = {
      trigger: async () => [],
      triggerBlock: async (
        event: string,
        args?: { matcherValue?: string },
      ): Promise<{ block: true; reason: string } | undefined> => {
        calls.push({ event, toolName: args?.matcherValue });
        if (args?.matcherValue === 'StubHook') return { block: true, reason: 'vetoed by test hook' };
        return undefined;
      },
      fireAndForgetTrigger: async (event: string, args?: { matcherValue?: string }) => {
        calls.push({ event, toolName: args?.matcherValue });
        return [];
      },
    };
    const hookTool = (name: string): ExecutableTool => ({
      name,
      description: 'hook target',
      parameters: { type: 'object', properties: {} },
      resolveExecution: () => ({
        isError: false,
        approvalRule: 'allow',
        execute: async () => ({ output: `${name} ran`, isError: false }),
      }),
    });
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'tool_call', toolCallId: 'call_hook', name: 'StubHook', argumentsPart: '{}' },
        { type: 'tool_call', toolCallId: 'call_allow', name: 'StubAllow', argumentsPart: '{}' },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'after hook' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto'), externalHookServices(hookRunner)]);
    ctx.get(IAgentToolRegistryService).register(hookTool('StubHook'));
    ctx.get(IAgentToolRegistryService).register(hookTool('StubAllow'));
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run hook tools' }] });

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('after hook')),
      'post-hook reply',
    );
    // PostToolUse is fire-and-forget: allow a beat for the async trigger.
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(calls).toContainEqual({ event: 'PreToolUse', toolName: 'StubHook' });
    expect(calls).toContainEqual({ event: 'PreToolUse', toolName: 'StubAllow' });
    // The vetoed call never executed → no PostToolUse for it.
    expect(calls).not.toContainEqual({ event: 'PostToolUse', toolName: 'StubHook' });
    // The allowed call executed → PostToolUse fires.
    expect(calls).toContainEqual({ event: 'PostToolUse', toolName: 'StubAllow' });
    const context = JSON.stringify(ctx.get(IAgentContextMemoryService).get());
    // The vetoed call's tool result carries the block reason; the allowed
    // call ran its executor.
    expect(context).toContain('vetoed by test hook');
    expect(context).toContain('StubAllow ran');
  });

  it('passes the turn origin through to the engine turn.started (TS parity)', async () => {
    // The runner serializes the prompt origin into the wire `TurnOrigin`
    // shape and the engine echoes it on `turn.started` — task-origin
    // notification turns must not arrive as plain user turns.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'text', delta: 'task turn reply' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);
    const started: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      if ((event as { type?: string }).type === 'turn.started') {
        started.push(event as Record<string, unknown>);
      }
    });
    const runner = ctx.get(IRustEngineTurnRunner);
    const launched = await runner.runTurn({
      input: [{ type: 'text', text: 'task notification' }],
      origin: {
        kind: 'task',
        taskId: 'task_1',
        status: 'completed',
        notificationId: 'task_1:completed',
      },
    });
    expect(launched).toBeDefined();
    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('task turn reply')),
      'task turn reply',
    );
    expect(started.length).toBeGreaterThan(0);
    expect(started[0]!['origin']).toMatchObject({ kind: 'task', taskId: 'task_1' });
    // P1-1 (adversarial review): a task-origin turn must NOT leak its
    // steering prompt into `turn.started.prompt` (TS `isDisplayablePromptOrigin`).
    expect(started[0]!['prompt']).toBeUndefined();
  });

  it('filters tools by the profile activeToolNames (TS isToolActive parity)', async () => {
    // The runner registers only ACTIVE tools with the engine. A tool the
    // model calls but that is filtered out must not resolve — the engine
    // answers `Tool "X" not found` instead of executing it.
    const stubTool = (name: string): ExecutableTool => ({
      name,
      description: 'filter target',
      parameters: { type: 'object', properties: {} },
      resolveExecution: () => ({
        isError: false,
        approvalRule: 'allow',
        execute: async () => ({ output: `${name} executed`, isError: false }),
      }),
    });
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_inactive',
          name: 'StubInactive',
          argumentsPart: '{}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'after inactive' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentToolRegistryService).register(stubTool('StubActive'));
    ctx.get(IAgentToolRegistryService).register(stubTool('StubInactive'));
    ctx.get(IAgentProfileService).update({ activeToolNames: ['StubActive'] });
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'call filtered tool' }] });

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('after inactive')),
      'post-filtered reply',
    );
    const toolText = ctx
      .get(IAgentContextMemoryService)
      .get()
      .filter((message) => message.role === 'tool')
      .map((message) => JSON.stringify(message.content))
      .join('');
    // The filtered tool was never registered → the engine rejects the call.
    expect(toolText).toContain('not found');
    expect(toolText).not.toContain('StubInactive executed');
  });

  it('consults the composed tool policy for the engine allowlist (P1-2)', async () => {
    // The effective allowlist must come from `IAgentToolPolicyService`
    // (profile + global [tools] + session denylist composition), not a
    // profile-only filter — the runner consults the service per tool.
    const policyStub: IAgentToolPolicyService = {
      _serviceBrand: undefined,
      isToolActive: (name: string) => name !== 'StubPolicyOff',
      isToolActiveForDisclosure: () => true,
      isToolActiveForProfile: () => true,
      setSessionDisabledTools: async () => {},
    };
    const stubTool = (name: string): ExecutableTool => ({
      name,
      description: 'policy target',
      parameters: { type: 'object', properties: {} },
      resolveExecution: () => ({
        isError: false,
        approvalRule: 'allow',
        execute: async () => ({ output: `${name} executed`, isError: false }),
      }),
    });
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_policy_off',
          name: 'StubPolicyOff',
          argumentsPart: '{}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'after policy' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([
      permissionModeServices('auto'),
      agentService(IAgentToolPolicyService, policyStub),
    ]);
    ctx.get(IAgentToolRegistryService).register(stubTool('StubPolicyOn'));
    ctx.get(IAgentToolRegistryService).register(stubTool('StubPolicyOff'));
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'call policy tool' }] });

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('after policy')),
      'post-policy reply',
    );
    const toolText = ctx
      .get(IAgentContextMemoryService)
      .get()
      .filter((message) => message.role === 'tool')
      .map((message) => JSON.stringify(message.content))
      .join('');
    // The policy-declared-inactive tool was never registered → rejected.
    expect(toolText).toContain('not found');
    expect(toolText).not.toContain('StubPolicyOff executed');
  });

  it('fires PostToolUse for native in-engine tools (TS notifyPostToolUse parity)', async () => {
    // TS fires PostToolUse for EVERY executed tool. Native tools (Bash/…)
    // execute inside the engine and bypass the external-tool callback, so
    // the runner fires the hook on the mirrored tool.result.
    const calls: Array<{ event: string; toolName?: string }> = [];
    const hookRunner = {
      trigger: async () => [],
      triggerBlock: async () => undefined,
      fireAndForgetTrigger: async (event: string, args?: { matcherValue?: string }) => {
        calls.push({ event, toolName: args?.matcherValue });
        return [];
      },
    };
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_native_bash',
          name: 'Bash',
          argumentsPart: '{"command":"echo native-hook"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'after native' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([permissionModeServices('auto'), externalHookServices(hookRunner)]);
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run native tool' }] });

    await waitForContext(
      ctx,
      (messages) =>
        messages.some((message) =>
          message.role === 'assistant' &&
          message.content
            .filter((part) => part.type === 'text')
            .map((part) => (part as { text?: string }).text ?? '')
            .join('')
            .includes('after native')),
      'post-native reply',
    );
    // PostToolUse is fire-and-forget: allow a beat for the async trigger.
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(calls).toContainEqual({ event: 'PostToolUse', toolName: 'Bash' });
  });

  it('publishes turn.step.completed with TS four-component usage on the bus (P0-1)', async () => {
    // The live transcript projector folds the bus event's usage per step —
    // it must be the TS TokenUsage shape (inputOther/output/inputCacheRead/
    // inputCacheCreation), not the engine's TranscriptUsage shape, or the
    // turn header usage degrades to NaN→null and the zod contract breaks.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        { type: 'text', delta: 'usage bus' },
        { type: 'usage', promptTokens: 10, completionTokens: 20 },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent();
    ctx.get(IAgentLoopService);
    const stepCompleted: Array<Record<string, unknown>> = [];
    ctx.get(IEventBus).subscribe((event) => {
      if ((event as { type?: string }).type === 'turn.step.completed') {
        stepCompleted.push(event as Record<string, unknown>);
      }
    });
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'usage on bus' }] });
    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.role === 'assistant'),
      'assistant message',
    );
    expect(stepCompleted.length).toBeGreaterThan(0);
    const usage = stepCompleted[0]!['usage'] as Record<string, unknown>;
    expect(usage['inputOther']).toBe(10);
    expect(usage['output']).toBe(20);
    expect(usage['inputCacheRead']).toBe(0);
    expect(usage['inputCacheCreation']).toBe(0);
    expect(usage['inputTokens']).toBeUndefined();
  });

  it('fires the Stop hook after a non-tool step (P1-3 parity)', async () => {
    const stops: string[] = [];
    const hookRunner = {
      trigger: async () => [],
      triggerBlock: async (event: string) => {
        if (event === 'Stop') stops.push('Stop');
        return undefined;
      },
      fireAndForgetTrigger: async () => [],
    };
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'text', delta: 'plain answer' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([externalHookServices(hookRunner)]);
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'stop hook' }] });
    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.role === 'assistant'),
      'assistant message',
    );
    // The hook trigger is fire-and-forget: allow a beat for the async call.
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(stops).toContain('Stop');
  });

  it('resets the Stop-hook continuation latch per turn (P1-3 focused review)', async () => {
    // TS resets `stopHookContinuationUsed` on every turn.ended — the Stop
    // hook must fire again on the NEXT turn even after a continuation was
    // used. The hook returns a continuation once (setting the latch), then
    // stays silent.
    const stops: string[] = [];
    let returned = false;
    const hookRunner = {
      trigger: async () => [],
      triggerBlock: async (
        event: string,
      ): Promise<{ block: true; reason: string } | undefined> => {
        if (event === 'Stop') {
          stops.push('Stop');
          if (!returned) {
            returned = true;
            return { block: true, reason: 'continue please' };
          }
        }
        return undefined;
      },
      fireAndForgetTrigger: async () => [],
    };
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [{ type: 'text', delta: 'first answer' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    ctx = createTestAgent([externalHookServices(hookRunner)]);
    ctx.get(IAgentLoopService);
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'first' }] });
    await waitForContext(
      ctx,
      (messages) => messages.some((message) => message.role === 'assistant'),
      'first assistant',
    );
    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'second' }] });
    await waitForContext(
      ctx,
      (messages) => messages.filter((message) => message.role === 'assistant').length >= 2,
      'second assistant',
    );
    await new Promise((resolve) => setTimeout(resolve, 100));
    // Turn 1 fired Stop (continuation returned → latch set); turn 2 must
    // fire it again — the latch was reset on turn.ended.
    expect(stops.length).toBeGreaterThanOrEqual(2);
  });
});
