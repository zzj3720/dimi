/**
 * TranscriptStore — the session-level root.
 *
 * Owns one AgentTranscript per agent, created lazily. Per-agent granularity
 * subscriptions are a transport (L3) concern and deliberately absent here;
 * this layer only guarantees that an agent's transcript exists on demand and
 * that roster changes are observable (so the server can fan out, and clients
 * can render an agent picker).
 */

import type { AgentId, AttachmentId, InteractionId, PromptId, TaskId, TodoId, TurnId } from '../model/ids';
import type { TranscriptAttachment } from '../model/attachment';
import type { TranscriptInteraction } from '../model/interaction';
import type { TranscriptItem } from '../model/item';
import type { TranscriptMeta } from '../model/meta';
import type { TranscriptPrompt } from '../model/prompt';
import type { TranscriptTask } from '../model/task';
import type { TranscriptTodo } from '../model/todo';
import type { TranscriptTurn } from '../model/turn';
import type {
  AgentTranscriptSnapshot,
  AppliedOps,
  TranscriptOperation,
} from '../ops/operation';
import {
  AgentTranscript,
  type Disposable,
  type TranscriptListener,
} from './agentTranscript';

export interface AgentDescriptor {
  readonly agentId: AgentId;
  /** Engine metadata, mirrored for display (e.g. 'main' | 'sub' | swarm member). */
  readonly type?: 'main' | 'sub' | 'independent';
  readonly parentAgentId?: AgentId;
  readonly label?: string;
  readonly createdAt?: string;
  readonly disposedAt?: string;
}

export type RosterListener = (agents: readonly AgentDescriptor[]) => void;

/**
 * The per-agent transcript surface the server consumes — `AgentTranscript`
 * plus the swap-in socket for the Rust store adapter (dimi-store). The
 * native adapter derives the read methods from `snapshot()`, so this is the
 * structural contract both implementations satisfy.
 */
export interface AgentTranscriptLike {
  readonly agentId: AgentId;
  /** Full load == applying a reset (same as `apply`). */
  receive(ops: readonly TranscriptOperation[]): AppliedOps;
  /** The single convergence path; returns accepted ops + gap signal. */
  apply(ops: readonly TranscriptOperation[]): AppliedOps;
  onChange(listener: TranscriptListener): Disposable;
  getItems(): readonly TranscriptItem[];
  getTurn(turnId: TurnId): TranscriptTurn | undefined;
  getTasks(): ReadonlyMap<TaskId, TranscriptTask>;
  getTask(taskId: TaskId): TranscriptTask | undefined;
  getInteractions(): ReadonlyMap<InteractionId, TranscriptInteraction>;
  getInteraction(interactionId: InteractionId): TranscriptInteraction | undefined;
  getAttachments(): ReadonlyMap<AttachmentId, TranscriptAttachment>;
  getAttachment(attachmentId: AttachmentId): TranscriptAttachment | undefined;
  getTodos(): ReadonlyMap<TodoId, TranscriptTodo>;
  getTodo(todoId: TodoId): TranscriptTodo | undefined;
  getPrompts(): ReadonlyMap<PromptId, TranscriptPrompt>;
  getPrompt(promptId: PromptId): TranscriptPrompt | undefined;
  getMeta(): TranscriptMeta;
  listPendingInteractions(): readonly InteractionId[];
  get hasMoreOlder(): boolean;
  /** Materialize current state (optionally windowed to the newest turns). */
  snapshot(window?: { tailTurns: number }): AgentTranscriptSnapshot;
}

/**
 * The session-level store surface the server consumes — `TranscriptStore`
 * plus the swap-in socket for the Rust store adapter. Structural typing
 * (TS's `#private` fields make the concrete classes mutually incompatible,
 * so server code must type against this interface, not the class).
 */
export interface TranscriptStoreLike {
  readonly sessionId: string;
  /** Lazily create (or fetch) the transcript for an agent. */
  ensureAgent(agentId: AgentId, descriptor?: AgentDescriptor): AgentTranscriptLike;
  getAgent(agentId: AgentId): AgentTranscriptLike | undefined;
  /** Drop an agent entirely (disposed sub-agent, swarm member cleaned up). */
  removeAgent(agentId: AgentId): boolean;
  /** Merge or replace an agent's roster descriptor. */
  describeAgent(descriptor: AgentDescriptor): void;
  markDisposed(agentId: AgentId, disposedAt: string): void;
  agents(): readonly AgentDescriptor[];
  onRosterChange(listener: RosterListener): Disposable;
}

export class TranscriptStore implements TranscriptStoreLike {
  readonly #agents = new Map<AgentId, AgentTranscript>();
  readonly #descriptors = new Map<AgentId, AgentDescriptor>();
  readonly #rosterListeners = new Set<RosterListener>();

  constructor(readonly sessionId: string) { }

  /** Lazily create (or fetch) the transcript for an agent. */
  ensureAgent(agentId: AgentId, descriptor?: AgentDescriptor): AgentTranscript {
    let transcript = this.#agents.get(agentId);
    if (!transcript) {
      transcript = new AgentTranscript(agentId);
      this.#agents.set(agentId, transcript);
    }
    if (descriptor && this.#descriptors.get(agentId) !== descriptor) {
      this.#descriptors.set(agentId, descriptor);
      this.#emitRoster();
    }
    return transcript;
  }

  getAgent(agentId: AgentId): AgentTranscript | undefined {
    return this.#agents.get(agentId);
  }

  /** Drop an agent entirely (disposed sub-agent, swarm member cleaned up). */
  removeAgent(agentId: AgentId): boolean {
    const removed = this.#agents.delete(agentId);
    if (this.#descriptors.delete(agentId) || removed) this.#emitRoster();
    return removed;
  }

  /** Merge or replace an agent's roster descriptor. */
  describeAgent(descriptor: AgentDescriptor): void {
    if (this.#descriptors.get(descriptor.agentId) !== descriptor) {
      this.#descriptors.set(descriptor.agentId, descriptor);
      this.#emitRoster();
    }
  }

  markDisposed(agentId: AgentId, disposedAt: string): void {
    const descriptor = this.#descriptors.get(agentId);
    if (descriptor === undefined || descriptor.disposedAt !== undefined) return;
    this.describeAgent({ ...descriptor, disposedAt });
  }

  agents(): readonly AgentDescriptor[] {
    return [...this.#descriptors.values()];
  }

  onRosterChange(listener: RosterListener): Disposable {
    this.#rosterListeners.add(listener);
    return { dispose: () => void this.#rosterListeners.delete(listener) };
  }

  #emitRoster(): void {
    const agents = this.agents();
    for (const listener of this.#rosterListeners) listener(agents);
  }
}
