import {
  COMPACTION_ELISION_VARIANT,
  buildCompactionElisionText,
  collectCompactableUserMessages,
  isRealUserInput,
  readContextCompactionRecord,
  renderToolResultForModel,
  selectCompactionUserMessages,
} from '@moonshot-ai/agent-core-v2';
import type {
  ContentPart,
  ContextMessage,
  PermissionMode,
  AgentConfigUpdateData,
  TokenUsage,
  ToolCall,
  WireEntry,
} from './agent-record-types';

export interface ProjectedMessage {
  lineNo: number;
  time?: number;
  source: 'append_message' | 'compaction_summary' | 'undo' | 'clear';
  message: ContextMessage;
  toolStepUuids: string[];
  /** Set only when source === 'undo'. */
  undo?: { count: number; removedMessageCount: number };
  /** Set only on the summary bubble of source === 'compaction_summary'. */
  compaction?: { compactedCount: number; tokensBefore: number; tokensAfter: number };
}

export interface UsageTotals {
  byScope: { session: TokenUsage; turn: TokenUsage };
  byModel: Record<string, TokenUsage>;
}

export interface ConfigSnapshot {
  cwd?: string;
  modelAlias?: string;
  profileName?: string;
  thinkingEffort?: string;
  systemPrompt?: string;
}

export interface GoalSnapshot {
  goalId: string;
  objective: string;
  completionCriterion?: string;
  status?: string;
  actor?: string;
  reason?: string;
  tokensUsed?: number;
  turnsUsed?: number;
  wallClockMs?: number;
}

export interface ContextProjection {
  messages: ProjectedMessage[];
  usage: UsageTotals;
  /** Absolute current context-window fill, mirroring agent-core
   *  ContextMemory._tokenCount. Updated from the latest step.end.usage, and
   *  also reset on the lifecycle events agent-core touches: context.clear → 0,
   *  context.apply_compaction → tokensAfter. Distinct from the cumulative
   *  `usage` totals. */
  contextTokens: number;
  config: ConfigSnapshot;
  permission: { mode: PermissionMode | null };
  planMode: { active: boolean; id?: string };
  goal: GoalSnapshot | null;
  swarm: { active: boolean; trigger?: string };
}

const ZERO: TokenUsage = { inputOther: 0, output: 0, inputCacheRead: 0, inputCacheCreation: 0 };

/** Build a conversation timeline + derived state from a sequence of
 *  wire entries. The reconstruction mirrors agent-core's own
 *  `appendLoopEvent` logic, so:
 *
 *  - `context.append_message` records become messages as-is (the
 *    user / tool messages and any explicit assistant injections).
 *  - `step.begin` pushes a fresh assistant message; later
 *    `content.part` and `tool.call` events on the same step **mutate
 *    that same message** to grow its content / toolCalls. `step.end`
 *    just closes the step.
 *  - `tool.result` events emit an independent `role: 'tool'` message,
 *    matching how agent-core surfaces tool exchanges to the model.
 *
 *  Without this loop-event reconstruction the timeline would only
 *  show user prompts — agent-core does not emit a synthetic
 *  `context.append_message` for assistant turns.
 *
 *  `mode` selects between two views of the four destructive lifecycle
 *  events (compaction / undo / clear / micro-compaction):
 *
 *  - `'model'` (default): faithfully mirrors what the model currently
 *    sees — compaction drops the compacted prefix, undo splices removed
 *    messages out, clear empties the list, micro-compaction blanks old
 *    tool results. All existing behaviour.
 *  - `'full'`: full reconstructed history for debugging — the same four
 *    events insert an INLINE MARKER but do NOT mutate/drop the message
 *    list, so messages compacted/undone/cleared away stay visible and
 *    micro-compacted tool results keep their original content.
 *
 *  Everything else (append_message, loop events, goal/swarm/permission/
 *  plan/config/usage/contextTokens derived state) is identical in both
 *  modes — `mode` only affects the `messages` array and which markers
 *  appear. */
