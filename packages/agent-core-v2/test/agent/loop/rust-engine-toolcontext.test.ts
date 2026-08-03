/**
 * Slice-2: the Rust engine path must carry the same-batch tool-call context
 * into external (TS-side) tool callbacks (`ToolResolutionContext.toolCalls`),
 * so AllDone's mixed-use / background-task guards really take effect.
 *
 * Before the fix the runner's `registerExternalTool` callback called
 * `tool.resolveExecution(payload.arguments)` with NO context. AllDone's guard
 * is `context?.toolCalls.length !== 1 || context.toolCalls[0]?.name !==
 * this.name` — with `context === undefined` that is always true, so AllDone
 * rejected EVERY call with "AllDone must be the only tool call in its round."
 * and the success / background-rejection paths were unreachable on the Rust
 * path (the TS loop already passes `{ toolCalls }` via toolExecutorService).
 *
 * Like `rust-engine-coverage.test.ts`, this suite drives the REAL
 * `RustTurnSession` (napi binding) with `DIMI_RUST_ENGINE_SCRIPTED` scripted
 * LLM segments. AllDone is an external tool on this path: the profile's
 * `profileName` gates its `when` contribution, and the runner registers every
 * non-native TS tool through `registerExternalTool`.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { IAgentLoopService } from '#/agent/loop/loop';
import { IAgentProfileService } from '#/agent/profile/profile';
import { IAgentToolActivationService } from '#/agent/toolActivation/toolActivation';
import { IEventBus } from '#/app/event/eventBus';
import {
  createTestAgent,
  permissionModeServices,
  type TestAgentContext,
} from '../../harness';

const DIMI_LEGACY = 'DIMI_LEGACY';
const RUST_ENGINE_SCRIPTED = 'DIMI_RUST_ENGINE_SCRIPTED';

/** Poll an arbitrary predicate (bus events / wire model). */
async function waitFor(
  predicate: () => boolean,
  what: string,
  timeoutMs = 10_000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timeout waiting for ${what}`);
}

interface ToolResultEvent {
  type?: string;
  toolCallId?: string;
  output?: string;
  isError?: boolean;
}

describe('Rust engine external tool callbacks carry the same-batch context', () => {
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

  /** Collect `tool.result` bus events for the current agent. */
  function collectToolResults(): ToolResultEvent[] {
    const results: ToolResultEvent[] = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'tool.result') results.push(event as ToolResultEvent);
    });
    return results;
  }

  /** Activate AllDone as an external tool (its `when` requires a profileName). */
  async function bindAllDone(): Promise<void> {
    ctx.get(IAgentProfileService).update({ profileName: 'agent' });
    await ctx.get(IAgentToolActivationService).activate();
  }

  it('rejects AllDone mixed with another tool in the same batch, but runs the sibling', async () => {
    // The assistant calls AllDone + Bash in ONE message. AllDone's resolution
    // context must contain BOTH calls (`toolCalls` length 2 → reject), while
    // the sibling Bash executes normally. This is a regression guard: the
    // buggy no-context path also rejects AllDone, so the distinguishing
    // assertions live in the next two tests.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_done_mixed',
          name: 'AllDone',
          argumentsPart: '{}',
        },
        {
          type: 'tool_call',
          toolCallId: 'call_bash_mixed',
          name: 'Bash',
          argumentsPart: '{"command":"echo mixed-ok"}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'probe done' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService); // agent scope must be live
    await bindAllDone();
    const results = collectToolResults();

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'Finish after probing' }] });

    await waitFor(
      () => results.some((result) => result.toolCallId === 'call_done_mixed'),
      'AllDone mixed-batch result',
    );
    const done = results.find((result) => result.toolCallId === 'call_done_mixed')!;
    expect(done.isError).toBe(true);
    expect(done.output).toContain('AllDone must be the only tool call in its round.');
    await waitFor(
      () => results.some((result) => result.toolCallId === 'call_bash_mixed'),
      'Bash sibling result',
    );
    const bash = results.find((result) => result.toolCallId === 'call_bash_mixed')!;
    expect(bash.isError).toBe(false);
    expect(bash.output).toContain('mixed-ok');
  }, 30_000);

  it('accepts AllDone as the only tool call in its round and ends the turn', async () => {
    // Single AllDone, no background tasks: the resolution context must carry
    // exactly ONE call named AllDone, so the execution resolves to the
    // stopTurn completion. Before the fix the missing context rejected this
    // with the mixed-use error.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_done_only',
          name: 'AllDone',
          argumentsPart: '{}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);
    await bindAllDone();
    const results = collectToolResults();
    const ended: Array<{ reason?: string }> = [];
    ctx.get(IEventBus).subscribe((event) => {
      const type = (event as { type?: string }).type;
      if (type === 'turn.ended') ended.push(event as { reason?: string });
    });

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'wrap up' }] });

    await waitFor(
      () => results.some((result) => result.toolCallId === 'call_done_only'),
      'AllDone-only result',
    );
    const done = results.find((result) => result.toolCallId === 'call_done_only')!;
    expect(done.isError).toBe(false);
    expect(done.output).toContain('All work is complete.');
    // `stopTurn: true` ends the turn right after the tool result.
    await waitFor(
      () => ended.some((end) => end.reason === 'completed'),
      'turn.ended completed after AllDone',
    );
  }, 30_000);

  it('rejects AllDone while a background task is active', async () => {
    // A foreground Bash call with timeout 1 is moved to the background (the
    // engine registers it in the shared task registry; the runner mirrors it
    // into the TS task service). AllDone then resolves with a single-call
    // context (passes the mixed-use guard) but `tasks.list(true)` is
    // non-empty → rejected. Before the fix the missing context rejected it
    // with the WRONG (mixed-use) message.
    process.env[RUST_ENGINE_SCRIPTED] = JSON.stringify([
      [
        {
          type: 'tool_call',
          toolCallId: 'call_bash_bg',
          name: 'Bash',
          argumentsPart: '{"command":"sleep 30","timeout":1}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        {
          type: 'tool_call',
          toolCallId: 'call_done_bg',
          name: 'AllDone',
          argumentsPart: '{}',
        },
        { type: 'finish', finishReason: 'tool_calls' },
      ],
      [
        { type: 'text', delta: 'checked' },
        { type: 'finish', finishReason: 'stop' },
      ],
    ]);
    ctx = createTestAgent([permissionModeServices('auto')]);
    ctx.get(IAgentLoopService);
    await bindAllDone();
    const results = collectToolResults();

    await ctx.rpc.prompt({ input: [{ type: 'text', text: 'background then done' }] });

    await waitFor(
      () => results.some((result) => result.toolCallId === 'call_done_bg'),
      'AllDone with active background task result',
      15_000,
    );
    const done = results.find((result) => result.toolCallId === 'call_done_bg')!;
    expect(done.isError).toBe(true);
    expect(done.output).toContain(
      'AllDone cannot complete while background tasks are active',
    );
    expect(done.output).toContain('bash-');
  }, 30_000);
});
