/**
 * `MessageLegacyService` — kap-server-edge projection of the v1
 * `GET /api/v1/sessions/{sid}/messages[/{mid}]` contract on top of the native
 * v2 engine services.
 *
 * This is the v1 wire-compat adapter previously kept in agent-core-v2
 * (`src/app/messageLegacy/`) — deliberately relocated to the kap-server edge
 * (same as `services/legacyStatus/`) so the core engine stays free of v1
 * wire-compatibility concerns.
 *
 * The native `IAgentContextMemoryService` (Agent scope, serving `/api/v2`
 * `messages:*`) holds the model's CURRENT, folded context and is NOT the full
 * transcript: after a compaction it collapses into `[...keptUserMessages,
 * compaction_summary]`. The full transcript is reduced on demand by streaming
 * the main agent's `wire.jsonl`; the service does not make every live Agent
 * retain its raw journal in memory. The `ContextMessage → Message` projection
 * is shared with the `snapshot` and `:undo` edges via
 * `contextMemory/messageProjection`.
 *
 * History is streamed from the main agent's append log after its pending wire
 * writes are flushed. The journal is folded incrementally by the same
 * transcript reducer v1's `MessageService` uses, keeping full history across
 * compactions (inserting a summary marker instead of folding) — unlike the live
 * `IAgentContextMemoryService.get()`, whose folded context collapses into
 * `[...keptUserMessages, compaction_summary]` and would lose the prefix.
 * `foldedLength` is what the live history length WOULD be from the journal's
 * records; because the journal can trail the live context by a record within a
 * single dispatch, anything beyond it is appended as the unflushed tail.
 * Pagination, id derivation, and the role filter mirror v1's `MessageService`.
 *
 * Error contract (mapped at the route layer):
 *   - `session.not_found`  → 40401
 *   - `message.not_found`  → 40403
 */

import type { Message, MessageRole } from '@dimi-agent/agent-core-v2/agent/contextMemory/protocolMessage';
import {
  createContextTranscriptReducer,
  ensureMainAgent,
  Error2,
  ErrorCodes,
  IAgentBlobService,
  IAgentContextMemoryService,
  IAgentScopeContext,
  IAppendLogStore,
  ISessionIndex,
  ISessionLifecycleService,
  IWireService,
  toProtocolMessage,
  AGENT_WIRE_RECORD_KEY,
  type ContextMessage,
  type ContextTranscript,
  type ErrorCode,
  type IAgentScopeHandle,
  type Scope,
  type WireRecord,
} from '@dimi-agent/agent-core-v2';

/** Cursor pagination query shared by the v1 history/list endpoints. */
export interface CursorQuery {
  before_id?: string | undefined;
  after_id?: string | undefined;
  page_size?: number | undefined;
}

export interface PageResponse<T> {
  items: T[];
  has_more: boolean;
}

export interface MessageListQuery extends CursorQuery {
  readonly role?: MessageRole;
}

const DEFAULT_PAGE_SIZE = 50;
const MAX_PAGE_SIZE = 100;

/** v1 wire code for an unknown message (`message.not_found` → 40403). */
const MESSAGE_NOT_FOUND = 'message.not_found' as const;
export class MessageLegacyService {
  private readonly lifecycle: ISessionLifecycleService;
  private readonly index: ISessionIndex;
  private readonly appendLog: IAppendLogStore;

  constructor(core: Scope) {
    this.lifecycle = core.accessor.get(ISessionLifecycleService);
    this.index = core.accessor.get(ISessionIndex);
    this.appendLog = core.accessor.get(IAppendLogStore);
  }