export function projectContext(
  entries: ReadonlyArray<WireEntry>,
  mode: 'model' | 'full' = 'model',
): ContextProjection {
  let messages: ProjectedMessage[] = [];
  const usage: UsageTotals = {
    byScope: { session: { ...ZERO }, turn: { ...ZERO } },
    byModel: {},
  };
  const config: ConfigSnapshot = {};
  let permissionMode: PermissionMode | null = null;
  let planActive = false;
  let planId: string | undefined;
  let contextTokens = 0;
  let goal: GoalSnapshot | null = null;
  let swarm: { active: boolean; trigger?: string } = { active: false };
  let microCutoff = 0;
  // Maps step.uuid → the assistant ProjectedMessage that step is filling in.
  // Cleared on context.clear / context.apply_compaction.
  let openSteps = new Map<string, ProjectedMessage>();

  for (const entry of entries) {
    const rec = entry.data;
    switch (rec.type) {
      case 'context.append_message':
        messages.push({
          lineNo: entry.lineNo,
          time: rec.time,
          source: 'append_message',
          message: rec.message,
          toolStepUuids: [],
        });
        break;
      case 'context.append_loop_event': {
        const ev = rec.event;
        if (ev.type === 'step.begin') {
          const message: ContextMessage = {
            role: 'assistant',
            content: [],
            toolCalls: [],
          };
          const projected: ProjectedMessage = {
            lineNo: entry.lineNo,
            time: rec.time,
            source: 'append_message',
            message,
            toolStepUuids: [ev.uuid],
          };
          messages.push(projected);
          openSteps.set(ev.uuid, projected);
        } else if (ev.type === 'content.part') {
          const projected = openSteps.get(ev.stepUuid);
          if (projected !== undefined) {
            (projected.message.content as ContentPart[]).push(ev.part);
          }
        } else if (ev.type === 'tool.call') {
          const projected = openSteps.get(ev.stepUuid);
          if (projected !== undefined) {
            const args =
              typeof ev.args === 'string'
                ? ev.args
                : ev.args === undefined
                  ? null
                  : JSON.stringify(ev.args);
            (projected.message.toolCalls as ToolCall[]).push({
              type: 'function',
              id: ev.toolCallId,
              name: ev.name,
              arguments: args,
            });
          }
        } else if (ev.type === 'step.end') {
          // Absolute context-window fill, mirroring agent-core
          // ContextMemory._tokenCount: the latest step.end usage REPLACES the
          // snapshot (it is not cumulative — see Task P1.7 note on byScope).
          // A zero-usage step.end (e.g. a content-filtered response) is the one
          // exception agent-core makes — it keeps the prior count instead of
          // resetting to 0 — so guard against a false drop here too.
          if ('usage' in ev && ev.usage !== undefined) {
            const fill =
              ev.usage.inputCacheRead +
              ev.usage.inputCacheCreation +
              ev.usage.inputOther +
              ev.usage.output;
            if (fill > 0) contextTokens = fill;
          }
          openSteps.delete(ev.uuid);
        } else if (ev.type === 'tool.result') {
          // Mirror what the MODEL saw, not the raw output. This calls the
          // SAME `renderToolResultForModel` agent-core applies at its LLM
          // projection boundary (error status prefix, empty-output
          // placeholder, trailing note), so vis's model view is the real
          // projection rather than a hand-kept copy.
          const content = renderToolResultForModel(ev.result);
          const toolMsg: ContextMessage = {
            role: 'tool',
            content,
            toolCalls: [],
            toolCallId: ev.toolCallId,
            ...(ev.result.isError === true ? { isError: true } : {}),
          };
          messages.push({
            lineNo: entry.lineNo,
            time: rec.time,
            source: 'append_message',
            message: toolMsg,
            toolStepUuids: [],
          });
        }
        break;
      }
      case 'context.update_token_count':
        contextTokens = rec.tokenCount;
        break;
      case 'context.clear':
        if (mode === 'model') {
          messages = [];
          openSteps = new Map();
          microCutoff = 0;
        } else {
          // Full history: keep all preceding messages and openSteps as-is, just
          // append a synthetic 'clear' marker inline. The original tool results
          // stay un-blanked, so the cutoff is not applied (the end-of-loop
          // blanking pass is gated on model mode).
          messages.push({
            lineNo: entry.lineNo,
            time: rec.time,
            source: 'clear',
            // Synthetic marker: never rendered as a bubble (the web dispatches on
            // `source === 'clear'`). `role: 'assistant'` keeps it out of any
            // role-counting / tool-blanking path.
            message: { role: 'assistant', content: [], toolCalls: [] } as ContextMessage,
            toolStepUuids: [],
          });
        }
        // Mirror agent-core clear() → _tokenCount = 0: the context-window fill is
        // wiped. Derived state, so it is mode-INDEPENDENT (applied for both modes).
        contextTokens = 0;
        break;
      case 'context.apply_compaction': {
        openSteps = new Map();
        const compaction = readContextCompactionRecord(rec);
        // Mirror agent-core's `applyCompaction`
        // (`packages/agent-core-v2/src/agent/contextMemory/contextMemoryService.ts`):
        // the live history
        // becomes the kept real user messages (verbatim, within a token budget
        // — the oldest head plus the most recent tail, separated by an elision
        // marker when the pool overflowed) followed by a single user-role
        // summary tagged `origin.kind = 'compaction_summary'`. Assistant
        // messages, tool calls, and tool results are dropped. The selection
        // rules (`selectCompactionUserMessages` /
        // `collectCompactableUserMessages`) are the same helpers the runtime's
        // `ContextMemory` and the web transcript reducer apply, so all three
        // views stay in sync.
        const summaryBubble: ProjectedMessage = {
          lineNo: entry.lineNo,
          time: rec.time,
          source: 'compaction_summary',
          message: {
            role: 'user',
            content: [{ type: 'text', text: compaction.summary }],
            toolCalls: [],
            origin: { kind: 'compaction_summary' },
          } as ContextMessage,
          toolStepUuids: [],
          compaction: {
            compactedCount: compaction.compactedCount,
            tokensBefore: compaction.tokensBefore,
            tokensAfter: compaction.tokensAfter ?? 0,
          },
        };
        const modelSummaryBubble: ProjectedMessage = {
          ...summaryBubble,
          message: {
            ...summaryBubble.message,
            content: [{ type: 'text', text: compaction.contextSummary }],
          } as ContextMessage,
        };
        if (mode === 'model') {
          const historyEntries = messages.filter(isHistoryEntry);
          const realUserEntries = historyEntries.filter(
            (pm) => collectCompactableUserMessages([pm.message]).length === 1,
          );
          const selection = selectCompactionUserMessages(
            realUserEntries.map((pm) => pm.message),
          );
          const tailStart = realUserEntries.length - selection.tail.length;
          const headEntries: ProjectedMessage[] = selection.head.map((message, i) => {
            const original = i < tailStart ? realUserEntries[i]! : realUserEntries[tailStart]!;
            if (original.message === message) return original;
            return i < tailStart
              ? { ...original, message }
              : { ...original, lineNo: original.lineNo - 0.5, message };
          });
          const tailEntries: ProjectedMessage[] = selection.tail.map((message, i) => {
            const original = realUserEntries[tailStart + i]!;
            return original.message === message ? original : { ...original, message };
          });
          const markerBubble: ProjectedMessage[] = selection.elided
            ? [{
              lineNo: entry.lineNo - 0.5,
              time: rec.time,
              source: 'append_message',
              message: {
                role: 'user',
                content: [
                  { type: 'text', text: buildCompactionElisionText(selection.omittedTokens) },
                ],
                toolCalls: [],
                origin: { kind: 'injection', variant: COMPACTION_ELISION_VARIANT },
              } as ContextMessage,
              toolStepUuids: [],
            }]
            : [];
          messages = [...headEntries, ...markerBubble, ...tailEntries, modelSummaryBubble];
        } else {
          // Full history: keep ALL preceding messages, just append the summary
          // marker inline so the compacted prefix stays visible.
          messages.push(summaryBubble);
        }
        microCutoff = 0;
        // Mirror agent-core applyCompaction() → _tokenCount = result.tokensAfter:
        // the live context-window fill is now the post-compaction count. Derived
        // state, so it is mode-INDEPENDENT.
        contextTokens = compaction.tokensAfter;
        break;
      }
      case 'usage.record': {
        // byScope keeps per-scope cumulative spend. This is NOT the live context-window
        // fill — that is `contextTokens` (latest step.end.usage). The web TokenBar shows
        // contextTokens; byScope/byModel are for the cumulative breakdown only.
        const scope = (rec.usageScope ?? 'session') as 'session' | 'turn';
        addUsage(usage.byScope[scope], rec.usage);
        if (!usage.byModel[rec.model]) usage.byModel[rec.model] = { ...ZERO };
        addUsage(usage.byModel[rec.model]!, rec.usage);
        break;
      }
      case 'config.update': {
        const upd = rec as AgentConfigUpdateData & { type: 'config.update' };
        if (upd.cwd !== undefined) config.cwd = upd.cwd;
        if (upd.modelAlias !== undefined) config.modelAlias = upd.modelAlias;
        if (upd.profileName !== undefined) config.profileName = upd.profileName;
        if (upd.thinkingLevel !== undefined) config.thinkingEffort = upd.thinkingLevel;
        if (upd.systemPrompt !== undefined) config.systemPrompt = upd.systemPrompt;
        break;
      }
      case 'permission.set_mode':
        permissionMode = rec.mode;
        break;
      case 'plan_mode.enter':
        planActive = true; planId = rec.id; break;
      case 'plan_mode.cancel':
      case 'plan_mode.exit':
        planActive = false; planId = undefined; break;
      case 'context.undo': {
        // Mirror agent-core `undo` (`agent/context/index.ts`): walk from the
        // end, skip `origin.kind === 'injection'`, stop at
        // `origin.kind === 'compaction_summary'`, remove others, counting real
        // user prompts via `isRealUserInput` until `count` is reached. Then
        // leave an undo marker.
        //
        // `computeUndoCutoff` is the single source of truth for that skip/stop
        // walk (shared by both modes); only the actual removal is gated on
        // `'model'` mode.
        const { cutoff, removedMessageCount } = computeUndoCutoff(messages, rec.count);
        if (mode === 'model') {
          // Remove everything from `cutoff` onward EXCEPT injections, which the
          // walk skips (they survive even when inside the undo window). Using
          // the same `origin.kind === 'injection'` predicate keeps removal in
          // lockstep with the counting walk above.
          messages = messages.filter(
            (pm, i) => i < cutoff || pm.message.origin?.kind === 'injection',
          );
          openSteps = new Map();
          const historyCount = messages.reduce(
            (count, message) => count + (isHistoryEntry(message) ? 1 : 0),
            0,
          );
          microCutoff = Math.min(microCutoff, historyCount);
        }
        // In 'full' mode: do NOT remove — keep the undone messages and openSteps
        // as-is, only push the undo marker. `removedMessageCount` still reflects
        // what WOULD have been removed.
        messages.push({
          lineNo: entry.lineNo,
          time: rec.time,
          source: 'undo',
          // Synthetic message: never rendered. The web dispatches on
          // `source === 'undo'`; this only satisfies ProjectedMessage.
          // `role: 'assistant'` is deliberate so this marker can never match the
          // `role: 'tool'` micro-compaction blanking gate — keep it non-tool if
          // you ever change the placeholder.
          message: { role: 'assistant', content: [], toolCalls: [] } as ContextMessage,
          toolStepUuids: [],
          undo: { count: rec.count, removedMessageCount },
        });
        break;
      }
      case 'micro_compaction.apply':
        microCutoff = rec.cutoff;
        break;
      case 'goal.create':
        goal = {
          goalId: rec.goalId,
          objective: rec.objective,
          completionCriterion: rec.completionCriterion,
        };
        break;
      case 'goal.update':
        if (goal !== null) {
          const prev: GoalSnapshot = goal;
          goal = {
            ...prev,
            status: rec.status ?? prev.status,
            actor: rec.actor ?? prev.actor,
            reason: rec.reason ?? prev.reason,
            tokensUsed: rec.tokensUsed ?? prev.tokensUsed,
            turnsUsed: rec.turnsUsed ?? prev.turnsUsed,
            wallClockMs: rec.wallClockMs ?? prev.wallClockMs,
          };
        }
        break;
      case 'goal.clear':
        goal = null;
        break;
      case 'swarm_mode.enter':
        swarm = { active: true, trigger: rec.trigger };
        break;
      case 'swarm_mode.exit':
        swarm = { active: false };
        break;
      // Kinds that don't affect the projected timeline / derived state,
      // including the observability records (request trace — `llm.*`,
      // `mcp.tools_discovered`), which are never part of context state:
      case 'forked':
      case 'turn.prompt':
      case 'turn.steer':
      case 'turn.cancel':
      case 'permission.record_approval_result':
      case 'full_compaction.begin':
      case 'full_compaction.cancel':
      case 'full_compaction.complete':
      case 'tools.register_user_tool':
      case 'tools.unregister_user_tool':
      case 'tools.set_active_tools':
      case 'tools.update_store':
      case 'llm.tools_snapshot':
      case 'llm.request':
      case 'mcp.tools_discovered':
        break;
      default: {
        break;
      }
    }
  }

  if (mode === 'model' && microCutoff > 0) {
    let historyIndex = 0;
    for (const projected of messages) {
      if (!isHistoryEntry(projected)) continue;
      if (historyIndex >= microCutoff) break;
      historyIndex++;
      const message = projected.message;
      if (
        message.role === 'tool' &&
        message.toolCallId !== undefined &&
        estimateContentTokens(message.content) >= MICRO_MIN_CONTENT_TOKENS
      ) {
        projected.message = {
          ...message,
          content: [{ type: 'text', text: MICRO_TRUNCATED_MARKER }],
        };
      }
    }
  }

  return {
    messages,
    usage,
    contextTokens,
    config,
    permission: { mode: permissionMode },
    planMode: { active: planActive, id: planId },
    goal,
    swarm,
  };
}

