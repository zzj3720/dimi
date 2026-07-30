/**
 * `wait` domain (L4) — one durable active wait for an Agent.
 *
 * The wire Model is authoritative. Timers are live-only resources keyed by the
 * persisted wait id, so replacement and restart cannot deliver a stale wake.
 */

import { createDecorator } from '#/_base/di/instantiation';
import { z } from 'zod';

import { defineModel } from '#/wire/model';

export const WAIT_DEFAULT_SECONDS = 60;
export const WAIT_MAX_SECONDS = 1_800;

export type AgentWaitSource = 'wait_for' | 'auto_wait';
export type AgentWaitEndReason = 'notification' | 'message' | 'timeout';

export interface AgentWait {
  readonly waitId: string;
  readonly reason: string;
  readonly source: AgentWaitSource;
  readonly timeoutSeconds: number;
  readonly startedAt: number;
  readonly deadlineAt: number;
  readonly turnId?: number;
  readonly taskIds?: string[];
}

export interface AutoWaitTask {
  readonly taskId: string;
  readonly toolName: string;
  readonly autoWaitTimeoutSeconds: number;
}

export const WaitModel = defineModel<AgentWait | null>('wait', () => null);

const waitSchema = z.object({
  waitId: z.string().min(1),
  reason: z.string().min(1),
  source: z.enum(['wait_for', 'auto_wait']),
  timeoutSeconds: z.number().int().min(1).max(WAIT_MAX_SECONDS),
  startedAt: z.number().int().nonnegative(),
  deadlineAt: z.number().int().nonnegative(),
  turnId: z.number().int().nonnegative().optional(),
  taskIds: z.array(z.string().min(1)).optional(),
});

declare module '#/app/event/eventBus' {
  interface DomainEventMap {
    'wait.started': { readonly wait: AgentWait };
    'wait.terminated': {
      readonly wait: AgentWait;
      readonly reason: AgentWaitEndReason;
    };
  }
}

declare module '#/wire/types' {
  interface PersistedOpMap {
    'wait.started': typeof waitStarted;
    'wait.terminated': typeof waitTerminated;
  }
}

export const waitStarted = WaitModel.defineOp('wait.started', {
  schema: z.object({ wait: waitSchema }),
  apply: (_state, payload) => payload.wait,
  toEvent: (payload) => ({ type: 'wait.started' as const, wait: payload.wait }),
});

export const waitTerminated = WaitModel.defineOp('wait.terminated', {
  schema: z.object({
    wait: waitSchema,
    reason: z.enum(['notification', 'message', 'timeout']),
  }),
  apply: (state, payload) => (state?.waitId === payload.wait.waitId ? null : state),
  toEvent: (payload) => ({
    type: 'wait.terminated' as const,
    wait: payload.wait,
    reason: payload.reason,
  }),
});

export interface IAgentWaitService {
  readonly _serviceBrand: undefined;

  active(): AgentWait | null;
  start(reason: string, timeoutSeconds?: number): Promise<AgentWait>;
  startAutoWait(turnId: number, tasks: readonly AutoWaitTask[]): Promise<AgentWait | null>;
}

export const IAgentWaitService = createDecorator<IAgentWaitService>('agentWaitService');
