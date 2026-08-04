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
    coveredBy: 'PENDING — runner-level truncation test to be added',
  },
  {
    domain: 'PreToolUse/PostToolUse external hooks',
    migrated: false,
    gap: 'runner triggers hooks for EXTERNAL tools only; native tools (Bash/Agent/WaitFor) still bypass them — user-configured veto hooks do not apply to Bash',
  },
  {
    domain: 'tool dedupe (toolDedupe dup detection)',
    migrated: false,
    gap: 'not implemented — repeated identical calls execute twice',
  },
  {
    domain: 'activeToolNames filtering (policy [tools]/disabled)',
    migrated: false,
    gap: 'not implemented — the model sees every registered tool',
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
    gap: 'not implemented — compaction triggers on a plain estimate',
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
    migrated: false,
    gap: 'engine hardcodes TurnOrigin::User',
  },
  {
    domain: 'interrupted-step streamed text lands in context',
    migrated: false,
    gap: 'runner flushParts only fires on turn.step.completed; error/cancel paths lose partial text',
  },
  {
    domain: 'external tool updates streamed as tool.progress',
    migrated: false,
    gap: 'runner completes external calls with updates: []',
  },
  {
    domain: 'worker rejection/cancel message suffix (TS guidance)',
    migrated: false,
    gap: 'engine reject text lacks the TS "Try a different approach…" suffix',
  },
  {
    domain: 'usage: cache-read double counting (aimux semantics)',
    migrated: false,
    gap: 'input_tokens may double-count cached tokens — needs aimux verification',
  },
  {
    domain: 'turn.ended failed error payload includes name',
    migrated: false,
    gap: 'runner error bus event lacks name/stack fields',
  },
  {
    domain: 'runner usage reset between steps',
    migrated: false,
    gap: 'step.end without usage inherits the previous step usage',
  },
  {
    domain: 'thinking/text flush ordering',
    migrated: false,
    gap: 'runner flushes think before text regardless of stream order',
  },
  {
    domain: 'dead sync AgentTool removal',
    migrated: false,
    gap: 'tool.rs still carries the unused synchronous AgentTool (bridge uses AsyncAgentTool)',
  },
];