function addUsage(into: TokenUsage, src: TokenUsage): void {
  (into as any).inputOther += src.inputOther;
  (into as any).output += src.output;
  (into as any).inputCacheRead += src.inputCacheRead;
  (into as any).inputCacheCreation += src.inputCacheCreation;
}

const MICRO_TRUNCATED_MARKER = '[Old tool result content cleared]';
const MICRO_MIN_CONTENT_TOKENS = 100;

function estimateContentTokens(content: readonly ContentPart[]): number {
  let total = 0;
  for (const part of content) {
    if (part.type === 'text') total += estimateTokens(part.text);
    else if (part.type === 'think') total += estimateTokens(part.think);
  }
  return total;
}

function estimateTokens(text: string): number {
  let asciiCount = 0;
  let nonAsciiCount = 0;
  for (const char of text) {
    if (char.codePointAt(0)! <= 127) asciiCount++;
    else nonAsciiCount++;
  }
  return Math.ceil(asciiCount / 4) + nonAsciiCount;
}

/** True for messages that correspond to a real agent-core `_history` entry —
 *  i.e. `append_message` and `compaction_summary` (the summary IS in `_history`).
 *  The synthetic UI-only markers (`undo` / `clear`) are NOT in `_history`, so
 *  index-based operations that mirror agent-core (compaction slice, micro-
 *  compaction cutoff) must skip them to stay aligned with agent-core indices. */
