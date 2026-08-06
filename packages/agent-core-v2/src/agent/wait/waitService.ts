/** `wait` domain (L4) — durable wait lifecycle and stale-safe deadline. */

import { randomUUID } from 'node:crypto';

import { Disposable } from '#/_base/di/lifecycle';
import { ILogService } from '#/_base/log/log';
import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';
import { IEventBus } from '#/app/event/eventBus';
import { rustEngineEnabled } from '#/agent/loop/engineMode';
import { IAgentLoopService } from '#/agent/loop/loop';
import { MessageStepRequest } from '#/agent/loop/stepRequest';
import { IWireService } from '#/wire/wire';

import {
  IAgentWaitService,
  WAIT_DEFAULT_SECONDS,
  WAIT_MAX_SECONDS,
  WaitModel,
  type AgentWait,
  type AgentWaitEndReason,
  type AutoWaitTask,
  waitStarted,
  waitTerminated,
} from './wait';

export class AgentWaitService extends Disposable implements IAgentWaitService {
  declare readonly _serviceBrand: undefined;

  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    @IWireService private readonly wire: IWireService,
    @IEventBus private readonly eventBus: IEventBus,
    @IAgentLoopService private readonly loop: IAgentLoopService,
    @ILogService private readonly log: ILogService,
  ) {
    super();
    this._register(
      eventBus.subscribe('task.terminated', () => {
        this.runWake('notification');
      }),
    );
    this._register(
      eventBus.subscribe('context.spliced', (event) => {
        if (event.messages.some(isUserWakeMessage)) this.runWake('message');
      }),
    );
    this._register(
      this.wire.hooks.onDidRestore.register('wait', async (_context, next) => {
        const wait = this.active();
        if (wait !== null) {
          if (wait.deadlineAt <= Date.now()) await this.expire(wait.waitId);
          else this.arm(wait);
        }
        await next();
      }),
    );
  }

  active(): AgentWait | null {
    return this.wire.getModel(WaitModel) as AgentWait | null;
  }

  start(reason: string, timeoutSeconds = WAIT_DEFAULT_SECONDS): Promise<AgentWait> {
    const normalized = reason.trim();
    if (normalized.length === 0) throw new Error("'reason' is required");
    if (
      !Number.isInteger(timeoutSeconds) ||
      timeoutSeconds < 1 ||
      timeoutSeconds > WAIT_MAX_SECONDS
    ) {
      throw new Error(`'timeout_seconds' must be between 1 and ${String(WAIT_MAX_SECONDS)}`);
    }
    return this.begin({ reason: normalized, source: 'wait_for', timeoutSeconds });
  }

  startAutoWait(turnId: number, tasks: readonly AutoWaitTask[]): Promise<AgentWait | null> {
    if (tasks.length === 0) return Promise.resolve(null);
    const timeoutSeconds = Math.min(
      WAIT_MAX_SECONDS,
      Math.max(...tasks.map((task) => task.autoWaitTimeoutSeconds)),
    );
    const names = [...new Set(tasks.map((task) => task.toolName))].join(', ');
    return this.begin({
      reason: `waiting for background tool completion: ${names}`,
      source: 'auto_wait',
      timeoutSeconds,
      turnId,
      taskIds: tasks.map((task) => task.taskId),
    });
  }

  override dispose(): void {
    this.clearTimer();
    super.dispose();
  }

  private async begin(
    input: Omit<AgentWait, 'waitId' | 'startedAt' | 'deadlineAt'>,
  ): Promise<AgentWait> {
    const startedAt = Date.now();
    const wait: AgentWait = {
      ...input,
      waitId: `wait-${randomUUID()}`,
      startedAt,
      deadlineAt: startedAt + input.timeoutSeconds * 1_000,
    };
    this.wire.dispatch(waitStarted({ wait }));
    this.arm(wait);
    await this.wire.flush();
    return wait;
  }

  private runWake(reason: Exclude<AgentWaitEndReason, 'timeout'>): void {
    void this.end(reason).catch((error) => {
      this.log.error('wait termination failed', { error });
    });
  }

  private async end(reason: AgentWaitEndReason, waitId?: string): Promise<AgentWait | null> {
    const wait = this.active();
    if (wait === null || (waitId !== undefined && wait.waitId !== waitId)) return null;
    this.clearTimer();
    this.wire.dispatch(waitTerminated({ wait, reason }));
    await this.wire.flush();
    return wait;
  }

  private arm(wait: AgentWait): void {
    this.clearTimer();
    this.timer = setTimeout(
      () => {
        void this.expire(wait.waitId).catch((error) => {
          this.log.error('wait timeout delivery failed', { waitId: wait.waitId, error });
        });
      },
      Math.max(0, wait.deadlineAt - Date.now()),
    );
    this.timer.unref?.();
  }

  private clearTimer(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }

  private async expire(waitId: string): Promise<void> {
    const wait = await this.end('timeout', waitId);
    if (wait === null) return;
    const content = JSON.stringify({
      type: 'wait_expired',
      wait_id: wait.waitId,
      reason: wait.reason,
      timeout_seconds: wait.timeoutSeconds,
      message: 'wait timeout reached; inspect current state before deciding what to do next',
    });
    if (rustEngineEnabled()) {
      this.eventBus.publish({ type: 'wait.timeout', wait, content });
      return;
    }
    this.loop.enqueue(
      new MessageStepRequest(
        {
          role: 'user',
          content: [{ type: 'text', text: content }],
          toolCalls: [],
          origin: { kind: 'system_trigger', name: 'wait_timeout' },
        },
        {
          kind: 'wait_timeout',
          mergeable: true,
          turnScoped: false,
          admission: 'activeOrNewTurn',
        },
      ),
    );
  }
}

function isUserWakeMessage(message: {
  readonly role: string;
  readonly origin?: { readonly kind: string };
}): boolean {
  return message.role === 'user' && message.origin?.kind !== 'injection';
}

registerScopedService(
  LifecycleScope.Agent,
  IAgentWaitService,
  AgentWaitService,
  ScopeActivation.OnScopeCreated,
  'wait',
);
