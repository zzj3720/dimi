/**
 * Parity migration manifest (A5 architecture review).
 *
 * One machine-checkable ledger of every TS behavior domain the Rust engine
 * must match. Each entry is either `migrated: true` with the test that
 * covers it, or `migrated: false` with an explicit `gap` description (an
 * acknowledged, tracked debt). The gate test (`parity-manifest.test.ts`)
 * refuses entries that are unclaimed: a migrated entry without coverage, or
 * an unmigrated entry without a gap description.
 *
 * This replaces the human-memory "what's left" state: a fresh review round
 * reads the ledger instead of rediscovering gaps from zero, and fixing a gap
 * means flipping the entry + landing the coverage test.
 */

export interface ParityEntry {
  readonly domain: string;
  readonly migrated: boolean;
  /** Test(s) that pin this domain (required when migrated). */
  readonly coveredBy?: string;
  /** Acknowledged gap (required when NOT migrated). */
  readonly gap?: string;
}

export const PARITY_MANIFEST: readonly ParityEntry[] = [
  {
    domain: 'permission: 12-node chain order',
    migrated: true,
    coveredBy: 'permissionDifferential.test.ts + permission.rs tests',
  },
  {
    domain: 'permission: deny/skip/reject message text',
    migrated: true,
    coveredBy: 'permissionDifferential.test.ts (reason parity)',
  },
  {
    domain: 'permission: rule arg-glob matching (picomatch `/` semantics)',
    migrated: true,
    coveredBy: 'permissionDifferential.test.ts + glob_matches_picomatch_separator_semantics',
  },
  {
    domain: 'approval: pause/resume continues the batch',
    migrated: true,
    coveredBy: 'approval_batch_tests + engine-event-matrix.test.ts',
  },
  {
    domain: 'approval: started/result exactly once, after the decision',
    migrated: true,
    coveredBy: 'approval_batch_tests + engine-event-matrix.test.ts',
  },
  {
    domain: 'approval: session-scope applies to the same turn',
    migrated: true,
    coveredBy: 'engine-event-matrix.test.ts + rustEngineTurnRunner.test.ts',
  },
  {
    domain: 'cancel: mid-batch and during pending approval',
    migrated: true,
    coveredBy: 'cancel tests + engine-event-matrix.test.ts',
  },
  {
    domain: 'event stream: turn/step/tool sequences on all paths',
    migrated: true,
    coveredBy: 'engine-event-matrix.test.ts (golden matrix)',
  },
  {
    domain: 'max-steps: no step.interrupted (TS parity)',
    migrated: true,
    coveredBy: 'engine-event-matrix.test.ts + engine-differential.test.ts',
  },
  {
    domain: 'tool abort: registry forwards to the executor',
    migrated: true,
    coveredBy: 'tool registry abort test',
  },
  {
    domain: 'tool result truncation (50k preview for external tools)',
    migrated: true,
    coveredBy: 'rustEngineTurnRunner.test.ts (truncation service consulted for external results)',
  },
  {
    domain: 'PreToolUse/PostToolUse external hooks',
    migrated: true,
    coveredBy:
      'engine-event-matrix.test.ts (native gate veto) + rustEngineTurnRunner.test.ts (external veto/hooks + native PostToolUse)',
  },
  {
    domain: 'tool dedupe (toolDedupe dup detection)',
    migrated: true,
    coveredBy:
      'engine dedupe_tests (same-step suppression, cross-step reminders 3/5/8, force-stop 12, distinct-args passthrough)',
  },
  {
    domain: 'activeToolNames filtering (policy [tools]/disabled)',
    migrated: true,
    coveredBy:
      'rustEngineTurnRunner.test.ts (filtered tool rejected) + bridge engine_tools_filters_defs_by_active_tools + engine request-tools pass-through',
  },
  {
    domain: 'step-level cancel (per-step AbortController)',
    migrated: false,
    gap: 'not implemented — only whole-turn cancel exists',
  },
  {
    domain: 'WaitFor user-wait parking / notification wake-up',
    migrated: false,
    gap: 'not implemented — WaitFor blocks the turn in-engine; the def honestly declares this',
  },
  {
    domain: 'compaction reservedContextSize / observed window / media stripping',
    migrated: false,
    gap:
      'reserved-space trigger, origin filtering, and head/tail message selection are implemented; ' +
      'dynamic-tool stripping, observed-window calibration, media degradation, and partial-message truncation remain',
  },
  {
    domain: 'plan mode orchestration (EnterPlanMode/ExitPlanMode)',
    migrated: false,
    gap: 'not implemented — separate module slice',
  },
  {
    domain: 'turn id 0-based on the wire (TS parity)',
    migrated: true,
    coveredBy:
      'rustEngineTurnRunner.test.ts + rust-engine-coverage.test.ts (turn_id 0, 1, …; TS loop.test.ts telemetry also asserts turn_id: 0)',
  },
  {
    domain: 'turn.started origin (task-origin turns)',
    migrated: true,
    coveredBy:
      'engine turn_started_carries_the_input_origin + rustEngineTurnRunner.test.ts (task origin on the bus)',
  },
  {
    domain: 'interrupted-step streamed text (completed-response flush, mid-stream drop)',
    migrated: true,
    coveredBy:
      'rustEngineTurnRunner.test.ts (flushes text whose LLM response completed when interrupted mid-tool; drops mid-stream partials — TS parity)',
  },
  {
    domain: 'external tool updates streamed as tool.progress',
    migrated: true,
    coveredBy: 'rustEngineTurnRunner.test.ts (external-tool onUpdate → tool.progress bus events)',
  },
  {
    domain: 'worker rejection/cancel message suffix (TS guidance)',
    migrated: true,
    coveredBy:
      'engine deny/rejected-suffix tests + runner passes usesWorkerRejectionGuidance from agentId',
  },
  {
    domain: 'usage: cache-read double counting (aimux semantics)',
    migrated: true,
    coveredBy: 'engine transcript_usage_does_not_add_cached_tokens_twice',
  },
  {
    domain: 'usage: cache-write component',
    migrated: false,
    gap:
      'not implemented — Rust usage events do not carry aimux cache-write tokens; ' +
      'runner reports inputCacheCreation: 0',
  },
  {
    domain: 'turn.ended failed error payload includes name',
    migrated: true,
    coveredBy:
      'rustEngineTurnRunner.test.ts (failed-turn error + turn.ended.error carry name/retryable — DimiErrorPayload parity)',
  },
  {
    domain: 'bus turn.step.completed usage shape (TS TokenUsage, P0-1)',
    migrated: true,
    coveredBy:
      'rustEngineTurnRunner.test.ts (bus turn.step.completed.usage is four-component; transcript projector contract)',
  },
  {
    domain: 'turn.started prompt stripped for non-displayable origins',
    migrated: true,
    coveredBy:
      'rustEngineTurnRunner.test.ts (task-origin turn.started has no prompt — isDisplayablePromptOrigin parity)',
  },
  {
    domain: 'external hooks Stop hook (onDidFinishStep continuation)',
    migrated: true,
    coveredBy:
      'rustEngineTurnRunner.test.ts (Stop hook fires after a non-tool step; continuation steers/launches)',
  },
  {
    domain: 'tool-call step assistant text preserved in context',
    migrated: true,
    coveredBy: 'engine tool_call_step_preserves_assistant_text_in_next_request (P1-4)',
  },
  {
    domain: 'runner usage reset between steps',
    migrated: true,
    coveredBy: 'rustEngineTurnRunner.test.ts (usage-less step gets zeros, not the previous step)',
  },
  {
    domain: 'thinking/text flush ordering',
    migrated: true,
    coveredBy: 'rustEngineTurnRunner.test.ts (parts recorded in stream order, not merged think-before-text)',
  },
  {
    domain: 'dead sync AgentTool removal',
    migrated: true,
    coveredBy: 'tool.rs no longer carries the sync AgentTool (bridge uses AsyncAgentTool); AgentTasks kept',
  },
];