function isHistoryEntry(pm: ProjectedMessage): boolean {
  return pm.source !== 'undo' && pm.source !== 'clear';
}

/** Single source of truth for the `context.undo` backward walk, shared by both
 *  projection modes. Mirrors agent-core `undo` (`agent/context/index.ts`): walk
 *  from the end, skip `origin.kind === 'injection'` (those are KEPT even when
 *  they sit inside the undo window), stop at `origin.kind === 'compaction_summary'`,
 *  and count real user prompts via `isRealUserInput` until `count` is reached.
 *
 *  Returns the `cutoff` (lowest index to remove from, inclusive) plus the
 *  `removedMessageCount` (number of non-skipped messages in the window). In
 *  `'model'` mode the caller removes everything from `cutoff` onward EXCEPT
 *  injections; in `'full'` mode only `removedMessageCount` is reported on the
 *  undo marker (no removal). Defining the skip/stop predicate exactly once here
 *  keeps the two modes from drifting. */
function computeUndoCutoff(
  messages: readonly ProjectedMessage[],
  count: number,
): { cutoff: number; removedMessageCount: number } {
  let removedUserCount = 0;
  let removedMessageCount = 0;
  let cutoff = messages.length;
  for (let i = messages.length - 1; i >= 0; i--) {
    const origin = messages[i]?.message.origin;
    if (origin?.kind === 'injection') continue; // skip, keep
    if (origin?.kind === 'compaction_summary') break; // stop
    removedMessageCount++;
    cutoff = i;
    if (isRealUserInput(messages[i]!.message) && ++removedUserCount >= count) break;
  }
  return { cutoff, removedMessageCount };
}
