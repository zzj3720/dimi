/**
 * `contextMemory` domain (L4) — wire Model (`ContextModel`) and the wire-protocol
 * 1.4 Ops `context.append_message` (`contextAppendMessage`) / `context.clear`
 * (`contextClear`) / `context.apply_compaction` (`contextApplyCompaction`) /
 * `context.undo` (`contextUndo`) / `context.append_loop_event`
 * (`contextAppendLoopEvent`) for the per-agent conversation history.
 *
 * Declares the history as `ContextMessage[]` (initial `[]`); every Op's `apply`
 * is a pure array transform that returns a NEW reference on change and the SAME
 * reference on a no-op (so the wire's reference-equality gate stays quiet), and
 * carries no non-determinism.
 *
 * The live write path emits flat Ops: non-loop appends (user prompts,
 * injections, hook/task notices) go on the wire as `append_message` (persisted
 * without local ids), while the
 * agent loop streams each turn as `context.append_loop_event` records — the
 * canonical on-disk shape — and `contextAppendLoopEvent` folds
 * them into assistant / tool messages (see `loopEventFold.ts`) both at live
 * dispatch time and on replay. The swarm-mode exit reminder removal is a cross-model fold:
 * `ContextModel` registers a reducer on `swarm_mode.exit` (see
 * `popSwarmModeReminder`) so the pop replays from the `swarm_mode.exit` record
 * itself.
 *
 * `context.undo` counts conversation ticks with the single `isUndoAnchor`
 * predicate (`./conversationTime`) — the same definition the checkpoint
 * protocol pushes with, so anchor counting and checkpoint pushing can never
 * drift apart.
 *
 * Blob handling is declared as a `ModelBlobCodec` on `ContextModel.blobs`:
 * - `dehydrate(record, transform)`: at dispatch time, traverses message content
 *   in `context.append_message` and `context.append_loop_event` records,
 *   passing each `ContentPart[]` through `transform` to offload oversized data
 *   URIs.
 * - `rehydrate(state, transform)`: after replay, traverses the surviving final
 *   state and loads `blobref:` URLs back to inline data — skipping I/O for
 *   data that was compacted away during the session.
 */

import { z } from "zod";

import type { ContentPart } from "#/llmProtocol/message";
import { defineModel, type PartsTransformer } from "#/wire/model";
import type { WireRecord } from "#/wire/record";

import { buildContextCompactionShape } from "./compactionHandoff";
import { isPromptOwnedInjection, isUndoAnchor, isValidUndoCount } from "./conversationTime";
import {
  foldAppendMessage,
  foldLoopEvent,
  resetFold,
  type LoopRecordedEvent,
} from "./loopEventFold";
import type { ContextMessage } from "./types";

async function dehydrateMessages(
  messages: readonly ContextMessage[],
  transform: PartsTransformer,
): Promise<{ changed: boolean; result: ContextMessage[] }> {
  let changed = false;
  const result: ContextMessage[] = [];
  for (const msg of messages) {
    const parts = await transform(msg.content);
    if (parts !== msg.content) {
      changed = true;
      result.push({ ...msg, content: [...parts] as ContentPart[] });
    } else {
      result.push(msg);
    }
  }
  return { changed, result };
}

async function dehydrateRecord(
  record: WireRecord,
  transform: PartsTransformer,
): Promise<WireRecord> {
  if (record.type === "context.append_message") {
    const message = record["message"] as ContextMessage | undefined;
    if (message === undefined) return record;
    const parts = await transform(message.content);
    if (parts === message.content) return record;
    return { ...record, message: { ...message, content: [...parts] } };
  }
  if (record.type === "context.append_loop_event") {
    const event = record["event"] as LoopRecordedEvent | undefined;
    if (event === undefined) return record;
    if (event.type === "content.part") {
      const parts = await transform([event.part]);
      if (parts[0] === event.part) return record;
      return { ...record, event: { ...event, part: parts[0] } };
    }
    if (event.type === "tool.result") {
      const output = event.result.output;
      if (!Array.isArray(output)) return record;
      const parts = await transform(output);
      if (parts === output) return record;
      return { ...record, event: { ...event, result: { ...event.result, output: [...parts] } } };
    }
    return record;
  }
  return record;
}

export const ContextModel = defineModel<ContextMessage[]>("contextMemory", () => [], {
  blobs: {
    dehydrate: dehydrateRecord,
    rehydrate: async (state, transform) => {
      const { changed, result } = await dehydrateMessages(state, transform);
      return changed ? result : state;
    },
  },
  reducers: {
    "swarm_mode.exit": popSwarmModeReminder,
  },
});

function popSwarmModeReminder(state: ContextMessage[], _payload: unknown): ContextMessage[] {
  const last = state[state.length - 1];
  if (last === undefined) return state;
  const origin = last.origin;
  if (origin?.kind !== "injection" || origin.variant !== "swarm_mode") return state;
  return resetFold(state.slice(0, -1)) as ContextMessage[];
}

