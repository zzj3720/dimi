/**
 * Rust-backed transcript store (M1 swap-in socket).
 *
 * `RustTranscriptStore` implements the same session-level surface as the TS
 * `TranscriptStore`, but every agent transcript lives on the Rust side
 * (`dimi-store` via the napi bridge): ops are applied there and snapshots are
 * materialized there. The read methods of `RustAgentTranscript` derive from
 * `snapshot()` — the differential suites prove the snapshot JSON is
 * wire-identical to the TS store's, so downstream consumers cannot tell the
 * difference.
 *
 * Notes:
 *  - `pendingInteractions` derives from the snapshot's interaction entities
 *    (`state === 'pending'`), which is exactly how the TS store maintains its
 *    set (applyReset derives it the same way; upserts maintain the same
 *    invariant).
 *  - `onChange` is a no-op: kap-server never subscribes to the per-agent
 *    store events — the core binding broadcasts through its own op pipeline
 *    (`bindSessionTranscript`), and the roster change channel used by the
 *    broadcaster is the store-level `onRosterChange`, which is implemented.
 *  - The bridge is loaded lazily; constructing a store with the flag on but
 *    no native binding throws the dimi-native build hint.
 */

import { RustAgentTranscript } from '@dimi-agent/dimi-native';
import type {
  AgentDescriptor,
  AgentTranscriptLike,
  AppliedOps,
  AttachmentId,
  Disposable,
  InteractionId,
  PromptId,
  RosterListener,
  TaskId,
  TodoId,
  TranscriptAttachment,
  TranscriptInteraction,
  TranscriptItem,
  TranscriptListener,
  TranscriptMeta,
  TranscriptOperation,
  TranscriptPrompt,
  TranscriptStoreLike,
  TranscriptTask,
  TranscriptTodo,
  TranscriptTurn,
  TurnId,
  AgentTranscriptSnapshot,
} from '@dimi-agent/transcript';

/** One agent's transcript, held on the Rust side. */
export class RustAgentTranscriptAdapter implements AgentTranscriptLike {
  constructor(
    readonly agentId: string,
    private readonly inner: RustAgentTranscript,
  ) {}

  /** Full load == applying a reset (same as `apply`). */
  receive(ops: readonly TranscriptOperation[]): AppliedOps {
    return this.apply(ops);
  }

  apply(ops: readonly TranscriptOperation[]): AppliedOps {
    // The Rust side omits `gap` when clean (see dimi-store store.rs), but a
    // JSON round-trip may still hand us `null`; TS consumers check
    // `result.gap !== undefined`, so normalize null → undefined.
    const result = JSON.parse(this.inner.apply(JSON.stringify(ops))) as {
      accepted: TranscriptOperation[];
      gap?: unknown;
    };
    return { accepted: result.accepted, gap: (result.gap ?? undefined) as AppliedOps['gap'] };
  }

  /** No-op: server consumers use the core binding's own op pipeline. */
  onChange(_listener: TranscriptListener): Disposable {
    return { dispose: () => {} };
  }

  private state(): AgentTranscriptSnapshot {
    return JSON.parse(this.inner.snapshot()) as AgentTranscriptSnapshot;
  }

  getItems(): readonly TranscriptItem[] {
    return this.state().items;
  }

  getTurn(turnId: TurnId): TranscriptTurn | undefined {
    const item = this.state().items.find(
      (entry) => entry.kind === 'turn' && entry.turnId === turnId,
    );
    return item?.kind === 'turn' ? item : undefined;
  }

  getTasks(): ReadonlyMap<TaskId, TranscriptTask> {
    return new Map(this.state().tasks.map((task) => [task.taskId, task]));
  }

  getTask(taskId: TaskId): TranscriptTask | undefined {
    return this.getTasks().get(taskId);
  }

  getInteractions(): ReadonlyMap<InteractionId, TranscriptInteraction> {
    return new Map(this.state().interactions.map((i) => [i.interactionId, i]));
  }

  getInteraction(interactionId: InteractionId): TranscriptInteraction | undefined {
    return this.getInteractions().get(interactionId);
  }

  getAttachments(): ReadonlyMap<AttachmentId, TranscriptAttachment> {
    return new Map(this.state().attachments.map((a) => [a.attachmentId, a]));
  }

  getAttachment(attachmentId: AttachmentId): TranscriptAttachment | undefined {
    return this.getAttachments().get(attachmentId);
  }

  getTodos(): ReadonlyMap<TodoId, TranscriptTodo> {
    return new Map(this.state().todos.map((t) => [t.todoId, t]));
  }

  getTodo(todoId: TodoId): TranscriptTodo | undefined {
    return this.getTodos().get(todoId);
  }

  getPrompts(): ReadonlyMap<PromptId, TranscriptPrompt> {
    return new Map(this.state().prompts.map((p) => [p.promptId, p]));
  }

  getPrompt(promptId: PromptId): TranscriptPrompt | undefined {
    return this.getPrompts().get(promptId);
  }

  getMeta(): TranscriptMeta {
    return this.state().meta;
  }

  listPendingInteractions(): readonly InteractionId[] {
    return this.state()
      .interactions.filter((interaction) => interaction.state === 'pending')
      .map((interaction) => interaction.interactionId);
  }

  get hasMoreOlder(): boolean {
    return this.state().hasMoreOlder ?? false;
  }

  snapshot(window?: { tailTurns: number }): AgentTranscriptSnapshot {
    return JSON.parse(
      this.inner.snapshot(window === undefined ? undefined : JSON.stringify(window)),
    ) as AgentTranscriptSnapshot;
  }

  dispose(): void {
    // The Rust handle owns no native resources beyond the object itself.
  }
}

/**
 * Session-level store over Rust transcripts — same surface as `TranscriptStore`.
 */
export class RustTranscriptStore implements TranscriptStoreLike {
  readonly #transcripts = new Map<string, RustAgentTranscriptAdapter>();
  readonly #descriptors = new Map<string, AgentDescriptor>();
  readonly #rosterListeners = new Set<RosterListener>();

  constructor(readonly sessionId: string) {}

  ensureAgent(agentId: string, descriptor?: AgentDescriptor): RustAgentTranscriptAdapter {
    let transcript = this.#transcripts.get(agentId);
    if (transcript === undefined) {
      transcript = new RustAgentTranscriptAdapter(agentId, new RustAgentTranscript(agentId));
      this.#transcripts.set(agentId, transcript);
    }
    if (descriptor !== undefined && this.#descriptors.get(agentId) !== descriptor) {
      this.#descriptors.set(agentId, descriptor);
      this.#emitRoster();
    }
    return transcript;
  }

  getAgent(agentId: string): RustAgentTranscriptAdapter | undefined {
    return this.#transcripts.get(agentId);
  }

  removeAgent(agentId: string): boolean {
    const removed = this.#transcripts.delete(agentId);
    if (this.#descriptors.delete(agentId) || removed) this.#emitRoster();
    return removed;
  }

  describeAgent(descriptor: AgentDescriptor): void {
    if (this.#descriptors.get(descriptor.agentId) !== descriptor) {
      this.#descriptors.set(descriptor.agentId, descriptor);
      this.#emitRoster();
    }
  }

  markDisposed(agentId: string, disposedAt: string): void {
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