  async list(sessionId: string, query: MessageListQuery): Promise<PageResponse<Message>> {
    const all = await this.loadMessages(sessionId);
    const desc = [...all].reverse();

    let pivotIndex = -1;
    if (query.before_id !== undefined) {
      pivotIndex = desc.findIndex((m) => m.id === query.before_id);
    } else if (query.after_id !== undefined) {
      pivotIndex = desc.findIndex((m) => m.id === query.after_id);
    }

    let slice: Message[];
    if (query.before_id !== undefined && pivotIndex >= 0) {
      slice = desc.slice(pivotIndex + 1);
    } else if (query.after_id !== undefined && pivotIndex >= 0) {
      slice = desc.slice(0, pivotIndex);
    } else {
      slice = desc;
    }

    const requestedSize = query.page_size ?? DEFAULT_PAGE_SIZE;
    const pageSize = Math.min(Math.max(requestedSize, 1), MAX_PAGE_SIZE);
    const page = slice.slice(0, pageSize);
    const hasMore = slice.length > pageSize;

    const filtered = query.role !== undefined ? page.filter((m) => m.role === query.role) : page;

    return { items: filtered, has_more: hasMore };
  }

  async get(sessionId: string, messageId: string): Promise<Message> {
    const all = await this.loadMessages(sessionId);
    const entry = all.find((m) => m.id === messageId);
    if (entry === undefined) {
      // `message.not_found` is an edge-owned v1 wire code, no longer part of
      // the engine's closed `ErrorCode` union — assert it at the boundary so
      // the route's `isError2` mapping still sees it.
      throw new Error2(
        MESSAGE_NOT_FOUND as ErrorCode,
        `message ${messageId} does not exist in session ${sessionId}`,
      );
    }
    return entry;
  }

  private async loadMessages(sessionId: string): Promise<Message[]> {
    const summary = await this.index.get(sessionId);
    if (summary === undefined) {
      throw new Error2(ErrorCodes.SESSION_NOT_FOUND, `session ${sessionId} does not exist`);
    }

    const session = await this.lifecycle.resume(sessionId);
    if (session === undefined) return [];
    const agent = await ensureMainAgent(session);

    const transcript = await this.readTranscript(agent);
    const contextMessages = agent.accessor.get(IAgentContextMemoryService).get();
    const merged = mergeLiveTail(transcript, contextMessages);
    const entries = await this.rehydrate(agent, merged.messages);

    let previousMs = Number.NEGATIVE_INFINITY;
    return entries.map((msg, index) => {
      const baseMs = merged.times[index] ?? summary.createdAt + index;
      const createdAtMs = Math.max(previousMs + 1, baseMs);
      previousMs = createdAtMs;
      return toProtocolMessage(sessionId, index, msg, summary.createdAt, createdAtMs);
    });
  }

  /**
   * Replace `blobref:` media URLs with `data:` URIs read from the agent's
   * blob store (v1's `rehydrateBlobRefs`); unresolvable refs become the
   * `[media missing]` placeholder, same as v1 and live replay.
   */
  private async rehydrate(
    agent: IAgentScopeHandle,
    messages: readonly ContextMessage[],
  ): Promise<readonly ContextMessage[]> {
    const blobs = agent.accessor.get(IAgentBlobService);
    let changed = false;
    const out: ContextMessage[] = [];
    for (const msg of messages) {
      const content = await blobs.loadParts(msg.content);
      if (content === msg.content) {
        out.push(msg);
        continue;
      }
      changed = true;
      out.push({ ...msg, content: [...content] });
    }
    return changed ? out : messages;
  }

  private async readTranscript(agent: IAgentScopeHandle): Promise<ContextTranscript> {
    await agent.accessor.get(IWireService).flush();
    const scope = agent.accessor.get(IAgentScopeContext).scope();
    const reducer = createContextTranscriptReducer();
    for await (const record of this.appendLog.read<WireRecord>(scope, AGENT_WIRE_RECORD_KEY)) {
      reducer.add(record);
    }
    return reducer.result();
  }
}

function mergeLiveTail(
  transcript: ContextTranscript,
  contextMessages: readonly ContextMessage[],
): {
  readonly messages: readonly ContextMessage[];
  readonly times: readonly (number | undefined)[];
} {
  if (contextMessages.length <= transcript.foldedLength) {
    return { messages: transcript.entries, times: transcript.times };
  }
  const tail = contextMessages.slice(transcript.foldedLength);
  return {
    messages: [...transcript.entries, ...tail],
    times: [...transcript.times, ...tail.map(() => undefined)],
  };
}
