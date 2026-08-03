/**
 * Cross-turn subagent task resolution (code-review P1-1). The bridge creates
 * a fresh `RustTurnSession` per turn, and each session used to own a FRESH
 * `AgentTasks` registry. Because the Agent tool hardcodes `stop_turn: true`,
 * the launching turn ends immediately — the subagent task is registered in
 * the LAUNCHING session's table — so the NEXT turn (a new session with an
 * empty table) could not resolve it: AgentOutput / WaitFor returned
 * "No subagent found". The bridge now shares ONE process-level registry
 * across every session, so a later turn reads the earlier turn's tasks.
 *
 * Like `rust-engine-coverage.test.ts`, this suite drives the REAL
 * `RustTurnSession` (napi binding) with `DIMI_RUST_ENGINE_SCRIPTED` scripted
 * LLM segments. Each turn's session replays the scripted segments from
 * cursor 0, so the second turn's script is set AFTER the first turn launched
 * (the env var is read per turn in `rustEngineTurnRunner.runTurnNow`).
 *
 * NOTE: the subagent's nested turn blocks in Bash (`sleep 6`) so it is still
 * RUNNING when turn 1 queries it — the cross-turn lookup resolves a task the
 * launching session registered, exactly the P1-1 scenario. The idle-path
 * notification turn that launches after the settle (~6s) replays turn 1's
 * AgentOutput segment against the now-completed task; it lands after this
 * test's assertions, so it cannot interfere.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { IAgentLoopService } from '#/agent/loop/loop';
import { IEventBus } from '#/app/event/eventBus';
import {
  createTestAgent,
  permissionModeServices,
  type TestAgentContext,
} from '../../harness';

const DIMI_LEGACY = 'DIMI_LEGACY';
const RUST_ENGINE_SCRIPTED = 'DIMI_RUST_ENGINE_SCRIPTED';

/** Poll an arbitrary predicate (bus events / wire model). */
async function waitFor(predicate: () => boolean, what: string, timeoutMs = 6_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timeout waiting for ${what}`);
}

describe('Rust engine cross-turn agent tasks', () => {
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

  it('resolves a subagent launched in a previous turn via AgentOutput (shared task registry)', async () => {
    // Turn 0: the model calls Agent (stop_turn ends the turn immediately);
    // the subagent's nested turn blocks in Bash (`sleep 6`) so it is still
    // running when turn 1 queries it — the launch was registered in turn 0's
    // session table. Turn 1 (a NEW session) calls AgentOutput with turn 0's
    // agent_id: with a process-level shared registry it returns the task
    // status JSON; with per-session tables it fails with "No subagent found".
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0 (turn 0's main turn): spawn the subagent.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_spawn',
          name: 'Agent',
          argumentsPart: '{"prompt":"cross-turn work","description":"cross-turn sub"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1 (the subagent's nested turn): block so the task stays running
      // across the turn boundary.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_nested_sleep',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 6"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2 (the subagent's final answer).
      [
        { type: 'text', delta: 'cross-turn subagent output' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);

    const toolResults: Array<{
      toolCallId?: string;
      name?: string;
      output?: string;
      isError?: boolean;
    }> = [];
    const spawned: Array<{ subagentId?: string }> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'tool.result') toolResults.push(event as typeof toolResults[number]);
      if (type === 'subagent.spawned') spawned.push(event as { subagentId?: string });
    });

    // Turn 0: launch the subagent; the turn ends immediately (stop_turn).
    const first = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn subagent' }] });
    expect(first).toEqual({ turn_id: 0 });
    await waitFor(() => spawned.length === 1, 'subagent.spawned');
    const agentId = spawned[0]!.subagentId!;
    expect(agentId).toMatch(/^agent-\d+$/);
    // Let turn 0 finish teardown (its Agent tool result landed) before the
    // next prompt, so turn 1 launches instead of queueing.
    await waitFor(
      () => toolResults.some((result) => result.toolCallId === 'call_spawn'),
      'turn 0 Agent tool result',
    );

    // Turn 1 (a NEW RustTurnSession): the model calls AgentOutput with the
    // agent_id from turn 0. The per-turn script is read at session creation,
    // so swap the env for this turn.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_output',
          name: 'AgentOutput',
          argumentsPart: JSON.stringify({ agent_id: agentId }),
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [{ type: 'text', delta: 'checked subagent' }, { type: 'finish', finishReason: 'stop' }],
    ]);
    const second = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'check subagent' }] });
    expect(second === undefined || second.turn_id === 1).toBe(true);

    // The cross-turn AgentOutput result: task status JSON, NOT the
    // "No subagent found" error the per-session tables produced.
    await waitFor(
      () => toolResults.some((result) => result.toolCallId === 'call_output'),
      'cross-turn AgentOutput result',
    );
    const output = toolResults.find((result) => result.toolCallId === 'call_output')!;
    expect(output.isError).toBe(false);
    expect(output.output).toContain('"agent_id"');
    expect(output.output).toContain('"status"');
    expect(output.output).not.toContain('No subagent found');
    // The resolved task is turn 0's subagent; its status is the task state
    // the launching session recorded (running or completed — the point is
    // that the cross-turn lookup resolves at all).
    const parsed = JSON.parse(output.output ?? '{}') as {
      agent_id?: string;
      status?: string;
    };
    expect(parsed.agent_id).toBe(agentId);
    expect(parsed.status).toBeTruthy();
  }, 30_000);
});
