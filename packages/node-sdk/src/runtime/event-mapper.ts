/**
 * Runtime → public SDK event translation (pure mapping layer).
 *
 * The runtime's per-agent `IEventBus` publishes `DomainEvent`s whose
 * payloads already match the public protocol
 * through `Agent.emitEvent` (the print runner renders them untranslated
 * for the same reason). What the bus does not carry is the
 * `sessionId` / `agentId` stamping: the bus is per-agent, so the engine-side
 * consumer knows both (kap-server's broadcaster stamps them the same way).
 * This module restores the stamping and reconciles the two streams' type
 * sets: internal-only types are dropped because the public `Event` union is
 * closed, and the one public fact the runtime publishes on the process-global
 * `IEventService` (`session.meta.updated`) is unwrapped from its
 * `{type, payload}` envelope.
 */
import type { Event } from '@dimi-agent/protocol';
import type { DomainEvent } from '@dimi-agent/agent-core-v2';

/**
 * DomainEvent types the public SDK event stream never carries:
 * - internal facts with no public protocol counterpart: `agent.activity.updated`
 *   (kap-server folds it into the `agent.status.updated` phase slice at the WS
 *   edge), `context.spliced`, `task.notified`, `plan.revision`, and the
 *   `permission.approval.*` pair (v1 surfaces approvals through the
 *   `requestApproval` callback, never as events).
 * - `prompt.*`: the daemon surface owns those lifecycle events; the in-process
 *   SDK client does not duplicate them from the agent bus.
 */
const DROPPED_DOMAIN_EVENT_TYPES: ReadonlySet<string> = new Set([
  'agent.activity.updated',
  'context.spliced',
  'task.notified',
  'plan.revision',
  'permission.approval.requested',
  'permission.approval.resolved',
  'prompt.submitted',
  'prompt.completed',
  'prompt.aborted',
  'prompt.steered',
]);

/**
 * Translate one agent-bus event into the public `Event` shape (payload plus the
 * `sessionId` / `agentId` stamping), or `undefined` when the type has no
 * place in that stream (see {@link DROPPED_DOMAIN_EVENT_TYPES}). The cast
 * only bridges the two packages' type declarations — every type not dropped
 * carries a payload that is field-identical with its protocol counterpart.
 */
export function translateDomainEvent(
  event: DomainEvent,
  sessionId: string,
  agentId: string,
): Event | undefined {
  if (DROPPED_DOMAIN_EVENT_TYPES.has(event.type)) return undefined;
  return { ...event, sessionId, agentId } as unknown as Event;
}

/**
 * Translate one process-global `IEventService` fact (`{type, payload}`
 * envelope) into the public `Event` shape. Only `session.meta.updated`
 * crosses; every other global-bus type belongs to the daemon/WS edge.
 */
export function translateGlobalEvent(event: {
  readonly type: string;
  readonly payload: unknown;
}): Event | undefined {
  if (event.type !== 'session.meta.updated' || typeof event.payload !== 'object') {
    return undefined;
  }
  return { type: event.type, ...event.payload } as unknown as Event;
}
