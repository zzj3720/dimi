/**
 * M3 slice-1 differential: the TS loop's engine events vs the Rust engine's
 * event stream, on the same scripted LLM input.
 *
 * The TS side runs the real loopService with the scripted mock provider; the
 * Rust side runs `RustEngine` with equivalent scripted segments. The
 * assertion is on the *engine events* (the shapes `coreEventMap` projects
 * into transcript ops): turn.started / thinking.delta / assistant.delta /
 * tool.call.* / turn.step.completed / turn.ended. Wire/context/activity
 * events are TS-side plumbing and out of scope.
 */
// The TS side of the differential drives the real TS loop with the scripted
// mock provider (mockNextResponse); the Rust engine is the default runtime
// (`--legacy` sets DIMI_LEGACY=1), so pin legacy mode for the TS side.
process.env["DIMI_LEGACY"] = "1";

import { describe, expect, it } from 'vitest';

import { RustEngine, RustTurnSession } from '@dimi-agent/dimi-native';
import { IAgentToolRegistryService } from '#/agent/toolRegistry/toolRegistry';
import { IAgentProfileService } from '#/index';
import { IEventBus } from '#/app/event/eventBus';
import {
  createTestAgent,
  permissionModeServices,
  type TestAgentContext,
} from '../../harness';

interface EngineEventLike {
  type: string;
  [key: string]: unknown;
}

/** Collect the TS loop's engine events (the projection-consumed subset). */
async function collectTsEngineEvents(
  ctx: TestAgentContext,
  run: () => Promise<void>,
): Promise<EngineEventLike[]> {
  const bus = ctx.get(IEventBus);
  const events: EngineEventLike[] = [];
  const disposable = bus.subscribe((event) => {
    const type = (event as { type?: string }).type;
    if (
      type === 'turn.started' ||
      type === 'turn.ended' ||
      type === 'turn.step.started' ||
      type === 'turn.step.completed' ||
      type === 'assistant.delta' ||
      type === 'thinking.delta' ||
      type === 'tool.call.delta' ||
      type === 'tool.call.started' ||
      type === 'tool.progress' ||
      type === 'tool.result'
    ) {
      events.push(event as EngineEventLike);
    }
  });
  try {
    await run();
    await ctx.untilTurnEnd();
  } finally {
    disposable.dispose();
  }
  return events;
}

/** Run the Rust engine with the equivalent scripted LLM segments. */
async function runRustEngine(
  prompt: string,
  segments: Array<Array<Record<string, unknown>>>,
  turnId = 0,
): Promise<{ events: EngineEventLike[]; outcome: { status: string } }> {
  // Auto policy: tools execute without approval (the differential is about
  // orchestration; approval flows are covered by the runner tests).
  const session = new RustTurnSession(
    JSON.stringify({
      turnId,
      messages: [{ role: 'user', content: prompt }],
      tools: [],
      provider: { baseUrl: 'http://example.test/v1', apiKey: 'test-key', model: 'mock-model' },
      maxStepsPerTurn: null,
      cwd: '/tmp',
      shell: '/bin/sh',
    }),
    JSON.stringify({ mode: 'auto', rules: [], sessionApprovedPatterns: [] }),
    JSON.stringify(segments),
    'test-registry',
  );
  // The engine streams every event through the per-event callback as it is
  // emitted; the response carries only the progress.
  const events: EngineEventLike[] = [];
  session.setOnEvent((eventJson: string) => {
    events.push(JSON.parse(eventJson) as EngineEventLike);
  });
  const batch = JSON.parse(await session.run()) as {
    events: EngineEventLike[];
    progress: { status: string; outcome?: { status: string } };
  };
  return { events, outcome: { status: batch.progress.outcome?.status ?? batch.progress.status } };
}

/** The projection-relevant fields of an engine event. */
function projectFields(events: EngineEventLike[]): EngineEventLike[] {
  return events.map((event) => {
    const out: EngineEventLike = { type: event.type };
    switch (event.type) {
      case 'turn.started':
        return { type: 'turn.started', turnId: event['turnId'], prompt: event['prompt'] };
      case 'turn.ended':
        return { type: 'turn.ended', turnId: event['turnId'], reason: event['reason'] };
      case 'turn.step.completed':
        return {
          type: 'turn.step.completed',
          turnId: event['turnId'],
          step: event['step'],
          finishReason: event['finishReason'],
        };
      case 'assistant.delta':
      case 'thinking.delta':
        return { type: event.type, delta: event['delta'] };
      case 'tool.result':
        // The tool executors differ (TS fake vs Rust Bash); the differential
        // is about orchestration, so normalize the output payload.
        return {
          type: 'tool.result',
          toolCallId: event['toolCallId'],
          output: '<tool-output>',
          isError: event['isError'] === true,
        };
      default:
        return out;
    }
  });
}

