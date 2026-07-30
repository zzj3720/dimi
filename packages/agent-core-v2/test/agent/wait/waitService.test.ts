/** Scenario: durable generic waits wake on notifications, messages, deadlines, and restore. */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { SyncDescriptor } from '#/_base/di/descriptors';
import { DisposableStore } from '#/_base/di/lifecycle';
import { ILogService } from '#/_base/log/log';
import { TestInstantiationService } from '#/_base/di/test';
import { IEventBus } from '#/app/event/eventBus';
import { EventBusService } from '#/app/event/eventBusService';
import { IAgentLoopService } from '#/agent/loop/loop';
import { IAgentWaitService, WaitModel, waitStarted, type AgentWait } from '#/agent/wait/wait';
import { AgentWaitService } from '#/agent/wait/waitService';
import { createHooks } from '#/hooks';
import type { Op } from '#/wire/op';
import type { DeepReadonly, ModelDef } from '#/wire/model';
import { IWireService, type WireHooks } from '#/wire/wire';

import { stubLog } from '../../_base/log/stubs';
import { stubLoopWithHooks, type StubLoop } from '../loop/stubs';

describe('AgentWaitService', () => {
  let disposables: DisposableStore;
  let eventBus: EventBusService;
  let loop: StubLoop;
  let wire: TestWire;
  let waits: IAgentWaitService;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-07-30T00:00:00Z'));
    disposables = new DisposableStore();
    eventBus = disposables.add(new EventBusService());
    loop = stubLoopWithHooks();
    wire = new TestWire(eventBus);
    const ix = disposables.add(new TestInstantiationService());
    ix.set(ILogService, stubLog());
    ix.set(IEventBus, eventBus);
    ix.set(IAgentLoopService, loop);
    ix.set(IWireService, wire);
    ix.set(IAgentWaitService, new SyncDescriptor(AgentWaitService));
    waits = ix.get(IAgentWaitService);
  });

  afterEach(() => {
    disposables.dispose();
    vi.useRealTimers();
  });

  it('replaces the active wait and ignores the stale deadline', async () => {
    const first = await waits.start('first', 1);
    const second = await waits.start('second', 2);

    expect(waits.active()?.waitId).toBe(second.waitId);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(waits.active()?.waitId).toBe(second.waitId);
    expect(loop.queue.hasPendingRequests()).toBe(false);

    await vi.advanceTimersByTimeAsync(1_000);
    expect(waits.active()).toBeNull();
    expect(loop.queue.hasPendingRequests()).toBe(true);
    expect(first.waitId).not.toBe(second.waitId);
  });

  it('ends on a task notification or ordinary user message without a timeout message', async () => {
    await waits.start('task', 60);
    eventBus.publish({
      type: 'task.terminated',
      info: {
        kind: 'tool',
        taskId: 'tool-1',
        description: 'tool',
        status: 'completed',
        detached: true,
        startedAt: Date.now(),
        endedAt: Date.now(),
        turnId: 1,
        toolCallId: 'call-1',
        toolName: 'Test',
        autoWaitTimeoutSeconds: 20,
      },
    });
    await vi.waitFor(() => {
      expect(waits.active()).toBeNull();
    });
    expect(loop.queue.hasPendingRequests()).toBe(false);

    await waits.start('message', 60);
    eventBus.publish({
      type: 'context.spliced',
      start: 0,
      deleteCount: 0,
      messages: [{ role: 'user', content: [], toolCalls: [], origin: { kind: 'user' } }],
    });
    await vi.waitFor(() => {
      expect(waits.active()).toBeNull();
    });
    expect(loop.queue.hasPendingRequests()).toBe(false);
  });

  it('records one auto wait for a detached batch and timeout does not touch tasks', async () => {
    const wait = await waits.startAutoWait(9, [
      { taskId: 'tool-a', toolName: 'A', autoWaitTimeoutSeconds: 20 },
      { taskId: 'tool-b', toolName: 'B', autoWaitTimeoutSeconds: 12 },
    ]);

    expect(wait).toMatchObject({
      source: 'auto_wait',
      timeoutSeconds: 20,
      turnId: 9,
      taskIds: ['tool-a', 'tool-b'],
    });
    await vi.advanceTimersByTimeAsync(20_000);

    const messages: unknown[] = [];
    loop.drainNextBatch({ append: (...incoming) => messages.push(...incoming) });
    expect(messages).toEqual([
      expect.objectContaining({
        origin: { kind: 'system_trigger', name: 'wait_timeout' },
        content: [expect.objectContaining({ text: expect.stringContaining('wait_expired') })],
      }),
    ]);
    expect(wire.ops.map((op) => op.type)).toEqual(['wait.started', 'wait.terminated']);
  });

  it('re-arms a restored wait using its persisted id', async () => {
    const restored: AgentWait = {
      waitId: 'wait-restored',
      reason: 'restore',
      source: 'wait_for',
      timeoutSeconds: 3,
      startedAt: Date.now(),
      deadlineAt: Date.now() + 3_000,
    };
    wire.dispatch(waitStarted({ wait: restored }));

    await wire.hooks.onDidRestore.run({});
    await vi.advanceTimersByTimeAsync(3_000);

    expect(waits.active()).toBeNull();
    expect(loop.queue.hasPendingRequests()).toBe(true);
  });
});

class TestWire implements IWireService {
  declare readonly _serviceBrand: undefined;
  readonly hooks = createHooks<WireHooks, keyof WireHooks>(['onDidRestore']);
  readonly ops: Op[] = [];
  private readonly states = new Map<ModelDef<unknown>, unknown>();

  constructor(private readonly events: IEventBus) {}

  dispatch(...ops: Op[]): void {
    for (const op of ops) {
      this.ops.push(op);
      const model = op.descriptor.model as ModelDef<unknown>;
      const current = this.states.has(model) ? this.states.get(model) : model.initial();
      const next = op.descriptor.apply(current, op.payload);
      this.states.set(model, next);
      const event = op.descriptor.toEvent?.(op.payload, next);
      if (event !== undefined) this.events.publish(event as never);
    }
  }

  seal(): Promise<void> {
    return Promise.resolve();
  }
  restore(): Promise<void> {
    return Promise.resolve();
  }
  flush(): Promise<void> {
    return Promise.resolve();
  }

  getModel<S>(model: ModelDef<S>): DeepReadonly<S> {
    if (!this.states.has(model as ModelDef<unknown>)) {
      this.states.set(model as ModelDef<unknown>, model.initial());
    }
    return this.states.get(model as ModelDef<unknown>) as DeepReadonly<S>;
  }
}
