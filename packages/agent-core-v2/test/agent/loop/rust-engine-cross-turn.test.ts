/**
 * Same-turn subagent task resolution (code-review P1-1 regression). The
 * bridge shares ONE process-level `AgentTasks` registry across every turn
 * session. The Agent tool no longer stops the caller's turn (TS parity —
 * `stop_turn: false`): the launch returns immediately with the agent_id and
 * the SAME turn can keep calling AgentOutput / WaitFor against the shared
 * registry. This suite drives the REAL `RustTurnSession` (napi binding) with
 * `DIMI_RUST_ENGINE_SCRIPTED` scripted LLM segments, so a single turn can
 * spawn a subagent and then query it.
 *
 * The subagent's nested turn reuses the parent's scripted client (shared
 * cursor), so it consumes whatever segments the main turn leaves over. The
 * launch registers the task synchronously, so AgentOutput resolves it
 * immediately regardless of the worker's progress.
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

  it('resolves a subagent launched in the same turn via AgentOutput (shared task registry)', async () => {
    // Agent no longer ends the caller's turn: Seg 0 spawns the subagent and
    // the turn KEEPS GOING; Seg 1 calls AgentOutput with the id the engine
    // just assigned; Seg 2 blocks in Bash so the subagent's completion
    // steers into the RUNNING turn (no idle notification turn); Seg 3 is a
    // text reply that ends the turn. On a fresh agent the first subagent is
    // `agent-0` — the runner seeds the engine's id counter from
    // `computeNextAgentId` (no persisted agents), so the scripted AgentOutput
    // segment can address it by that id.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      // Seg 0: spawn the subagent (stop_turn: false — the turn continues).
      [
        {
          type: 'tool_call',
          toolCallId: 'call_spawn',
          name: 'Agent',
          argumentsPart: '{"prompt":"cross-turn work","description":"cross-turn sub"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 1: the SAME turn queries the just-launched task.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_output',
          name: 'AgentOutput',
          argumentsPart: '{"agent_id":"agent-0"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 2: block so the subagent's completion folds into this running
      // turn instead of launching an idle notification turn.
      [
        {
          type: 'tool_call',
          toolCallId: 'call_hold',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 2"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      // Seg 3: the turn finishes with a text reply.
      [{ type: 'text', delta: 'checked subagent' }, { type: 'finish', finishReason: 'stop' }],
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

    const first = await ctx.rpc.prompt({ input: [{ type: 'text', text: 'spawn subagent' }] });
    expect(first).toEqual({ turn_id: 0 });
    await waitFor(() => spawned.length >= 1, 'subagent.spawned');
    const agentId = spawned[0]!.subagentId!;
    expect(agentId).toMatch(/^agent-\d+$/);
    // The first subagent on a fresh agent is agent-0 (the scripted
    // AgentOutput segment addresses it by that id).
    expect(agentId).toBe('agent-0');
    // The Agent tool result landed (launch returned agent_id/task_id).
    await waitFor(
      () => toolResults.some((result) => result.toolCallId === 'call_spawn'),
      'Agent tool result',
    );

    // The same-turn AgentOutput result: task status JSON, NOT the
    // "No subagent found" error a missing/table-scoped lookup produces.
    await waitFor(
      () => toolResults.some((result) => result.toolCallId === 'call_output'),
      'same-turn AgentOutput result',
    );
    const output = toolResults.find((result) => result.toolCallId === 'call_output')!;
    expect(output.isError).toBe(false);
    expect(output.output).toContain('"agent_id"');
    expect(output.output).toContain('"status"');
    expect(output.output).not.toContain('No subagent found');
    // The resolved task is the subagent the launch just registered; its
    // status is the task state the shared registry recorded (running or
    // completed — the point is that the lookup resolves at all).
    const parsed = JSON.parse(output.output ?? '{}') as {
      agent_id?: string;
      status?: string;
    };
    expect(parsed.agent_id).toBe(agentId);
    expect(parsed.status).toBeTruthy();
  }, 30_000);
});