describe('M3 engine differential: TS loop vs Rust engine', () => {
  it('text-only turn produces the same engine events', async () => {
    const ctx = createTestAgent();
    try {
      ctx.mockNextResponse({ type: 'think', think: '<think-1>' }, { type: 'text', text: '<text-1>' });
      const tsEvents = await collectTsEngineEvents(ctx, async () => {
        await ctx.rpc.prompt({ input: [{ type: 'text', text: 'Hello' }] });
      });

      const rust = await runRustEngine('Hello', [
        [
          { type: 'thinking', delta: '<think-1>' },
          { type: 'text', delta: '<text-1>' },
          { type: 'finish', finishReason: 'stop' },
        ],
      ]);

      const tsProjected = projectFields(tsEvents);
      const rustProjected = projectFields(rust.events);
      // The TS loop emits turn.step.started with a generated stepId; the
      // Rust engine uses the step number. Compare the projection-relevant
      // sequence, filtering turn.step.started out of both.
      // tool.progress is executor-specific (Bash streams stdout; the fake
      // Lookup emits none) — not part of the orchestration contract.
      const filter = (events: EngineEventLike[]): EngineEventLike[] =>
        events.filter(
          (event) => event.type !== 'turn.step.started' && event.type !== 'tool.progress',
        );
      expect(filter(rustProjected)).toEqual(filter(tsProjected));
      expect(rust.outcome.status).toBe('completed');
    } finally {
      await ctx.dispose();
    }
  });

  it('tool-call turn produces the same engine events', async () => {
    const ctx = createTestAgent([permissionModeServices('auto')]);
    try {
      ctx.get(IAgentProfileService).update({ activeToolNames: ['Lookup'] });
      ctx.get(IAgentToolRegistryService).register({
        name: 'Lookup',
        description: 'Look up a short test value.',
        parameters: {
          type: 'object',
          properties: { query: { type: 'string' } },
          required: ['query'],
          additionalProperties: false,
        },
        resolveExecution: () => ({
          approvalRule: 'Lookup',
          execute: async () => ({ output: 'lookup-result' }),
        }),
      });
      ctx.mockNextResponse(
        { type: 'text', text: 'I will look it up.' },
        { type: 'function', id: 'call_lookup', name: 'Lookup', arguments: '{"query":"moon"}' },
      );
      const tsEvents = await collectTsEngineEvents(ctx, async () => {
        await ctx.rpc.prompt({ input: [{ type: 'text', text: 'run it' }] });
        // The second LLM response is queued after the prompt resolves (the
        // tool executes between the two rounds).
        ctx.mockNextResponse({ type: 'text', text: 'The result is lookup-result.' });
      });

      const rust = await runRustEngine('run it', [
        [
          { type: 'text', delta: 'I will look it up.' },
          {
            type: 'tool_call',
            toolCallId: 'call_lookup',
            name: 'Bash',
            argumentsPart: '{"command":"echo diff-ok"}',
          },
          { type: 'finish', finishReason: 'tool_calls' },
        ],
        [
          { type: 'text', delta: 'The result is lookup-result.' },
          { type: 'finish', finishReason: 'stop' },
        ],
      ]);

      const tsProjected = projectFields(tsEvents);
      const rustProjected = projectFields(rust.events);
      // tool.progress is executor-specific (Bash streams stdout; the fake
      // Lookup emits none) — not part of the orchestration contract.
      const filter = (events: EngineEventLike[]): EngineEventLike[] =>
        events.filter(
          (event) => event.type !== 'turn.step.started' && event.type !== 'tool.progress',
        );
      expect(filter(rustProjected)).toEqual(filter(tsProjected));
      expect(rust.outcome.status).toBe('completed');
    } finally {
      await ctx.dispose();
    }
  });
});