declare module "#/wire/types" {
  interface PersistedOpMap {
    "context.append_message": typeof contextAppendMessage;
    "context.append_loop_event": typeof contextAppendLoopEvent;
    "context.clear": typeof contextClear;
    "context.apply_compaction": typeof contextApplyCompaction;
    "context.undo": typeof contextUndo;
  }
}

const contextMessageSchema = z.custom<ContextMessage>();
const loopRecordedEventSchema = z.custom<LoopRecordedEvent>();

export const contextAppendMessage = ContextModel.defineOp("context.append_message", {
  schema: z.object({ message: contextMessageSchema }),
  apply: (state, p) => foldAppendMessage(state, p.message) as ContextMessage[],
});

export const contextAppendLoopEvent = ContextModel.defineOp("context.append_loop_event", {
  schema: z.object({ event: loopRecordedEventSchema }),
  apply: (state, p) => foldLoopEvent(state, p.event) as ContextMessage[],
});

export const contextClear = ContextModel.defineOp("context.clear", {
  schema: z.object({}),
  apply: (state) => (state.length === 0 ? state : (resetFold([]) as ContextMessage[])),
});

const contextApplyCompactionSchema = z.object({
  summary: z.string(),
  contextSummary: z.string(),
  compactedCount: z.number(),
  tokensBefore: z.number(),
  tokensAfter: z.number(),
  keptUserMessageCount: z.number(),
  keptHeadUserMessageCount: z.number().optional(),
  droppedCount: z.number().optional(),
});

export type ContextCompactionRecord = z.infer<typeof contextApplyCompactionSchema>;

export const contextApplyCompaction = ContextModel.defineOp("context.apply_compaction", {
  schema: contextApplyCompactionSchema,
  apply: (state, p) => {
    const result = buildContextCompactionShape(state, p);
    return resetFold([...result.messages]) as ContextMessage[];
  },
});

export function readContextCompactionRecord(record: unknown): ContextCompactionRecord {
  return contextApplyCompactionSchema.parse(record);
}

export interface UndoCut {
  readonly cutIndex: number;
  readonly removedCount: number;
  readonly stoppedAtCompaction: boolean;
}

export function computeUndoCut(state: readonly ContextMessage[], count: number): UndoCut {
  let remaining = count;
  let cutIndex = -1;
  let removedCount = 0;
  let stoppedAtCompaction = false;
  for (let i = state.length - 1; i >= 0 && remaining > 0; i--) {
    const message = state[i];
    if (message === undefined || message.origin?.kind === "injection") continue;
    if (message.origin?.kind === "compaction_summary") {
      stoppedAtCompaction = true;
      break;
    }
    if (isUndoAnchor(message)) {
      remaining--;
      removedCount++;
      cutIndex = i;
      while (cutIndex > 0 && isPromptOwnedInjection(state[cutIndex - 1]!, message)) {
        cutIndex--;
      }
    }
  }
  return { cutIndex, removedCount, stoppedAtCompaction };
}

export function isFullyUndoable(cut: UndoCut, count: number): boolean {
  return cut.cutIndex >= 0 && cut.removedCount >= count;
}

export type UndoUnavailableReason =
  | "empty"
  | "compaction_boundary"
  | "insufficient"
  | "checkpoint_lost";

export type UndoPrecheck =
  | { readonly ok: true }
  | {
      readonly ok: false;
      readonly reason: UndoUnavailableReason;
      readonly requested: number;
      readonly undoable: number;
    };

export function precheckUndo(history: readonly ContextMessage[], count: number): UndoPrecheck {
  const cut = computeUndoCut(history, count);
  if (isFullyUndoable(cut, count)) return { ok: true };
  const reason: UndoUnavailableReason = cut.stoppedAtCompaction
    ? "compaction_boundary"
    : cut.removedCount === 0
      ? "empty"
      : "insufficient";
  return { ok: false, reason, requested: count, undoable: cut.removedCount };
}

export function formatUndoUnavailableMessage(
  precheck: Extract<UndoPrecheck, { ok: false }>,
): string {
  switch (precheck.reason) {
    case "empty":
      return "Nothing to undo: no user message to undo";
    case "compaction_boundary":
      return "Nothing to undo: would cross a compaction boundary";
    case "insufficient":
      return `Nothing to undo: only ${precheck.undoable} of ${precheck.requested} requested turn(s) available`;
    case "checkpoint_lost":
      return "Nothing to undo: conversation state checkpoints are incomplete";
  }
}

export const contextUndo = ContextModel.defineOp("context.undo", {
  schema: z.object({
    count: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
  }),
  apply: (state, p) => {
    if (!isValidUndoCount(p.count) || state.length === 0) return state;
    const cut = computeUndoCut(state, p.count);
    if (!isFullyUndoable(cut, p.count)) return state;
    return resetFold(state.slice(0, cut.cutIndex)) as ContextMessage[];
  },
});
