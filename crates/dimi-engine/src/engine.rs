//! Turn orchestration core — the M3 swap-in socket for the TS loop domain.
//!
//! `Engine::run_turn` reproduces the TS `loopService.run` minimal closed
//! loop: step loop (LLM request → stream → tool execution → next step),
//! max-steps control, usage accumulation and the exact engine event stream
//! the transcript projection layer consumes. Context assembly stays on the
//! TS side (slice 1): the engine receives the assembled messages and appends
//! tool results as the turn progresses.
//!
//! Slice 2 adds the permission policy chain: before each tool call the
//! engine evaluates the policy (mode / whitelist / user rules / session
//! history) and either executes, denies (fixed error text) or pauses the
//! turn for an approval (`TurnProgress::NeedsApproval`); `TurnSession::resume`
//! continues with the user's decision.
//!
//! Effect boundaries are injected (no DI container — plain trait objects):
//! `LlmClient` for models and `ToolExecutor` for tools.

use std::time::Instant;

use dimi_wire::model::TranscriptUsage;
#[cfg(test)]
use dimi_wire::model::TurnOrigin;

use crate::dedupe::{DedupeCheck, DedupeState};
use crate::events::{EngineEvent, FinishReason};
use crate::llm::{ChatRequest, LlmClient, LlmStreamEvent};
use crate::permission::{
    ApprovalDecision, ApprovalRequest, PolicyConfig, PolicyDecision, PolicyInput, evaluate,
};
use crate::tool::{ToolCall, ToolContext, ToolExecutor, ToolResult};
use crate::types::{EngineTurnInput, LlmMessage, TurnEndReason, TurnOutcome};

/// Step-level retry budget for transient provider failures (TS
/// `loop_control.maxRetriesPerStep`, default 10 attempts per step).
pub const DEFAULT_MAX_RETRY_ATTEMPTS: u32 = 10;
/// Exponential backoff base (TS `retry.ts` `BASE_DELAY_MS`).
const RETRY_BASE_DELAY_MS: u64 = 500;
/// Backoff ceiling (TS `retry.ts` `MAX_DELAY_MS`).
const RETRY_MAX_DELAY_MS: u64 = 32_000;
/// Backoff multiplier (TS `retry.ts` `RETRY_FACTOR`).
const RETRY_FACTOR: u64 = 2;
/// Backoff jitter ratio (TS `retry.ts` `JITTER_FACTOR`).
const RETRY_JITTER: f64 = 0.25;

/// Retry delay for attempt `attempt` (1-based) within a `max_attempts`
/// budget — 500ms ×2 ramp capped at 32s plus jitter, matching the TS
/// `retryBackoffDelays` curve.
fn retry_backoff_delay(attempt: u32, max_attempts: u32) -> u64 {
    let count = max_attempts.saturating_sub(1).max(1);
    let index = (attempt - 1).min(count - 1);
    let base = (RETRY_BASE_DELAY_MS.saturating_mul(RETRY_FACTOR.pow(index)))
        .min(RETRY_MAX_DELAY_MS);
    base + ((base as f64) * retry_jitter_ratio()) as u64
}

/// Cheap jitter ratio in `[0, RETRY_JITTER]` derived from the sub-second
/// clock (no extra RNG dependency).
fn retry_jitter_ratio() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos as f64 / 1_000_000_000.0) * RETRY_JITTER
}

/// Engine configuration.
#[derive(Debug, Clone)]
pub struct Engine {
    /// Step limit; `None` = unlimited (mirrors `maxStepsPerTurn` unset).
    pub max_steps_per_turn: Option<u32>,
    /// Step-level retry budget for transient provider failures; `None` =
    /// `DEFAULT_MAX_RETRY_ATTEMPTS`.
    pub max_retries_per_step: Option<u32>,
    /// Shell for Bash tool execution (default `/bin/sh`).
    pub shell: String,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            max_steps_per_turn: None,
            max_retries_per_step: None,
            shell: dimi_exec::env::default_shell(),
        }
    }
}

/// How a step ended and what the outer loop should do next.
#[derive(Debug, Clone, PartialEq)]
enum StepDisposition {
    /// No tool calls — the turn is complete; carries the provider's finish
    /// reason (truncated/filtered responses change the turn outcome).
    Complete { finish: FinishReason },
    /// Tool calls to run through the policy gate, plus the step's assistant
    /// text (TS parity: an assistant message may carry BOTH text and tool
    /// calls — the text must reach the next request's context).
    Continue { calls: Vec<ToolCall>, text: String },
}

/// Accumulates usage across steps; per-step usage is reported in
/// `turn.step.completed` (the projection layer folds step usages into the
/// turn header).
#[derive(Debug, Clone, Default)]
pub(crate) struct UsageAccumulator {
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
}

impl UsageAccumulator {
    fn add(&mut self, event: &LlmStreamEvent) {
        if let LlmStreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            prompt_tokens_details,
            completion_tokens_details,
            ..
        } = event
        {
            self.prompt_tokens += prompt_tokens.unwrap_or(0);
            self.completion_tokens += completion_tokens.unwrap_or(0);
            if let Some(details) = prompt_tokens_details {
                self.cached_tokens += details.cached_tokens.unwrap_or(0);
                self.cache_write_tokens += details.cache_write_tokens.unwrap_or(0);
                self.reasoning_tokens += details.reasoning_tokens.unwrap_or(0);
            }
            if let Some(details) = completion_tokens_details {
                self.reasoning_tokens += details.reasoning_tokens.unwrap_or(0);
            }
        }
    }

    fn transcript_usage(&self) -> Option<TranscriptUsage> {
        if self.prompt_tokens == 0
            && self.completion_tokens == 0
            && self.cached_tokens == 0
            && self.cache_write_tokens == 0
        {
            return None;
        }
        Some(TranscriptUsage {
            // aimux's input_tokens.total already includes cache-read and
            // cache-write tokens; adding either breakdown would count them
            // twice.
            input_tokens: Some(self.prompt_tokens as i64),
            output_tokens: Some(self.completion_tokens as i64),
            cached_tokens: Some(self.cached_tokens as i64),
            cache_write_tokens: (self.cache_write_tokens > 0)
                .then_some(self.cache_write_tokens as i64),
            cost: None,
        })
    }
}

/// The tool-result text the TS projector inserts when a tool exchange is
/// unresolved at compaction time (parity with contextProjectorService's
/// `TOOL_INTERRUPTED_TEXT`).
const TOOL_INTERRUPTED_TEXT: &str =
    "Tool result is not available in the current context. Do not assume the tool completed successfully.";

/// The rejection-guidance suffix TS appends to deny / approval-rejection
/// messages for subagent/worker turns (`toolApprovalService.ts`
/// `formatDenyMessage` / `formatApprovalRejectionMessage` when
/// `scopeContext.agentId !== 'main'`). Byte-for-byte parity (leading space
/// and em-dash included — the suffix is appended directly after the base
/// message with no separator).
const WORKER_REJECTION_GUIDANCE_SUFFIX: &str =
    " Try a different approach — don't retry the same call, don't attempt to bypass the restriction.";

/// Close unresolved tool exchanges in a message list before sending it to the
/// provider: an assistant message whose tool_calls never got a result would
/// otherwise be sent as a dangling exchange. Each missing result is filled
/// with a synthetic tool message right after the assistant message that made
/// the call (TS contextProjector parity).
///
/// Beyond plain missing results this also repairs *interleaving*: a user
/// message (async notification / steer) that landed between an assistant
/// `tool_calls` and its tool result breaks the strict adjacency DeepSeek /
/// OpenAI enforce ("assistant tool_calls must be followed by tool messages").
/// Mirroring TS `AgentContextProjectorService.project` slot semantics, every
/// assistant tool_call opens a slot right after the assistant and the real
/// tool result is written back into that slot, so a foreign message is
/// reordered *after* the result instead of splitting the exchange.
fn close_unresolved_tool_exchanges(messages: &mut Vec<LlmMessage>) {
    struct Slot {
        index: usize,
        foreign_between: bool,
    }
    let mut slots: std::collections::HashMap<String, Slot> = std::collections::HashMap::new();
    let mut out: Vec<LlmMessage> = Vec::with_capacity(messages.len());
    for message in messages.drain(..) {
        if message.role == "tool" {
            let written_back = message
                .tool_call_id
                .as_ref()
                .and_then(|id| slots.remove(id));
            match written_back {
                Some(slot) => {
                    // Real result lands in the slot right after the assistant
                    // that made the call; any foreign message that arrived in
                    // between is naturally reordered after the result.
                    out[slot.index] = message;
                }
                None => {
                    // No open slot: orphan / duplicate result — drop it (TS
                    // `orphan_tool_result_dropped` / dedupe parity). A tool
                    // message with no preceding assistant call is invalid for
                    // strict providers.
                }
            }
            continue;
        }
        // Foreign message (user / assistant): every open exchange was
        // interrupted by it; results written back later reorder past it.
        for slot in slots.values_mut() {
            slot.foreign_between = true;
        }
        let is_assistant = message.role == "assistant";
        let call_ids: Vec<String> = message
            .tool_calls
            .iter()
            .flatten()
            .map(|call| call.id.clone())
            .collect();
        out.push(message);
        if is_assistant {
            for id in call_ids {
                if slots.contains_key(&id) {
                    // Re-declared call id: the earlier slot keeps its
                    // placeholder (TS `tool_result_synthesized` on reopen).
                    continue;
                }
                let slot_index = out.len();
                slots.insert(
                    id.clone(),
                    Slot {
                        index: slot_index,
                        foreign_between: false,
                    },
                );
                out.push(LlmMessage {
                    role: "tool".to_string(),
                    content: serde_json::Value::String(TOOL_INTERRUPTED_TEXT.to_string()),
                    name: None,
                    tool_call_id: Some(id),
                    tool_calls: None,
                    tools: None,
                    reasoning: None,
                    origin: None,
                });
            }
        }
    }
    // Every still-open slot keeps its placeholder: the synthetic interrupted
    // tool message already sits right after the assistant that made the call
    // (TS `tool_result_synthesized` at end of projection).
    *messages = out;
}

impl Engine {
    /// Run one turn to completion. Approvals are not wired in this
    /// convenience entry: if the policy asks, the tool call is denied
    /// (never hangs) — the session API (`TurnSession`) exposes the
    /// pause/resume flow for interactive use.
    pub async fn run_turn(
        &self,
        input: &EngineTurnInput,
        llm: &dyn LlmClient,
        tools: &dyn ToolExecutor,
        policy: &PolicyConfig,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnOutcome {
        let mut session = TurnSession::new(input.clone());
        Self::drive_turn(&mut session, llm, tools, policy, on_event).await
    }

    /// Run a turn with a steering channel: user messages pushed into `steer`
    /// while the turn runs are drained into the next step's request (the TS
    /// "prompt while busy" steer semantics). Same completion driver as
    /// [`Engine::run_turn`] — only the session construction differs.
    pub async fn run_turn_with_steer(
        &self,
        input: &EngineTurnInput,
        steer: std::sync::Arc<std::sync::Mutex<Vec<LlmMessage>>>,
        llm: &dyn LlmClient,
        tools: &dyn ToolExecutor,
        policy: &PolicyConfig,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnOutcome {
        let mut session = TurnSession::with_steer(input.clone(), Some(steer));
        Self::drive_turn(&mut session, llm, tools, policy, on_event).await
    }

    /// Shared completion driver: run the turn, resolving any approval request
    /// by denying it (no resolver is wired — never hang).
    async fn drive_turn(
        session: &mut TurnSession,
        llm: &dyn LlmClient,
        tools: &dyn ToolExecutor,
        policy: &PolicyConfig,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnOutcome {
        match session.run(llm, tools, policy, on_event).await {
            TurnProgress::Completed(outcome) => outcome,
            TurnProgress::NeedsApproval(_) => {
                // No resolver wired: deny by default (never hang).
                match session
                    .resume(
                        ApprovalDecision::Rejected { feedback: None },
                        llm,
                        tools,
                        policy,
                        on_event,
                    )
                    .await
                {
                    TurnProgress::Completed(outcome) => outcome,
                    TurnProgress::NeedsApproval(_) => {
                        // Another approval surfaced — deny it too, then finish.
                        match session
                            .resume(
                                ApprovalDecision::Rejected { feedback: None },
                                llm,
                                tools,
                                policy,
                                on_event,
                            )
                            .await
                        {
                            TurnProgress::Completed(outcome) => outcome,
                            // A third approval surfaced — fail with the
                            // session's real step count (P2-12 review: a
                            // hardcoded 0 under-reported progress).
                            TurnProgress::NeedsApproval(_) => TurnOutcome {
                                status: TurnEndReason::Failed,
                                steps: session.steps,
                                error: Some("approval pending without a resolver".to_string()),
                                error_code: Some("APPROVAL_PENDING".to_string()),
                                truncated: None,
                            },
                        }
                    }
                }
            }
        }
    }
}

/// Cooperative cancellation for a running turn: `cancel()` flips the flag and
/// stores a notify permit; the run loop checks the flag at step boundaries and
/// the LLM/tool awaits race `cancelled()`. `notify_one` keeps a permit when no
/// waiter is parked, so a cancel landing between the flag check and the await
/// is never missed.
///
/// The signal also carries an optional stop reason (TaskStop's custom reason,
/// e.g. "user abort"): the per-task settle reads it so the wire
/// `task.terminated` stopReason matches the task-service entry instead of the
/// hardcoded fallback (TS TaskStop parity).
#[derive(Debug, Default)]
pub struct CancelSignal {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    notify: std::sync::Arc<tokio::sync::Notify>,
    reason: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl CancelSignal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel without a stop reason (turn cancel / session close): the
    /// task settle falls back to "Stopped by TaskStop".
    pub fn cancel(&self) {
        self.cancel_with_reason(None);
    }

    /// Cancel carrying the TaskStop reason. The first reason wins (a signal
    /// is flipped once; later cancels cannot overwrite the recorded reason).
    /// An empty reason is treated as "no reason" — the settle then reports
    /// the "Stopped by TaskStop" fallback instead of an empty `error` on the
    /// wire (mirrors `taskService.normalizeReason`, which drops empty
    /// strings).
    pub fn cancel_with_reason(&self, reason: Option<String>) {
        if let Some(reason) = reason {
            if !reason.is_empty() {
                let mut slot = self.reason.lock().unwrap_or_else(|p| p.into_inner());
                if slot.is_none() {
                    *slot = Some(reason);
                }
            }
        }
        self.flag.store(true, std::sync::atomic::Ordering::Relaxed);
        self.notify.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The stop reason recorded at cancel time (`None` when the signal was
    /// cancelled without a reason — the settle then reports the default).
    pub fn reason(&self) -> Option<String> {
        self.reason
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Resolves once the turn has been cancelled (immediately if already so).
    pub async fn cancelled(&self) {
        if !self.is_cancelled() {
            self.notify.notified().await;
        }
    }
}

/// One in-flight turn: messages, step counter and the pending approval (if
/// the policy asked and the turn paused).
pub struct TurnSession {
    input: EngineTurnInput,
    messages: Vec<LlmMessage>,
    steps: u32,
    /// Consecutive transient provider failures on the current step; reset
    /// when any step succeeds (TS stepRetry `failedAttempts` parity).
    retry_attempts: u32,
    started_at: Instant,
    pending: Option<PendingApproval>,
    /// Steering messages injected while the turn runs (async subagent
    /// semantics): drained into the request assembly before each step.
    steer: Option<std::sync::Arc<std::sync::Mutex<Vec<LlmMessage>>>>,
    /// Token count right after the last compaction (TS
    /// `lastCompactedTokenCount`): while the estimate has not grown past it,
    /// compaction is skipped — prevents immediate re-compaction loops when
    /// the summary message itself approaches the window.
    last_compacted_tokens: Option<u64>,
    /// Tool-call dedupe state (TS `AgentToolDedupeService`): same-step
    /// suppression + cross-step repeat reminders. Per-turn by construction —
    /// a fresh session starts with a cleared streak.
    dedupe: DedupeState,
    /// Cooperative cancellation (RPC cancel → engine).
    cancel: std::sync::Arc<CancelSignal>,
    /// The signal for the currently executing step. The bridge keeps the
    /// handle so a step can be interrupted without cancelling the turn.
    active_step_cancel: std::sync::Arc<
        std::sync::Mutex<Option<std::sync::Arc<CancelSignal>>>,
    >,
    /// Set once the turn has ended (every finish path): a steer racing the
    /// teardown — between the engine's final `has_pending_steer()` check and
    /// the runner clearing its session — must be refused instead of landing
    /// in a queue that is never drained again (the TS runner then starts a
    /// new turn with the input).
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// A tool call waiting for the user's approval decision.
pub struct PendingApproval {
    pub request: ApprovalRequest,
    pub call: ToolCall,
    /// The pending call's position inside `batch` — where the resumed batch
    /// continues (siblings before it already ran; ones after it still must).
    pub index: usize,
    /// The full assistant-message batch this call came from — carried across
    /// the approval pause so a resumed call still sees its same-round
    /// siblings and the batch continues after the pending call (TS
    /// `ToolResolutionContext.toolCalls` parity; AllDone's mixed-use guard
    /// needs the whole original batch).
    pub batch: Vec<ToolCall>,
    /// The engine step that owns this batch (the paused step): after the
    /// resumed batch finishes, the step must still emit its
    /// `TurnStepCompleted` (P2-3 review — TS loop parity, the approval
    /// round-trip lives inside the step's tool phase).
    pub step: u32,
    /// The step's LLM usage accumulated before the pause, carried so the
    /// resumed step's `TurnStepCompleted` reports the same usage the normal
    /// path would (TS `finishStep` parity).
    pub(crate) usage: UsageAccumulator,
    /// The step remains cancellable while the approval request is paused.
    pub(crate) step_cancel: std::sync::Arc<CancelSignal>,
}

/// Where the turn stands after `run` / `resume`.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnProgress {
    Completed(TurnOutcome),
    NeedsApproval(ApprovalRequest),
}

/// Internal control returned by a tool batch. A step cancellation continues
/// the same turn; a turn cancellation finishes it; approval returns to the
/// bridge so the user can resolve it.
#[derive(Debug)]
enum BatchControl {
    NeedsApproval(ApprovalRequest),
    StepCancelled,
    TurnCancelled,
}

#[derive(Debug)]
enum StepControl {
    StepCancelled,
    TurnCancelled,
}

impl TurnSession {
    pub fn new(input: EngineTurnInput) -> Self {
        Self::with_steer(input, None)
    }

    pub fn with_steer(
        input: EngineTurnInput,
        steer: Option<std::sync::Arc<std::sync::Mutex<Vec<LlmMessage>>>>,
    ) -> Self {
        Self::with_steer_and_cancel(
            input,
            steer,
            std::sync::Arc::new(CancelSignal::new()),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    }

    pub fn with_steer_and_cancel(
        input: EngineTurnInput,
        steer: Option<std::sync::Arc<std::sync::Mutex<Vec<LlmMessage>>>>,
        cancel: std::sync::Arc<CancelSignal>,
        finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self::with_steer_and_cancel_and_step_cancel(
            input,
            steer,
            cancel,
            finished,
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        )
    }

    pub fn with_steer_and_cancel_and_step_cancel(
        input: EngineTurnInput,
        steer: Option<std::sync::Arc<std::sync::Mutex<Vec<LlmMessage>>>>,
        cancel: std::sync::Arc<CancelSignal>,
        finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
        active_step_cancel: std::sync::Arc<
            std::sync::Mutex<Option<std::sync::Arc<CancelSignal>>>,
        >,
    ) -> Self {
        Self {
            input,
            messages: Vec::new(),
            steps: 0,
            retry_attempts: 0,
            started_at: Instant::now(),
            pending: None,
            steer,
            last_compacted_tokens: None,
            dedupe: DedupeState::default(),
            cancel,
            active_step_cancel,
            finished,
        }
    }

    /// Return the shared slot used by the bridge's synchronous `cancelStep`
    /// method. The slot is populated only while an engine step is active.
    pub fn active_step_cancel_handle(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<CancelSignal>>>> {
        std::sync::Arc::clone(&self.active_step_cancel)
    }

    fn set_active_step_cancel(&self, signal: Option<std::sync::Arc<CancelSignal>>) {
        *self
            .active_step_cancel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = signal;
    }

    fn clear_active_step_cancel(&self, expected: &std::sync::Arc<CancelSignal>) {
        let mut active = self
            .active_step_cancel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active
            .as_ref()
            .is_some_and(|current| std::sync::Arc::ptr_eq(current, expected))
        {
            *active = None;
        }
    }

    /// Atomically claim the active-step slot at a normal completion boundary.
    /// A concurrent `cancelStep` either cancels the signal before this lock is
    /// acquired (so the caller continues with an interrupted step), or sees
    /// the slot already removed and returns `false` because the step has
    /// completed. This closes the check/clear window around step completion.
    fn take_active_step_cancel(&self, expected: &std::sync::Arc<CancelSignal>) -> bool {
        let mut active = self
            .active_step_cancel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !active
            .as_ref()
            .is_some_and(|current| std::sync::Arc::ptr_eq(current, expected))
        {
            return expected.is_cancelled();
        }
        let cancelled = active
            .as_ref()
            .is_some_and(|current| current.is_cancelled());
        *active = None;
        cancelled
    }

    /// Run until the turn completes or a tool call needs approval.
    pub async fn run(
        &mut self,
        llm: &dyn LlmClient,
        tools: &dyn ToolExecutor,
        policy: &PolicyConfig,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnProgress {
        self.messages = self.input.messages.clone();
        // `turn.started` belongs to the run entry, not the step loop: the
        // loop is re-entered after an approval resume, and a second
        // `turn.started` would corrupt the transcript (P2-3 review fix).
        let turn_id = self.input.turn_id;
        let origin = self.input.origin.clone();
        let prompt = last_user_text(&self.input.messages);
        emit(
            on_event,
            EngineEvent::TurnStarted {
                turn_id,
                origin: origin.clone(),
                prompt,
            },
        );
        self.run_loop(llm, tools, policy, on_event).await
    }

    /// The turn's working messages (history + tool results + assistant
    /// output so far) — consumed by subagent resume to carry history.
    pub fn messages(&self) -> &[LlmMessage] {
        &self.messages
    }

    /// Replace the LLM-facing tool definitions for subsequent requests
    /// (the bridge re-syncs them from the registry before each step so
    /// tools registered mid-session are visible to the model).
    pub fn update_tools(&mut self, tools: Vec<crate::types::EngineTool>) {
        self.input.tools = tools;
    }

    /// Resume after an approval decision; returns the next progress state.
    pub async fn resume(
        &mut self,
        decision: ApprovalDecision,
        llm: &dyn LlmClient,
        tools: &dyn ToolExecutor,
        policy: &PolicyConfig,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnProgress {
        let Some(pending) = self.pending.take() else {
            // Nothing pending — finish as failed (defensive).
            self.mark_finished();
            return TurnProgress::Completed(TurnOutcome {
                status: TurnEndReason::Failed,
                steps: self.steps,
                error: Some("resume called without a pending approval".to_string()),
                error_code: Some("NO_PENDING_APPROVAL".to_string()),
                truncated: None,
            });
        };
        // P1-5 (review): the user already cancelled while the approval was
        // pending (TaskStop / session close) — do not execute the pending
        // call, continue the batch, or re-ask. Finish cancelled immediately
        // (TS parity: the step signal aborts the whole step). The paused
        // step still gets its `turn.step.interrupted` so the transcript does
        // not leave it open (P2-6 review — TS emits interrupted for the
        // active step on cancel).
        if self.cancel.is_cancelled() {
            self.clear_active_step_cancel(&pending.step_cancel);
            self.emit_step_interrupted(pending.step, "aborted", None, on_event);
            return self.finish_turn_with_error(TurnEndReason::Cancelled, None, None, on_event);
        }
        if pending.step_cancel.is_cancelled() {
            self.retry_attempts = 0;
            self.clear_active_step_cancel(&pending.step_cancel);
            self.emit_step_interrupted(pending.step, "aborted", None, on_event);
            return self.run_loop(llm, tools, policy, on_event).await;
        }
        let pending_step = pending.step;
        let pending_usage = pending.usage.clone();
        let result = match decision {
            ApprovalDecision::Approved => {
                let ctx = self.tool_ctx(&pending.batch);
                emit_tool_call_started(&pending.call, self.input.turn_id, on_event);
                tokio::select! {
                    result = execute_tool(
                        pending.call.clone(),
                        tools,
                        &ctx,
                        pending.step,
                        self.input.turn_id,
                        on_event,
                    ) => result,
                    _ = self.cancel.cancelled() => {
                        tools.abort(&pending.call);
                        let result = aborted_tool_result(&pending.call);
                        emit_tool_result(&result, self.input.turn_id, on_event);
                        self.messages.push(tool_result_message(&result));
                        self.clear_active_step_cancel(&pending.step_cancel);
                        self.emit_step_interrupted(pending.step, "aborted", None, on_event);
                        return self.finish_turn_with_error(
                            TurnEndReason::Cancelled,
                            None,
                            None,
                            on_event,
                        );
                    }
                    _ = pending.step_cancel.cancelled() => {
                        tools.abort(&pending.call);
                        let result = aborted_tool_result(&pending.call);
                        emit_tool_result(&result, self.input.turn_id, on_event);
                        self.messages.push(tool_result_message(&result));
                        self.clear_active_step_cancel(&pending.step_cancel);
                        self.emit_step_interrupted(pending.step, "aborted", None, on_event);
                        return self.run_loop(llm, tools, policy, on_event).await;
                    }
                }
            }
            ApprovalDecision::Rejected { feedback } => {
                // The fold needs the started record before the result; the
                // started event lands after the approval decision, exactly
                // once per call (P2-2 review — TS toolExecutorService
                // dispatch parity).
                emit_tool_call_started(&pending.call, self.input.turn_id, on_event);
                // TS `formatApprovalRejectionMessage` parity: worker turns
                // append the rejection-guidance suffix to the message.
                let output = self.worker_rejection_output(match feedback {
                    Some(reason) if !reason.is_empty() => format!(
                        "Tool \"{}\" was not run because the user rejected the approval request. Reason: {}",
                        pending.call.name, reason
                    ),
                    _ => format!(
                        "Tool \"{}\" was not run because the user rejected the approval request.",
                        pending.call.name
                    ),
                });
                ToolResult {
                    tool_call_id: pending.call.id.clone(),
                    tool_name: pending.call.name.clone(),
                    output,
                    is_error: true,
                    stop_turn: false,
                    updates: vec![],
                }
            }
            ApprovalDecision::Cancelled => {
                emit_tool_call_started(&pending.call, self.input.turn_id, on_event);
                // The TS `formatApprovalRejectionMessage` prefix for a
                // cancelled decision also carries the worker guidance suffix.
                let output = self.worker_rejection_output(format!(
                    "Tool \"{}\" was not run because the approval request was cancelled.",
                    pending.call.name
                ));
                ToolResult {
                    tool_call_id: pending.call.id.clone(),
                    tool_name: pending.call.name.clone(),
                    output,
                    is_error: true,
                    stop_turn: false,
                    updates: vec![],
                }
            }
        };
        let result = self.dedupe.finalize_result(&pending.call.id, result);
        emit_tool_result(&result, self.input.turn_id, on_event);
        self.messages.push(tool_result_message(&result));
        if result.stop_turn {
            // The pending call itself stopped the turn (or the dedupe
            // force-stop fired): the remaining siblings never run and get
            // synthetic skipped results (P2-7 parity), so no tool_call
            // dangles in the next request.
            self.synthesize_skipped_siblings(&pending.batch, pending.index + 1, on_event);
            if self.interrupt_step_if_cancelled(&pending.step_cancel, pending_step, on_event) {
                return self.run_loop(llm, tools, policy, on_event).await;
            }
            self.dedupe.end_step();
            self.emit_step_completed(
                pending_step,
                &pending_usage,
                normalize_finish_reason(FinishReason::Completed),
                on_event,
            );
            return self.finish_turn(TurnEndReason::Completed, on_event);
        }
        // P1-1: an approval pauses the batch, it does not cancel the round —
        // after the pending call the remaining siblings still run (TS
        // parity). A sibling that needs approval pauses the turn again.
        match self
            .execute_batch(
                &pending.batch,
                pending.index + 1,
                tools,
                policy,
                on_event,
                self.input.turn_id,
                self.steps,
                &pending_usage,
                &pending.step_cancel,
            )
            .await
        {
            Ok(stop_turn) => {
                if self.take_active_step_cancel(&pending.step_cancel) {
                    self.emit_step_interrupted(pending_step, "aborted", None, on_event);
                    return self.run_loop(llm, tools, policy, on_event).await;
                }
                self.dedupe.end_step();
                if stop_turn {
                    self.emit_step_completed(
                        pending_step,
                        &pending_usage,
                        normalize_finish_reason(FinishReason::Completed),
                        on_event,
                    );
                    return self.finish_turn(TurnEndReason::Completed, on_event);
                }
                // The paused step finishes once its batch resolved (P2-3
                // review): without this the transcript keeps the step open
                // and the next `turn.step.started` has no matching
                // `turn.step.completed`.
                self.emit_step_completed(
                    pending_step,
                    &pending_usage,
                    normalize_finish_reason(FinishReason::ToolCalls),
                    on_event,
                );
                self.run_loop(llm, tools, policy, on_event).await
            }
            Err(BatchControl::NeedsApproval(request)) => TurnProgress::NeedsApproval(request),
            Err(BatchControl::StepCancelled) => {
                self.retry_attempts = 0;
                self.clear_active_step_cancel(&pending.step_cancel);
                self.emit_step_interrupted(pending_step, "aborted", None, on_event);
                self.run_loop(llm, tools, policy, on_event).await
            }
            Err(BatchControl::TurnCancelled) => {
                self.clear_active_step_cancel(&pending.step_cancel);
                self.emit_step_interrupted(pending_step, "aborted", None, on_event);
                self.finish_turn_with_error(TurnEndReason::Cancelled, None, None, on_event)
            }
        }
    }

    /// Emit `TurnStepCompleted` for a step whose batch was interrupted by an
    /// approval pause (TS `finishStep` parity): the normal path emits it
    /// right after `execute_batch` returns, and the resumed path must do the
    /// same so the transcript never sees an open step.
    fn emit_step_completed(
        &self,
        step: u32,
        usage: &UsageAccumulator,
        finish: &str,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) {
        emit(
            on_event,
            EngineEvent::TurnStepCompleted {
                turn_id: self.input.turn_id,
                step: step as i64,
                step_id: None,
                usage: usage.transcript_usage(),
                finish_reason: Some(finish.to_string()),
            },
        );
    }

    fn emit_step_interrupted(
        &self,
        step: u32,
        reason: &str,
        message: Option<String>,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) {
        emit(
            on_event,
            EngineEvent::TurnStepInterrupted {
                turn_id: self.input.turn_id,
                step: step as i64,
                step_id: None,
                reason: reason.to_string(),
                message,
            },
        );
    }

    /// Claim a step at the cancellation boundary and emit its interruption.
    /// The active-slot claim makes a concurrent cancel either win before a
    /// completion event or return false after the step has already ended.
    fn interrupt_step_if_cancelled(
        &self,
        signal: &std::sync::Arc<CancelSignal>,
        step: u32,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> bool {
        if !self.take_active_step_cancel(signal) {
            return false;
        }
        self.emit_step_interrupted(step, "aborted", None, on_event);
        true
    }

    fn tool_ctx(&self, calls: &[ToolCall]) -> ToolContext {
        ToolContext {
            // An empty `cwd` (e.g. a profile with no session cwd resolved)
            // would spawn with `current_dir("")` → ENOENT; fall back to the
            // spawn default (the engine process cwd via `"."`).
            cwd: match self.input.cwd.as_deref() {
                Some(cwd) if !cwd.is_empty() => cwd.to_string(),
                _ => ".".to_string(),
            },
            shell: self
                .input
                .shell
                .clone()
                .unwrap_or_else(dimi_exec::env::default_shell),
            tool_calls: calls.to_vec(),
        }
    }

    /// Append the worker rejection-guidance suffix when the input requests it
    /// (TS `usesWorkerRejectionGuidance` parity — `scopeContext.agentId !==
    /// 'main'`): the runner sets `uses_worker_rejection_guidance` for
    /// subagent/worker turns, and the deny / approval-rejection tool outputs
    /// then carry the same suffix TS appends for the model/user.
    fn worker_rejection_output(&self, output: String) -> String {
        if self.input.uses_worker_rejection_guidance {
            format!("{output}{WORKER_REJECTION_GUIDANCE_SUFFIX}")
        } else {
            output
        }
    }

    /// Execute the tool-call batch from `start` onward (approval-resume
    /// parity): returns `Ok(stop_turn)` when the batch finished, or an
    /// internal control when a sibling needs approval or the current step is
    /// cancelled. The caller owns the corresponding step/turn lifecycle
    /// events, so this helper never emits an interruption or ends the turn.
    ///
    /// The batch loop is shared between the initial step (start = 0) and an
    /// approval resume (start = pending.index + 1) so an approval pauses the
    /// round instead of cancelling it (P1-1), and a stop_turn inside the
    /// batch synthesizes skipped results for the unrun siblings (P2-7).
    async fn execute_batch(
        &mut self,
        calls: &[ToolCall],
        start: usize,
        tools: &dyn ToolExecutor,
        policy: &PolicyConfig,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
        turn_id: i64,
        step_number: u32,
        usage: &UsageAccumulator,
        step_cancel: &std::sync::Arc<CancelSignal>,
    ) -> Result<bool, BatchControl> {
        let mut stop_turn = false;
        for (index, call) in calls.iter().enumerate().skip(start) {
            // A cancel arriving between siblings stops the batch instead of
            // executing the rest or surfacing another approval request. The
            // caller emits the matching step lifecycle event.
            if self.cancel.is_cancelled() {
                return Err(BatchControl::TurnCancelled);
            }
            if step_cancel.is_cancelled() {
                return Err(BatchControl::StepCancelled);
            }
            let input = PolicyInput {
                mode: policy.mode,
                tool_name: call.name.clone(),
                args: call.arguments.clone(),
                rules: policy.rules.clone(),
                session_approved_patterns: policy.session_approved_patterns.clone(),
                match_arg: call
                    .arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                cwd: self.input.cwd.clone(),
                paths: crate::permission::extract_access_paths(&call.name, &call.arguments),
            };
            // Same-step dedupe (TS `onBeforeExecuteTool` veto): the check runs
            // before the policy gate — the veto fires before the allow
            // decision, so a same-step duplicate is suppressed even when the
            // original would be denied. The duplicate is announced but never
            // executed and settles with the original's finalized result.
            match self.dedupe.check(&call.id, &call.name, &call.arguments) {
                DedupeCheck::Duplicate { key } => {
                    let mut shared = self
                        .dedupe
                        .shared_result(&key)
                        .cloned()
                        .unwrap_or_else(|| ToolResult {
                            tool_call_id: call.id.clone(),
                            tool_name: call.name.clone(),
                            output: "Tool call deduplicated but original result was lost"
                                .to_string(),
                            is_error: true,
                            stop_turn: false,
                            updates: vec![],
                        });
                    // TS parity: the duplicate carries its own call id in the
                    // `tool.result` (the executor settles the dispatched call)
                    // but shares the original's output verbatim.
                    shared.tool_call_id = call.id.clone();
                    shared.tool_name = call.name.clone();
                    emit_tool_call_started(call, turn_id, on_event);
                    emit_tool_result(&shared, turn_id, on_event);
                    self.messages.push(tool_result_message(&shared));
                    if shared.stop_turn {
                        self.synthesize_skipped_siblings(calls, index + 1, on_event);
                        stop_turn = true;
                        break;
                    }
                    continue;
                }
                DedupeCheck::Original => {}
            }
            match evaluate(&input) {
                PolicyDecision::Approve => {
                    // Carry the step's full tool-call batch in the context so
                    // external tools can validate the round (e.g. the TS
                    // AllDone tool rejects a mixed batch).
                    let ctx = self.tool_ctx(calls);
                    emit_tool_call_started(call, turn_id, on_event);
                    let result = tokio::select! {
                        result = execute_tool(
                            call.clone(),
                            tools,
                            &ctx,
                            step_number,
                            turn_id,
                            on_event,
                        ) => result,
                        _ = self.cancel.cancelled() => {
                            tools.abort(call);
                            let result = aborted_tool_result(call);
                            emit_tool_result(&result, turn_id, on_event);
                            self.messages.push(tool_result_message(&result));
                            return Err(BatchControl::TurnCancelled);
                        }
                        _ = step_cancel.cancelled() => {
                            tools.abort(call);
                            let result = aborted_tool_result(call);
                            emit_tool_result(&result, turn_id, on_event);
                            self.messages.push(tool_result_message(&result));
                            return Err(BatchControl::StepCancelled);
                        }
                    };
                    // Cross-step repeat reminders (TS `finalizeResult`): the
                    // reminder is appended to the result output before the
                    // model sees it, and the result is recorded so same-step
                    // duplicates share it.
                    let result = self.dedupe.finalize_result(&call.id, result);
                    emit_tool_result(&result, turn_id, on_event);
                    self.messages.push(tool_result_message(&result));
                    if result.stop_turn {
                        // P2-7: the unrun siblings get synthetic error
                        // results so every tool_call in the assistant
                        // message has a matching tool result (TS
                        // toolExecutorService parity).
                        self.synthesize_skipped_siblings(calls, index + 1, on_event);
                        stop_turn = true;
                        break;
                    }
                }
                PolicyDecision::Deny { reason } => {
                    // The call still happened (the fold's tool.result needs
                    // the tool.call record).
                    emit_tool_call_started(call, turn_id, on_event);
                    // TS `formatDenyMessage` parity: worker turns append the
                    // rejection-guidance suffix to the deny message.
                    let output = self.worker_rejection_output(reason);
                    let result = ToolResult {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        output,
                        is_error: true,
                        stop_turn: false,
                        updates: vec![],
                    };
                    // Denied calls flow through the dedupe finalize like any
                    // other executed call (TS `onDidExecuteTool` fires for
                    // them too): reminders apply to the deny output.
                    let result = self.dedupe.finalize_result(&call.id, result);
                    emit_tool_result(&result, turn_id, on_event);
                    self.messages.push(tool_result_message(&result));
                }
                PolicyDecision::Ask => {
                    // The call is announced when it is dispatched after the
                    // approval decision (resume emits started + result for
                    // every decision — P2-2 parity with TS
                    // toolExecutorService), so no event is emitted at the
                    // pause itself.
                    let request = ApprovalRequest {
                        request_id: format!("approval-{turn_id}-{}", call.id),
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        action: Some("Run tool".to_string()),
                        display: Some(serde_json::json!({
                            "tool": call.name,
                            "args": call.arguments
                        })),
                        tool_input: Some(call.arguments.clone()),
                    };
                    self.pending = Some(PendingApproval {
                        request: request.clone(),
                        call: call.clone(),
                        batch: calls.to_vec(),
                        index,
                        step: step_number,
                        usage: usage.clone(),
                        step_cancel: std::sync::Arc::clone(step_cancel),
                    });
                    return Err(BatchControl::NeedsApproval(request));
                }
            }
        }
        // The caller closes the step only after atomically claiming the
        // active cancellation slot. A cancel that wins at the final
        // tool-result boundary must not advance the cross-step streak.
        Ok(stop_turn)
    }

    /// P2-7: synthesize an error tool result for every unrun sibling in the
    /// batch (a previous call stopped the turn), keeping the assistant
    /// message's tool_calls fully resolved (TS toolExecutorService parity).
    fn synthesize_skipped_siblings(
        &mut self,
        calls: &[ToolCall],
        after: usize,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) {
        for sibling in &calls[after..] {
            // P2-1 (review): announce the skipped call before its synthetic
            // result (TS `prepareSkippedToolCall` emits started first) — a
            // bare `tool.result` would be an orphan on the wire/transcript.
            emit_tool_call_started(sibling, self.input.turn_id, on_event);
            let skipped = ToolResult {
                tool_call_id: sibling.id.clone(),
                tool_name: sibling.name.clone(),
                // P2-5 (review): byte-for-byte TS `prepareSkippedToolCall`
                // output — the text is fed to the LLM, so divergence would
                // show up in model-visible behavior.
                output: "Tool skipped because a previous tool call stopped the turn.".to_string(),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            };
            emit_tool_result(&skipped, self.input.turn_id, on_event);
            self.messages.push(LlmMessage {
                role: "tool".to_string(),
                content: serde_json::Value::String(skipped.output),
                name: Some(skipped.tool_name.clone()),
                tool_call_id: Some(skipped.tool_call_id.clone()),
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            });
        }
    }

    fn has_pending_steer(&self) -> bool {
        self.steer
            .as_ref()
            .map(|steer| !steer.lock().unwrap().is_empty())
            .unwrap_or(false)
    }

    /// Mark the turn finished: any steer racing the teardown (between the
    /// engine's final steer check and the runner clearing its session) must
    /// be refused instead of landing in a queue that is never drained again.
    fn mark_finished(&self) {
        self.finished.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Estimate the next request's tokens: messages + tool definitions
    /// (mirrors `estimateRequestTokens` minus the injected system prompt,
    /// which the runner already folds into the first message).
    fn estimate_request_tokens(&self) -> u64 {
        let mut total = crate::context::estimate_tokens_for_messages(&self.messages);
        for tool in &self.input.tools {
            total += crate::context::estimate_tokens(&tool.name);
            total += crate::context::estimate_tokens(&tool.description);
            total += crate::context::estimate_tokens(
                &serde_json::to_string(&tool.args_schema).unwrap_or_default(),
            );
        }
        total
    }

    /// Run the compaction round: ask the LLM to summarize the history, then
    /// replace the working messages with the compacted shape. Empty summary
    /// responses drop the oldest message and retry (bounded); LLM failures
    /// fail soft (the turn continues without compacting). Returns whether a
    /// compaction actually happened (the overflow-recovery path uses it to
    /// decide between retry and failure).
    async fn compact(
        &mut self,
        llm: &dyn LlmClient,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> bool {
        let turn_id = self.input.turn_id;
        let tokens_before = crate::context::estimate_tokens_for_messages(&self.messages);
        let instruction = crate::compaction::compaction_instruction_message();

        let mut history = crate::compaction::strip_dynamic_tool_context(&self.messages);
        // Close unresolved tool exchanges before the summarizer sees the
        // history: an assistant message whose tool_calls never got a result
        // would otherwise be sent as a dangling exchange (TS
        // contextProjector parity — it inserts TOOL_INTERRUPTED_TEXT).
        close_unresolved_tool_exchanges(&mut history);
        let mut summary = String::new();
        for _ in 0..=crate::compaction::COMPACTION_MAX_SHRINK_ATTEMPTS {
            let mut messages = history.clone();
            messages.push(instruction.clone());
            let request = ChatRequest {
                messages,
                tools: None,
                model: Some(self.input.provider.model.clone()),
                thinking_effort: None,
            };
            match llm.stream_chat(&request).await {
                Ok(turn) => {
                    // A truncated summary (finish_reason = length) is not
                    // trustworthy: treat it like an empty summary and retry
                    // with a smaller prefix (TS CompactionTruncatedError
                    // parity — exhausted retries fail soft below).
                    let truncated = turn.events.iter().any(|event| {
                        matches!(
                            event,
                            LlmStreamEvent::Finish {
                                finish_reason: Some(reason),
                            } if reason == "length"
                        )
                    });
                    let text = turn.assistant.text.trim().to_string();
                    if !text.is_empty() && !truncated {
                        summary = text;
                        break;
                    }
                    if history.len() <= 1 {
                        break;
                    }
                    history.remove(0);
                }
                Err(_) => return false,
            }
        }
        if summary.is_empty() {
            return false;
        }

        let (messages, tokens_after) =
            crate::compaction::compacted_shape(&self.messages, &summary);
        let compacted_count = self.messages.len() as u64;
        self.last_compacted_tokens = Some(tokens_after);
        self.messages = messages;
        emit(
            on_event,
            EngineEvent::ContextCompacted {
                turn_id,
                summary,
                tokens_before,
                tokens_after,
                compacted_count,
            },
        );
        true
    }

    async fn run_loop(
        &mut self,
        llm: &dyn LlmClient,
        tools: &dyn ToolExecutor,
        policy: &PolicyConfig,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnProgress {
        let turn_id = self.input.turn_id;

        loop {
            // Cancellation (RPC cancel → engine): checked at every step
            // boundary; the in-flight LLM/tool awaits race it too.
            if self.cancel.is_cancelled() {
                return self.finish_turn_with_error(TurnEndReason::Cancelled, None, None, on_event);
            }

            // max-steps guard. TS parity: the guard fires before a step
            // begins (`runtime.current` is undefined), so TS never emits
            // `turn.step.interrupted` for it — emitting one would overwrite
            // the already-completed step's state on the transcript. The turn
            // ends failed via `turn.ended` alone.
            if let Some(max) = self.input.max_steps_per_turn {
                if max > 0 && self.steps >= max {
                    return self.finish_turn_with_error(
                        TurnEndReason::Failed,
                        Some(format!("Turn exceeded maxSteps={max}")),
                        Some("LOOP_MAX_STEPS_EXCEEDED".to_string()),
                        on_event,
                    );
                }
            }

            // Drain steering messages injected while the turn runs (async
            // subagent semantics): they land in the request assembled for
            // this step. The guard runs first so a max-steps failure never
            // folds a steer into a doomed turn.
            if let Some(steer) = &self.steer {
                let mut drained = steer.lock().unwrap();
                if !drained.is_empty() {
                    self.messages.append(&mut drained);
                }
            }

            self.steps += 1;
            let step_number = self.steps;
            // TS `onWillBeginStep` owns compaction and receives the step's
            // signal before it runs, so a step cancel during the summary
            // request skips this step instead of leaving the turn blocked in
            // compaction.
            self.dedupe.begin_step();
            let step_cancel = std::sync::Arc::new(CancelSignal::new());
            self.set_active_step_cancel(Some(std::sync::Arc::clone(&step_cancel)));

            // Full-history compaction (TS `fullCompaction` parity): when the
            // assembled request would cross the model window, run an LLM
            // summary round and replace the working messages before this
            // step's request is built. The last-compacted guard skips until
            // the estimate grows past the post-compaction size (TS
            // `lastCompactedTokenCount`), avoiding re-compaction loops.
            if let Some(max) = self.input.max_context_tokens {
                if max > 0 {
                    let estimated = self.estimate_request_tokens();
                    let already_compacted = self
                        .last_compacted_tokens
                        .is_some_and(|last| estimated <= last);
                    let trigger_ratio = self
                        .input
                        .compaction_trigger_ratio
                        .unwrap_or(crate::compaction::COMPACTION_TRIGGER_RATIO);
                    let reserved_context_size = self
                        .input
                        .reserved_context_size
                        .unwrap_or(crate::compaction::RESERVED_CONTEXT_SIZE);
                    if crate::compaction::should_compact_with_config(
                        estimated,
                        max,
                        trigger_ratio,
                        reserved_context_size,
                    ) && !already_compacted
                    {
                        let cancel = std::sync::Arc::clone(&self.cancel);
                        let step_cancel_for_compaction = std::sync::Arc::clone(&step_cancel);
                        let compaction = tokio::select! {
                            result = self.compact(llm, on_event) => Some(result),
                            _ = cancel.cancelled() => None,
                            _ = step_cancel_for_compaction.cancelled() => Some(false),
                        };
                        if compaction.is_none() {
                            self.clear_active_step_cancel(&step_cancel);
                            self.emit_step_interrupted(step_number, "aborted", None, on_event);
                            return self.finish_turn_with_error(
                                TurnEndReason::Cancelled,
                                None,
                                None,
                                on_event,
                            );
                        }
                        if step_cancel.is_cancelled() {
                            self.retry_attempts = 0;
                            self.clear_active_step_cancel(&step_cancel);
                            self.emit_step_interrupted(step_number, "aborted", None, on_event);
                            continue;
                        }
                    }
                }
            }

            // The TS loop runs `onWillBeginStep` (including compaction) before
            // publishing `turn.step.started`. Keep the transcript step closed
            // while compaction rewrites the context; the active signal is
            // already installed above, so a cancel from the started event is
            // still observed immediately once the real step begins.
            emit(
                on_event,
                EngineEvent::TurnStepStarted {
                    turn_id,
                    step: step_number as i64,
                    step_id: None,
                },
            );

            let mut usage = UsageAccumulator::default();
            let mut request_messages = match self.input.context_window {
                Some(window) if window > 0 => {
                    crate::context::project_window(&self.messages, window)
                }
                _ => self.messages.clone(),
            };
            // Every request must carry a tool result for each assistant
            // `tool_calls` entry, or strict providers (DeepSeek, OpenAI)
            // reject the request with HTTP 400. The working history can hold
            // a dangling exchange when a compaction/steer boundary split a
            // call from its result; close it before sending (TS
            // contextProjector parity, same as the compaction path).
            close_unresolved_tool_exchanges(&mut request_messages);
            let request = ChatRequest {
                messages: request_messages,
                tools: Some(aimux_tools_json(&self.input.tools)),
                model: Some(self.input.provider.model.clone()),
                thinking_effort: self.input.provider.thinking_effort.clone(),
            };

            let step_result: Result<
                Result<StepDisposition, crate::llm::LlmError>,
                StepControl,
            > = tokio::select! {
                result = execute_step(turn_id, llm, &request, &mut usage, on_event) => Ok(result),
                _ = self.cancel.cancelled() => Err(StepControl::TurnCancelled),
                _ = step_cancel.cancelled() => Err(StepControl::StepCancelled),
            };
            let disposition = match step_result {
                Err(StepControl::StepCancelled) => {
                    self.retry_attempts = 0;
                    self.clear_active_step_cancel(&step_cancel);
                    self.emit_step_interrupted(step_number, "aborted", None, on_event);
                    continue;
                }
                Err(StepControl::TurnCancelled) => {
                    self.clear_active_step_cancel(&step_cancel);
                    self.emit_step_interrupted(step_number, "aborted", None, on_event);
                    return self.finish_turn_with_error(
                        TurnEndReason::Cancelled,
                        None,
                        None,
                        on_event,
                    );
                }
                Ok(Ok(disposition)) => disposition,
                Ok(Err(error)) => {
                    if self.cancel.is_cancelled() {
                        self.clear_active_step_cancel(&step_cancel);
                        self.emit_step_interrupted(step_number, "aborted", None, on_event);
                        return self.finish_turn_with_error(
                            TurnEndReason::Cancelled,
                            None,
                            None,
                            on_event,
                        );
                    }
                    if step_cancel.is_cancelled() {
                        self.retry_attempts = 0;
                        self.take_active_step_cancel(&step_cancel);
                        self.emit_step_interrupted(step_number, "aborted", None, on_event);
                        continue;
                    }
                    // Context-overflow recovery (TS fullCompaction parity):
                    // the provider rejected the request as too large — run a
                    // compaction round and retry the step. The last-compacted
                    // guard bounds the loop: if the estimate has not grown,
                    // compact() is a no-op and the turn fails below.
                    if error.code.as_deref() == Some("CONTEXT_OVERFLOW")
                        && self.input.max_context_tokens.is_some()
                    {
                        let cancel = std::sync::Arc::clone(&self.cancel);
                        let step_cancel_for_compaction = std::sync::Arc::clone(&step_cancel);
                        match tokio::select! {
                            result = self.compact(llm, on_event) => Ok(result),
                            _ = cancel.cancelled() => Err(StepControl::TurnCancelled),
                            _ = step_cancel_for_compaction.cancelled() => Err(StepControl::StepCancelled),
                        } {
                            Ok(true) => continue,
                            Ok(false) => {}
                            Err(StepControl::TurnCancelled) => {
                                self.clear_active_step_cancel(&step_cancel);
                                self.emit_step_interrupted(step_number, "aborted", None, on_event);
                                return self.finish_turn_with_error(
                                    TurnEndReason::Cancelled,
                                    None,
                                    None,
                                    on_event,
                                );
                            }
                            Err(StepControl::StepCancelled) => {
                                self.retry_attempts = 0;
                                self.clear_active_step_cancel(&step_cancel);
                                self.emit_step_interrupted(step_number, "aborted", None, on_event);
                                continue;
                            }
                        }
                    }
                    // Transient provider failure step-level retry (TS
                    // stepRetry parity): connection / rate-limit / timeout /
                    // 5xx errors ride an exponential backoff ramp up to
                    // `max_retries_per_step`, then fail the turn. The retry
                    // re-executes the same LLM phase as a fresh step
                    // (consuming maxSteps budget, exactly like the TS
                    // `context.retry(driver, {at: "head"})` path).
                    if error.retryable {
                        let max_attempts = self
                            .input
                            .max_retries_per_step
                            .unwrap_or(DEFAULT_MAX_RETRY_ATTEMPTS)
                            .max(1);
                        if self.retry_attempts < max_attempts {
                            self.retry_attempts += 1;
                            let delay_ms = error
                                .retry_after_ms
                                .filter(|ms| *ms > 0)
                                .unwrap_or_else(|| {
                                    retry_backoff_delay(self.retry_attempts, max_attempts)
                                });
                            emit(
                                on_event,
                                EngineEvent::TurnStepRetrying {
                                    turn_id,
                                    step: step_number as i64,
                                    step_id: None,
                                    failed_attempt: self.retry_attempts as i64,
                                    next_attempt: self.retry_attempts as i64 + 1,
                                    max_attempts: max_attempts as i64,
                                    delay_ms: delay_ms as i64,
                                    error_name: error.code.clone(),
                                    error_message: error.message.clone(),
                                    status_code: None,
                                },
                            );
                            let sleep = tokio::time::sleep(std::time::Duration::from_millis(
                                delay_ms,
                            ));
                            tokio::pin!(sleep);
                            tokio::select! {
                                _ = &mut sleep => {}
                                _ = self.cancel.cancelled() => {
                                    self.clear_active_step_cancel(&step_cancel);
                                    self.emit_step_interrupted(
                                        step_number,
                                        "aborted",
                                        None,
                                        on_event,
                                    );
                                    return self.finish_turn_with_error(
                                        TurnEndReason::Cancelled,
                                        None,
                                        None,
                                        on_event,
                                    );
                                }
                                _ = step_cancel.cancelled() => {
                                    self.retry_attempts = 0;
                                    self.clear_active_step_cancel(&step_cancel);
                                    self.emit_step_interrupted(
                                        step_number,
                                        "aborted",
                                        None,
                                        on_event,
                                    );
                                    continue;
                                }
                            }
                            if self.cancel.is_cancelled() {
                                self.clear_active_step_cancel(&step_cancel);
                                self.emit_step_interrupted(step_number, "aborted", None, on_event);
                                return self.finish_turn_with_error(
                                    TurnEndReason::Cancelled,
                                    None,
                                    None,
                                    on_event,
                                );
                            }
                            if step_cancel.is_cancelled() {
                                self.retry_attempts = 0;
                                self.interrupt_step_if_cancelled(
                                    &step_cancel,
                                    step_number,
                                    on_event,
                                );
                                continue;
                            }
                            continue;
                        }
                        self.retry_attempts = 0;
                    }
                    let message = error.message.clone();
                    emit(
                        on_event,
                        EngineEvent::TurnStepInterrupted {
                            turn_id,
                            step: step_number as i64,
                            step_id: None,
                            reason: "error".to_string(),
                            message: Some(message.clone()),
                        },
                    );
                    return self.finish_turn_with_error(
                        TurnEndReason::Failed,
                        Some(message),
                        error.code.clone(),
                        on_event,
                    );
                }
            };

            if step_cancel.is_cancelled() {
                self.retry_attempts = 0;
                self.interrupt_step_if_cancelled(&step_cancel, step_number, on_event);
                continue;
            }

            match disposition {
                StepDisposition::Complete { finish } => {
                    // A successful step resets the transient-failure retry
                    // counter (TS stepRetry `onDidFinishStep` parity).
                    self.retry_attempts = 0;
                    let step_finish = normalize_finish_reason(finish).to_string();
                    // Completion-review injection (TS loopContinuationService
                    // parity): a tool-free step at/after the configured
                    // threshold must NOT end the turn with a plain text reply
                    // — the engine injects the review reminder into its
                    // working messages and keeps the turn alive so the model
                    // must call AllDone (TS appends the system reminder and
                    // enqueues a continuation step). Filtered / truncated /
                    // length finishes keep their failure/truncation paths (TS
                    // short-circuits `finishReason === 'filtered'` before the
                    // continuation).
                    if let Some(config) = &self.input.completion_review {
                        let reviewable = !matches!(
                            finish,
                            FinishReason::Filtered
                                | FinishReason::ContentFilter
                                | FinishReason::Truncated
                                | FinishReason::Length
                        );
                        if reviewable && self.steps >= config.min_steps {
                            if self.interrupt_step_if_cancelled(
                                &step_cancel,
                                step_number,
                                on_event,
                            ) {
                                continue;
                            }
                            self.emit_step_completed(
                                step_number,
                                &usage,
                                &step_finish,
                                on_event,
                            );
                            // P2-4 (review): wrap the reminder in
                            // `<system-reminder>` markers (TS
                            // `appendSystemReminder` parity). The wrap is
                            // idempotent: the runner passes the bare
                            // `COMPLETION_REVIEW_REMINDER`, tests may pass an
                            // already-wrapped one — both reach the LLM (and
                            // the mirror event) wrapped exactly once.
                            let reminder = wrap_system_reminder(&config.reminder);
                            self.messages.push(LlmMessage {
                                role: "user".to_string(),
                                content: serde_json::Value::String(reminder.clone()),
                                name: None,
                                tool_call_id: None,
                                tool_calls: None,
                                tools: None,
                                reasoning: None,
                                origin: None,
                            });
                            emit(
                                on_event,
                                EngineEvent::CompletionReviewInjected {
                                    turn_id,
                                    reminder,
                                },
                            );
                            continue;
                        }
                    }
                    if self.has_pending_steer() {
                        // A steer arrived while this step ran (async subagent
                        // finished). The step itself is complete, but the turn
                        // keeps going so the steering message reaches the LLM.
                        if self.interrupt_step_if_cancelled(&step_cancel, step_number, on_event) {
                            continue;
                        }
                        self.emit_step_completed(
                            step_number,
                            &usage,
                            &step_finish,
                            on_event,
                        );
                        continue;
                    }
                    // No pending steer: the turn is about to end. Mark it
                    // finished BEFORE the completion events are emitted, so a
                    // steer observed after them is refused (falls back to a
                    // new turn) instead of being dropped into a dead queue.
                    if self.interrupt_step_if_cancelled(&step_cancel, step_number, on_event) {
                        continue;
                    }
                    self.mark_finished();
                    self.emit_step_completed(
                        step_number,
                        &usage,
                        &step_finish,
                        on_event,
                    );
                    match finish {
                        // Provider-truncated response (length / max_tokens):
                        // the turn completes but is marked truncated (TS
                        // `result.truncated` parity).
                        FinishReason::Truncated | FinishReason::Length => {
                            return self.finish_turn_truncated(on_event);
                        }
                        // Provider safety block: the turn fails with the
                        // filtered code (TS ProviderFilteredError parity).
                        FinishReason::Filtered | FinishReason::ContentFilter => {
                            return self.finish_turn_with_error(
                                TurnEndReason::Failed,
                                Some("Provider safety policy blocked the response.".to_string()),
                                Some("PROVIDER_FILTERED".to_string()),
                                on_event,
                            );
                        }
                        _ => {
                            return self.finish_turn(TurnEndReason::Completed, on_event);
                        }
                    }
                }
                StepDisposition::Continue { calls, text } => {
                    // The assistant message carrying the tool calls must
                    // precede the tool results (providers reject a `tool`
                    // message without a preceding `tool_calls`; the TS loop
                    // pushes it the same way). P1-4 (adversarial review): the
                    // step's assistant TEXT is preserved alongside the calls —
                    // TS `appendResponseContent` keeps it in the context, so
                    // the next request must see the same text.
                    self.messages.push(LlmMessage {
                        role: "assistant".to_string(),
                        content: serde_json::Value::String(text),
                        name: None,
                        tool_call_id: None,
                        tool_calls: Some(
                            calls
                                .iter()
                                .map(|call| crate::types::LlmToolCall {
                                    id: call.id.clone(),
                                    call_type: Some("function".to_string()),
                                    function: crate::types::LlmToolCallFunction {
                                        name: call.name.clone(),
                                        arguments: serde_json::to_string(&call.arguments)
                                            .unwrap_or_default(),
                                    },
                                })
                                .collect(),
                        ),
                        tools: None,
                        reasoning: None,
                        origin: None,
                    });
                    let stop_turn = match self
                        .execute_batch(
                            &calls,
                            0,
                            tools,
                            policy,
                            on_event,
                            turn_id,
                            step_number,
                            &usage,
                            &step_cancel,
                        )
                        .await
                    {
                        Ok(stop_turn) => stop_turn,
                        Err(BatchControl::NeedsApproval(request)) => {
                            return TurnProgress::NeedsApproval(request);
                        }
                        Err(BatchControl::StepCancelled) => {
                            self.retry_attempts = 0;
                            self.clear_active_step_cancel(&step_cancel);
                            self.emit_step_interrupted(step_number, "aborted", None, on_event);
                            continue;
                        }
                        Err(BatchControl::TurnCancelled) => {
                            self.clear_active_step_cancel(&step_cancel);
                            self.emit_step_interrupted(step_number, "aborted", None, on_event);
                            return self.finish_turn_with_error(
                                TurnEndReason::Cancelled,
                                None,
                                None,
                                on_event,
                            );
                        }
                    };
                    // P2-4 (review): a successful tool-call step also resets
                    // the transient-failure retry budget (TS stepRetry resets
                    // on every successful step via `onDidFinishStep`).
                    self.retry_attempts = 0;
                    if stop_turn {
                        if self.interrupt_step_if_cancelled(&step_cancel, step_number, on_event) {
                            continue;
                        }
                        self.dedupe.end_step();
                        self.mark_finished();
                        emit(
                            on_event,
                            EngineEvent::TurnStepCompleted {
                                turn_id,
                                step: step_number as i64,
                                step_id: None,
                                usage: usage.transcript_usage(),
                                finish_reason: Some(
                                    normalize_finish_reason(FinishReason::Completed).to_string(),
                                ),
                            },
                        );
                        return self.finish_turn(TurnEndReason::Completed, on_event);
                    }
                    if self.interrupt_step_if_cancelled(&step_cancel, step_number, on_event) {
                        continue;
                    }
                    self.dedupe.end_step();
                    emit(
                        on_event,
                        EngineEvent::TurnStepCompleted {
                            turn_id,
                            step: step_number as i64,
                            step_id: None,
                            usage: usage.transcript_usage(),
                            finish_reason: Some(
                                normalize_finish_reason(FinishReason::ToolCalls).to_string(),
                            ),
                        },
                    );
                }
            }
        }
    }

    fn finish_turn(
        &mut self,
        status: TurnEndReason,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnProgress {
        self.finish_turn_with_error(status, None, None, on_event)
    }

    /// Complete the turn as truncated (provider length / max_tokens): the
    /// outcome stays `completed` with `truncated: true` (TS parity).
    fn finish_turn_truncated(
        &mut self,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnProgress {
        self.set_active_step_cancel(None);
        self.mark_finished();
        let outcome = TurnOutcome {
            status: TurnEndReason::Completed,
            steps: self.steps,
            error: None,
            error_code: None,
            truncated: Some(true),
        };
        emit(
            on_event,
            EngineEvent::TurnEnded {
                turn_id: self.input.turn_id,
                reason: "completed".to_string(),
                error: None,
                duration_ms: Some(self.started_at.elapsed().as_millis() as i64),
            },
        );
        TurnProgress::Completed(outcome)
    }

    fn finish_turn_with_error(
        &mut self,
        status: TurnEndReason,
        error: Option<String>,
        error_code: Option<String>,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnProgress {
        self.set_active_step_cancel(None);
        self.mark_finished();
        let outcome = TurnOutcome {
            status,
            steps: self.steps,
            error,
            error_code,
            truncated: None,
        };
        emit(
            on_event,
            EngineEvent::TurnEnded {
                turn_id: self.input.turn_id,
                reason: match status {
                    TurnEndReason::Completed => "completed".to_string(),
                    TurnEndReason::Cancelled => "cancelled".to_string(),
                    TurnEndReason::Failed => "failed".to_string(),
                    TurnEndReason::Blocked => "blocked".to_string(),
                },
                error: outcome
                    .error
                    .as_ref()
                    .map(|message| serde_json::json!({ "message": message, "code": outcome.error_code })),
                duration_ms: Some(self.started_at.elapsed().as_millis() as i64),
            },
        );
        TurnProgress::Completed(outcome)
    }
}

async fn execute_tool(
    call: ToolCall,
    tools: &dyn ToolExecutor,
    ctx: &ToolContext,
    step_number: u32,
    turn_id: i64,
    on_event: &mut (dyn FnMut(EngineEvent) + Send),
) -> ToolResult {
    let result = tools.execute_with_step(&call, ctx, step_number).await;
    for update in &result.updates {
        on_event(EngineEvent::ToolProgress {
            turn_id,
            tool_call_id: call.id.clone(),
            update: update.clone(),
        });
    }
    result
}

fn aborted_tool_result(call: &ToolCall) -> ToolResult {
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        output: format!("Tool \"{}\" was aborted", call.name),
        is_error: true,
        stop_turn: false,
        updates: vec![],
    }
}

/// Announce a tool call (TS `onToolCall`): the fold's `tool.result` needs
/// the matching `tool.call` record.
fn emit_tool_call_started(
    call: &ToolCall,
    turn_id: i64,
    on_event: &mut (dyn FnMut(EngineEvent) + Send),
) {
    on_event(EngineEvent::ToolCallStarted {
        turn_id,
        tool_call_id: call.id.clone(),
        name: call.name.clone(),
        args: Some(call.arguments.clone()),
        description: None,
    });
}

fn emit_tool_result(
    result: &ToolResult,
    turn_id: i64,
    on_event: &mut (dyn FnMut(EngineEvent) + Send),
) {
    on_event(EngineEvent::ToolResult {
        turn_id,
        tool_call_id: result.tool_call_id.clone(),
        output: result.output.clone(),
        is_error: Some(result.is_error),
        synthetic: None,
    });
}

/// The `tool` message the engine appends to its working messages after a
/// tool result (providers reject a `tool` message without a preceding
/// `tool_calls`; the TS loop pushes the same shape).
fn tool_result_message(result: &ToolResult) -> LlmMessage {
    LlmMessage {
        role: "tool".to_string(),
        content: serde_json::Value::String(result.output.clone()),
        name: Some(result.tool_name.clone()),
        tool_call_id: Some(result.tool_call_id.clone()),
        tool_calls: None,
        tools: None,
        reasoning: None,
        origin: None,
    }
}

/// Convert engine tool definitions into the JSON shape aimux's `Tool` enum
/// parses (`{type:"function", name, description, input_schema}`). The
/// OpenAI-nested shape (`{type:"function", function:{...}}`) fails aimux's
/// `#[serde(tag = "type")]` enum, so every tool was silently dropped
/// (`filter_map(...ok())`) and the request went out without tools — the
/// model then wrote tool calls as literal XML text.
fn aimux_tools_json(tools: &[crate::types::EngineTool]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.args_schema,
            })
        })
        .collect()
}

/// Execute one step's LLM phase: stream → parse → tool call list. The tool
/// phase is driven by the session loop (policy + approvals).
async fn execute_step(
    turn_id: i64,
    llm: &dyn LlmClient,
    request: &ChatRequest,
    usage: &mut UsageAccumulator,
    on_event: &mut (dyn FnMut(EngineEvent) + Send),
) -> Result<StepDisposition, crate::llm::LlmError> {
    let streamed = llm.stream_chat(request).await?;
    let mut provider_finish = None;
    for event in &streamed.events {
        match event {
            LlmStreamEvent::Text { delta } => {
                on_event(EngineEvent::AssistantDelta {
                    turn_id,
                    delta: delta.clone(),
                });
            }
            LlmStreamEvent::Thinking { delta } => {
                on_event(EngineEvent::ThinkingDelta {
                    turn_id,
                    delta: delta.clone(),
                });
            }
            LlmStreamEvent::ToolCall {
                tool_call_id,
                name,
                arguments_part,
            } => {
                on_event(EngineEvent::ToolCallDelta {
                    turn_id,
                    tool_call_id: tool_call_id.clone(),
                    name: name.clone(),
                    arguments_part: arguments_part.clone(),
                });
            }
            LlmStreamEvent::Usage { .. } => {
                usage.add(event);
            }
            LlmStreamEvent::Finish { finish_reason } => {
                provider_finish = finish_reason.clone();
            }
            LlmStreamEvent::Error { .. } => {}
        }
    }
    let assistant = &streamed.assistant;

    let mut tool_calls: Vec<ToolCall> = Vec::new();
    for call in &assistant.tool_calls {
        let args: serde_json::Value =
            serde_json::from_str(&call.function.arguments).unwrap_or(serde_json::Value::Null);
        tool_calls.push(ToolCall {
            id: call.id.clone(),
            name: call.function.name.clone(),
            arguments: args,
        });
    }

    // Map the provider's finish reason (TS `providerFinishReason` parity):
    // truncated/length responses complete the turn as truncated; a filtered
    // response fails it.
    let finish = match provider_finish.as_deref() {
        Some("tool_calls") => FinishReason::ToolCalls,
        Some("length") => FinishReason::Length,
        Some("max_tokens") => FinishReason::Truncated,
        Some("content_filter") => FinishReason::ContentFilter,
        Some("filtered") => FinishReason::Filtered,
        _ => FinishReason::Completed,
    };

    if tool_calls.is_empty() {
        return Ok(StepDisposition::Complete { finish });
    }
    Ok(StepDisposition::Continue {
        calls: tool_calls,
        text: assistant.text.clone(),
    })
}

fn emit(on_event: &mut dyn FnMut(EngineEvent), event: EngineEvent) {
    on_event(event);
}

/// TS `AgentSystemReminderService.appendSystemReminder` parity: reminders are
/// wrapped in `<system-reminder>` markers before they reach the LLM. The wrap
/// is idempotent — the reminder may already be wrapped (the engine tests
/// supply one; the runner's `COMPLETION_REVIEW_REMINDER` is bare text, which
/// gets wrapped here) — so an already-wrapped reminder is left untouched.
fn wrap_system_reminder(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("<system-reminder>") && trimmed.ends_with("</system-reminder>") {
        trimmed.to_string()
    } else {
        format!("<system-reminder>\n{trimmed}\n</system-reminder>")
    }
}

/// Last user message text — the `prompt` field of `turn.started`
/// (turnPromptText over the input parts).
fn last_user_text(messages: &[LlmMessage]) -> Option<String> {
    for message in messages.iter().rev() {
        if message.role == "user" {
            if let Some(text) = message.content.as_str() {
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

/// Normalize a finish reason to the transcript vocabulary — byte-compatible
/// with `loopService.normalizeFinishReason`.
pub fn normalize_finish_reason(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Completed => "end_turn",
        FinishReason::ToolCalls => "tool_use",
        FinishReason::Other => "other",
        FinishReason::Truncated => "max_tokens",
        FinishReason::Filtered => "filtered",
        FinishReason::Length => "length",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Cancelled => "cancelled",
        FinishReason::Interrupted => "interrupted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, ScriptedLlmClient};
    use crate::types::ProviderConfig;

    #[test]
    fn transcript_usage_does_not_add_cached_tokens_twice() {
        let mut usage = UsageAccumulator::default();
        usage.add(&LlmStreamEvent::Usage {
            prompt_tokens: Some(100),
            completion_tokens: Some(7),
            total_tokens: Some(107),
            prompt_tokens_details: Some(crate::llm::UsageDetails {
                cached_tokens: Some(20),
                cache_write_tokens: Some(7),
                reasoning_tokens: None,
            }),
            completion_tokens_details: None,
        });

        let transcript = usage.transcript_usage().expect("usage is present");
        assert_eq!(transcript.input_tokens, Some(100));
        assert_eq!(transcript.cached_tokens, Some(20));
        assert_eq!(transcript.cache_write_tokens, Some(7));
        assert_eq!(transcript.output_tokens, Some(7));
    }

    fn input(messages: Vec<LlmMessage>) -> EngineTurnInput {
        input_with_steps(messages, None)
    }

    fn input_with_steps(messages: Vec<LlmMessage>, max_steps: Option<u32>) -> EngineTurnInput {
        EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://example.test/v1".to_string(),
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: max_steps,
            cwd: Some(std::env::temp_dir().to_string_lossy().to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        }
    }

    fn user_message(text: &str) -> LlmMessage {
        LlmMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    fn event_names(events: &[EngineEvent]) -> Vec<String> {
        events
            .iter()
            .map(|event| {
                serde_json::to_value(event)
                    .unwrap()
                    .get("type")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn engine_tools_parse_as_aimux_function_tools() {
        // Regression: the engine used to build the OpenAI-nested shape
        // (`{type:"function", function:{name, description, parameters}}`),
        // which aimux's `#[serde(tag = "type")]` Tool enum cannot parse —
        // every tool was silently dropped, the request went out without
        // tools, and the model wrote tool calls as literal XML text. The
        // construction must yield values that parse as `Tool::Function`.
        let tools = vec![crate::types::EngineTool {
            name: "bash".to_string(),
            description: "Run a bash command".to_string(),
            args_schema: serde_json::json!({"type": "object", "properties": {}}),
        }];
        let values = aimux_tools_json(&tools);
        assert_eq!(values.len(), 1);
        for value in values {
            let parsed: Result<aimux_core::tool::Tool, _> = serde_json::from_value(value);
            assert!(
                parsed.is_ok(),
                "engine tool JSON must parse as aimux Tool: {:?}",
                parsed.err()
            );
            assert!(matches!(parsed.unwrap(), aimux_core::tool::Tool::Function(_)));
        }
    }

    #[test]
    fn tool_ctx_falls_back_when_input_cwd_is_empty() {
        // Regression: an empty `input.cwd` ("") — a profile whose session
        // cwd resolved to empty — flowed into ToolContext unchanged, and the
        // Bash tool spawned with `current_dir("")` → ENOENT on every call.
        // The tool context must fall back to "." so the shell runs in the
        // engine process cwd.
        let mut session = TurnSession::new(input(vec![]));
        session.input.cwd = Some(String::new());
        let ctx = session.tool_ctx(&[]);
        assert_eq!(ctx.cwd, ".");
        // A real cwd passes through untouched.
        session.input.cwd = Some("/workspace".to_string());
        let ctx = session.tool_ctx(&[]);
        assert_eq!(ctx.cwd, "/workspace");
    }

    #[tokio::test]
    async fn single_step_completes_with_text() {
        let engine = Engine::default();
        let llm = ScriptedLlmClient::once(vec![
            LlmStreamEvent::Text {
                delta: "Hello ".to_string(),
            },
            LlmStreamEvent::Text {
                delta: "world".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        let mut events = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let outcome = engine
            .run_turn(
                &input(vec![user_message("hi")]),
                &llm,
                &__bash,
                &crate::permission::PolicyConfig {
                    mode: crate::permission::PermissionMode::Auto,
                    rules: vec![],
                    session_approved_patterns: vec![],
                },
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Completed);
        assert_eq!(outcome.steps, 1);
        let names = event_names(&events);
        assert_eq!(
            names,
            vec![
                "turn.started",
                "turn.step.started",
                "assistant.delta",
                "assistant.delta",
                "turn.step.completed",
                "turn.ended"
            ]
        );
        // turn.started carries the prompt from the user message.
        let started = &events[0];
        let value = serde_json::to_value(started).unwrap();
        assert_eq!(value["prompt"], "hi");
        // step completed carries finishReason completed.
        let step_completed = serde_json::to_value(&events[4]).unwrap();
        assert_eq!(step_completed["finishReason"], "end_turn");
        // turn.ended reason completed.
        let ended = serde_json::to_value(&events[5]).unwrap();
        assert_eq!(ended["reason"], "completed");
    }

    #[tokio::test]
    async fn turn_started_carries_the_input_origin() {
        // TS parity: `turn.started` carries the input's `origin`
        // (PromptOrigin); the default is a user-origin turn.
        let engine = Engine::default();
        let llm = ScriptedLlmClient::once(vec![
            LlmStreamEvent::Text {
                delta: "hi".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        let __bash = crate::tool::BashTool::default();
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };

        // Custom origin: the event carries it verbatim.
        let mut custom_input = input(vec![user_message("hi")]);
        custom_input.origin = TurnOrigin::Task {
            task_id: dimi_wire::id::TaskId::new_unchecked("task-1".to_string()),
            payload: None,
        };
        let mut events = Vec::new();
        engine
            .run_turn(
                &custom_input,
                &llm,
                &__bash,
                &policy,
                &mut |event| events.push(event),
            )
            .await;
        let started = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(started["type"], "turn.started");
        assert_eq!(started["origin"]["kind"], "task");
        assert_eq!(started["origin"]["taskId"], "task-1");

        // Default input: a user-origin turn.
        let mut events = Vec::new();
        engine
            .run_turn(
                &input(vec![user_message("hi")]),
                &llm,
                &__bash,
                &policy,
                &mut |event| events.push(event),
            )
            .await;
        let started = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(started["origin"]["kind"], "user");
        assert!(started["origin"].get("payload").is_none());
    }

    #[tokio::test]
    async fn tool_call_step_preserves_assistant_text_in_next_request() {
        // P1-4 (adversarial review): TS keeps the step's assistant TEXT
        // alongside its tool calls in the context; the next request must see
        // the same assistant message (text + tool_calls), not an
        // empty-content stub.
        struct TextAndCallClient(std::sync::Arc<std::sync::Mutex<Vec<Vec<LlmMessage>>>>);
        #[async_trait::async_trait]
        impl LlmClient for TextAndCallClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<crate::llm::StreamedTurn, crate::llm::LlmError> {
                let mut recorded = self.0.lock().unwrap();
                recorded.push(request.messages.clone());
                let is_first = recorded.len() == 1;
                drop(recorded);
                if is_first {
                    let args = "{\"command\":\"echo hi\"}".to_string();
                    return Ok(crate::llm::StreamedTurn {
                        events: vec![
                            LlmStreamEvent::Text {
                                delta: "I will check".to_string(),
                            },
                            LlmStreamEvent::ToolCall {
                                tool_call_id: "call_1".to_string(),
                                name: Some("Bash".to_string()),
                                arguments_part: Some(args.clone()),
                            },
                            LlmStreamEvent::Finish {
                                finish_reason: Some("tool_calls".to_string()),
                            },
                        ],
                        assistant: crate::llm::AssistantTurn {
                            tool_calls: vec![crate::types::LlmToolCall {
                                id: "call_1".to_string(),
                                call_type: Some("function".to_string()),
                                function: crate::types::LlmToolCallFunction {
                                    name: "Bash".to_string(),
                                    arguments: args,
                                },
                            }],
                            text: "I will check".to_string(),
                            thinking: String::new(),
                        },
                    });
                }
                Ok(crate::llm::StreamedTurn {
                    events: vec![
                        LlmStreamEvent::Text {
                            delta: "done".to_string(),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("stop".to_string()),
                        },
                    ],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "done".to_string(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let llm = TextAndCallClient(std::sync::Arc::clone(&recorded));
        let __bash = crate::tool::BashTool::default();
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input(vec![user_message("go")]));
        let TurnProgress::Completed(outcome) = session
            .run(&llm, &__bash, &policy, &mut |_| {})
            .await
        else {
            panic!("turn must complete");
        };
        assert_eq!(outcome.status, TurnEndReason::Completed);
        let requests = recorded.lock().unwrap();
        assert!(requests.len() >= 2, "two LLM requests");
        // The second request's assistant message carries the text AND the call.
        let second = &requests[1];
        let assistant = second
            .iter()
            .find(|m| m.role == "assistant")
            .expect("assistant message in the second request");
        assert_eq!(
            assistant.content,
            serde_json::Value::String("I will check".to_string())
        );
        assert_eq!(
            assistant
                .tool_calls
                .as_ref()
                .map(|calls| calls.len())
                .unwrap_or(0),
            1
        );
    }

    #[test]
    fn input_json_parses_optional_origin_and_worker_guidance() {
        // The bridge deserializes `EngineTurnInput` straight from the
        // runner's JSON: absent fields fall back to a user origin + no worker
        // guidance, and the `{kind: 'task', taskId}` shape (TS PromptOrigin,
        // `TurnOrigin`'s serde in dimi-wire) is accepted — this pins the
        // contract the runner will send through `RustTurnSession`.
        let base = r#"{
            "turnId": 1,
            "messages": [],
            "provider": { "baseUrl": "http://example.test/v1", "apiKey": "k", "model": "m" }
        }"#;
        let input: EngineTurnInput = serde_json::from_str(base).unwrap();
        assert_eq!(input.origin, TurnOrigin::User { payload: None });
        assert!(!input.uses_worker_rejection_guidance);

        let custom: EngineTurnInput = serde_json::from_str(r#"{
            "turnId": 2,
            "messages": [],
            "provider": { "baseUrl": "http://example.test/v1", "apiKey": "k", "model": "m" },
            "origin": { "kind": "task", "taskId": "agent-1" },
            "usesWorkerRejectionGuidance": true
        }"#)
        .unwrap();
        assert!(matches!(custom.origin, TurnOrigin::Task { .. }));
        assert!(custom.uses_worker_rejection_guidance);
    }

    #[tokio::test]
    async fn tool_call_runs_bash_and_loops() {
        let engine = Engine::default();
        let llm = ScriptedLlmClient::new(vec![
            // Step 1: request a Bash tool call.
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo engine-test\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            // Step 2: model sees the tool result and completes.
            vec![
                LlmStreamEvent::Text {
                    delta: "done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let mut events = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let outcome = engine
            .run_turn(
                &input(vec![user_message("run a command")]),
                &llm,
                &__bash,
                &crate::permission::PolicyConfig {
                    mode: crate::permission::PermissionMode::Auto,
                    rules: vec![],
                    session_approved_patterns: vec![],
                },
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Completed);
        assert_eq!(outcome.steps, 2);
        let names = event_names(&events);
        assert!(names.contains(&"tool.call.started".to_string()));
        assert!(names.contains(&"tool.result".to_string()));
        let result_idx = names.iter().position(|name| name == "tool.result").unwrap();
        let result = serde_json::to_value(&events[result_idx]).unwrap();
        assert!(result["output"].as_str().unwrap().contains("engine-test"));
        assert_eq!(result["isError"], false);
        // Step 2 completed after the tool result.
        let step_completed_idx = names
            .iter()
            .position(|name| name == "turn.step.completed")
            .unwrap();
        let step_completed = serde_json::to_value(&events[step_completed_idx]).unwrap();
        assert_eq!(step_completed["finishReason"], "tool_use");
    }

    #[tokio::test]
    async fn max_steps_fails_the_turn() {
        let engine = Engine {
            max_steps_per_turn: Some(1),
            max_retries_per_step: None,
            shell: "/bin/sh".to_string(),
        };
        // The model always asks for a tool → step 2 would exceed maxSteps=1.
        let llm = ScriptedLlmClient::new(vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo x\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_2".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo y\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
        ]);
        let mut events = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let outcome = engine
            .run_turn(
                &input_with_steps(vec![user_message("loop")], Some(1)),
                &llm,
                &__bash,
                &crate::permission::PolicyConfig {
                    mode: crate::permission::PermissionMode::Auto,
                    rules: vec![],
                    session_approved_patterns: vec![],
                },
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Failed);
        assert_eq!(
            outcome.error_code.as_deref(),
            Some("LOOP_MAX_STEPS_EXCEEDED")
        );
        // TS parity: the max-steps failure happens before a step begins
        // (`runtime.current` is undefined in beginLoopStep), so TS never
        // emits `turn.step.interrupted` for it — emitting one here would
        // overwrite the already-completed step's state on the transcript.
        let names = event_names(&events);
        assert!(
            !names.contains(&"turn.step.interrupted".to_string()),
            "max_steps must not emit turn.step.interrupted: {names:?}"
        );
        // The turn still ends failed via turn.ended.
        assert!(names.contains(&"turn.ended".to_string()));
    }

    #[tokio::test]
    async fn tool_error_is_error_result() {
        let engine = Engine::default();
        let llm = ScriptedLlmClient::new(vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"exit 2\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![
                LlmStreamEvent::Text {
                    delta: "saw the error".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let mut events = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let outcome = engine
            .run_turn(
                &input(vec![user_message("fail")]),
                &llm,
                &__bash,
                &crate::permission::PolicyConfig {
                    mode: crate::permission::PermissionMode::Auto,
                    rules: vec![],
                    session_approved_patterns: vec![],
                },
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Completed);
        let result = events
            .iter()
            .find(|event| matches!(event, EngineEvent::ToolResult { .. }))
            .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["isError"], true);
        assert!(
            value["output"]
                .as_str()
                .unwrap()
                .contains("Command failed with exit code: 2.")
        );
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use crate::types::ProviderConfig;

    fn msg(role: &str, text: &str) -> LlmMessage {
        LlmMessage {
            role: role.to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    #[tokio::test]
    async fn context_window_projects_the_request_messages() {
        // A scripted client that records what it received.
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<LlmMessage>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<crate::llm::StreamedTurn, crate::llm::LlmError> {
                *self.0.lock().unwrap() = request.messages.clone();
                Ok(crate::llm::StreamedTurn {
                    events: vec![],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "".to_string(),
                        thinking: "".to_string(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<LlmMessage>>> = Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        let engine = Engine::default();
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![
                msg("system", "sys"),
                msg("user", "u1"),
                msg("assistant", "a1"),
                msg("user", "u2"),
                msg("assistant", "a2"),
                msg("user", "u3"),
            ],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: Some(3),
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let __bash = crate::tool::BashTool::default();
        engine
            .run_turn(&input, &llm, &__bash, &policy, &mut |_| {})
            .await;
        let sent = recorded.lock().unwrap();
        assert_eq!(sent.len(), 4); // system + tail 3
        assert_eq!(
            sent[0].content,
            serde_json::Value::String("sys".to_string())
        );
        assert_eq!(sent[1].content, serde_json::Value::String("u2".to_string()));
        assert_eq!(sent[3].content, serde_json::Value::String("u3".to_string()));
    }
}

#[cfg(test)]
mod steer_tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, StreamedTurn};
    use crate::types::ProviderConfig;

    #[tokio::test]
    async fn steering_messages_are_drained_into_the_next_request() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(
            Arc<Mutex<Vec<Vec<LlmMessage>>>>,
            Arc<Mutex<Vec<LlmMessage>>>,
        );
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                calls.push(request.messages.clone());
                if calls.len() == 1 {
                    // Simulate an async subagent completing while the first
                    // request is in flight: a steer lands mid-turn.
                    self.1.lock().unwrap().push(LlmMessage {
                        role: "user".to_string(),
                        content: serde_json::Value::String(
                            "steer: change direction".to_string(),
                        ),
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                        tools: None,
                        reasoning: None,
                        origin: None,
                    });
                }
                Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "".to_string(),
                        thinking: "".to_string(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Vec<LlmMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let steer: Arc<Mutex<Vec<LlmMessage>>> = Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded), Arc::clone(&steer));
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String("original".to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            }],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(2),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::with_steer(input, Some(steer));
        let __bash = crate::tool::BashTool::default();
        let progress = session
            .run(&llm, &__bash, &policy, &mut |_| {})
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)));
        let requests = recorded.lock().unwrap();
        // First request: original only. Second request: steer drained in.
        assert_eq!(requests.len(), 2);
        let second = &requests[1];
        let texts: Vec<&str> = second
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(texts.contains(&"steer: change direction"), "second request: {texts:?}");
    }
}

#[cfg(test)]
mod completion_review_tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, StreamedTurn};
    use crate::tool::BashTool;
    use crate::types::{CompletionReviewConfig, ProviderConfig};

    /// Records every request's messages and returns per-call scripted
    /// responses: the first `tool_steps` calls return a Bash tool call, the
    /// rest return a plain text reply (`filtered` turns the reply into a
    /// provider `content_filter` finish instead).
    struct RecordingClient {
        recorded: std::sync::Arc<std::sync::Mutex<Vec<Vec<LlmMessage>>>>,
        calls: std::sync::atomic::AtomicUsize,
        tool_steps: usize,
        filtered: bool,
    }

    impl RecordingClient {
        fn new(recorded: std::sync::Arc<std::sync::Mutex<Vec<Vec<LlmMessage>>>>, tool_steps: usize) -> Self {
            Self {
                recorded,
                calls: std::sync::atomic::AtomicUsize::new(0),
                tool_steps,
                filtered: false,
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for RecordingClient {
        async fn stream_chat(
            &self,
            request: &ChatRequest,
        ) -> Result<StreamedTurn, crate::llm::LlmError> {
            self.recorded.lock().unwrap().push(request.messages.clone());
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n < self.tool_steps {
                let id = format!("call_{n}");
                let args = "{\"command\":\"echo hi\"}".to_string();
                return Ok(StreamedTurn {
                    events: vec![
                        LlmStreamEvent::ToolCall {
                            tool_call_id: id.clone(),
                            name: Some("Bash".to_string()),
                            arguments_part: Some(args.clone()),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("tool_calls".to_string()),
                        },
                    ],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![crate::types::LlmToolCall {
                            id,
                            call_type: Some("function".to_string()),
                            function: crate::types::LlmToolCallFunction {
                                name: "Bash".to_string(),
                                arguments: args,
                            },
                        }],
                        text: String::new(),
                        thinking: String::new(),
                    },
                });
            }
            if self.filtered {
                return Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("content_filter".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: String::new(),
                        thinking: String::new(),
                    },
                });
            }
            Ok(StreamedTurn {
                events: vec![LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                }],
                assistant: crate::llm::AssistantTurn {
                    tool_calls: vec![],
                    text: "done".to_string(),
                    thinking: String::new(),
                },
            })
        }
    }

    fn review_input_with_reminder(
        messages: Vec<LlmMessage>,
        min_steps: u32,
        max_steps: Option<u32>,
        reminder: String,
    ) -> EngineTurnInput {
        EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: max_steps,
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            max_retries_per_step: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            completion_review: Some(CompletionReviewConfig {
                min_steps,
                reminder,
            }),
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        }
    }

    fn review_input(
        messages: Vec<LlmMessage>,
        min_steps: u32,
        max_steps: Option<u32>,
    ) -> EngineTurnInput {
        review_input_with_reminder(
            messages,
            min_steps,
            max_steps,
            "<system-reminder>\nreview now\n</system-reminder>".to_string(),
        )
    }

    fn policy_auto() -> crate::permission::PolicyConfig {
        crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        }
    }

    fn user_message(text: &str) -> LlmMessage {
        LlmMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    #[tokio::test]
    async fn tool_free_step_at_threshold_injects_reminder_and_keeps_the_turn_alive() {
        // 9 tool-call steps (1..9) + a text reply at step 10: the step count
        // crosses the threshold, so the engine must inject the reminder and
        // keep the turn alive for one more request (max_steps bounds the
        // scripted model that never calls AllDone).
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let llm = RecordingClient::new(std::sync::Arc::clone(&recorded), 9);
        let input = review_input(vec![user_message("complete the task")], 10, Some(11));
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = session
            .run(&llm, &BashTool::default(), &policy_auto(), &mut |event| {
                events.push(event);
            })
            .await;
        let requests = recorded.lock().unwrap();
        // Without the injection the turn would end after the step-10 text
        // reply (10 requests); the review holds it one request longer.
        assert_eq!(requests.len(), 11, "requests: {requests:?}");
        // The step-10 request must NOT carry the reminder; the step-11
        // request (the one assembled after the injection) must.
        let request_10 = &requests[9];
        let texts_10: Vec<&str> = request_10
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(
            !texts_10.iter().any(|t| t.contains("review now")),
            "step-10 request must not contain the reminder: {texts_10:?}"
        );
        let request_11 = &requests[10];
        let texts_11: Vec<&str> = request_11
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(
            texts_11.contains(&"<system-reminder>\nreview now\n</system-reminder>"),
            "step-11 request must contain the reminder: {texts_11:?}"
        );
        // The injection is announced on the event stream (the runner mirrors
        // it into the TS context from this event).
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::CompletionReviewInjected { .. })),
            "completion.review.injected event missing: {events:?}"
        );
        assert!(matches!(progress, TurnProgress::Completed(_)));
    }

    #[tokio::test]
    async fn bare_completion_review_reminder_is_wrapped_before_injection() {
        // P2-4 (review): the runner's `COMPLETION_REVIEW_REMINDER` is bare
        // text, but the TS `AgentSystemReminderService.appendSystemReminder`
        // wraps reminders in `<system-reminder>` markers before they reach
        // the LLM. The engine must wrap a bare configured reminder (and
        // leave an already-wrapped one untouched — see
        // `tool_free_step_at_threshold_injects_reminder_and_keeps_the_turn_alive`).
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let llm = RecordingClient::new(std::sync::Arc::clone(&recorded), 9);
        let input = review_input_with_reminder(
            vec![user_message("complete the task")],
            10,
            Some(11),
            "review now".to_string(),
        );
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = session
            .run(&llm, &BashTool::default(), &policy_auto(), &mut |event| {
                events.push(event);
            })
            .await;
        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 11, "requests: {requests:?}");
        let request_11 = &requests[10];
        let texts_11: Vec<&str> = request_11
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(
            texts_11.contains(&"<system-reminder>\nreview now\n</system-reminder>"),
            "step-11 request must carry the WRAPPED reminder: {texts_11:?}"
        );
        // The bare text must never be injected as a message on its own.
        assert!(
            !texts_11.iter().any(|t| *t == "review now"),
            "bare reminder must not be injected unwrapped: {texts_11:?}"
        );
        // The injected (wrapped) reminder is announced on the event stream.
        assert!(
            events.iter().any(|e| matches!(
                e,
                EngineEvent::CompletionReviewInjected { reminder, .. }
                    if reminder.contains("<system-reminder>")
            )),
            "injection event must carry the wrapped reminder: {events:?}"
        );
        assert!(matches!(progress, TurnProgress::Completed(_)));
    }

    #[tokio::test]
    async fn tool_free_step_below_threshold_ends_naturally_without_reminder() {
        // 8 tool-call steps + a text reply at step 9: 9 < 10, so the turn
        // ends at the text reply and no reminder is ever injected.
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let llm = RecordingClient::new(std::sync::Arc::clone(&recorded), 8);
        let input = review_input(vec![user_message("complete the task")], 10, None);
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let _progress = session
            .run(&llm, &BashTool::default(), &policy_auto(), &mut |event| {
                events.push(event);
            })
            .await;
        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 9);
        for request in requests.iter() {
            let texts: Vec<&str> = request
                .iter()
                .filter_map(|m| m.content.as_str())
                .collect();
            assert!(
                !texts.iter().any(|t| t.contains("review now")),
                "no request may contain the reminder: {texts:?}"
            );
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EngineEvent::CompletionReviewInjected { .. })),
            "no injection event for a short turn"
        );
    }

    #[tokio::test]
    async fn filtered_step_at_threshold_is_not_reviewed() {
        // A provider safety block must keep its failure path even past the
        // threshold (TS short-circuits `finishReason === 'filtered'`).
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut llm = RecordingClient::new(std::sync::Arc::clone(&recorded), 0);
        llm.filtered = true;
        let input = review_input(vec![user_message("go")], 1, None);
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = session
            .run(&llm, &BashTool::default(), &policy_auto(), &mut |event| {
                events.push(event);
            })
            .await;
        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 1);
        match progress {
            TurnProgress::Completed(outcome) => {
                assert_eq!(outcome.status, TurnEndReason::Failed);
                assert_eq!(outcome.error_code.as_deref(), Some("PROVIDER_FILTERED"));
            }
            other => panic!("expected failed outcome, got {other:?}"),
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EngineEvent::CompletionReviewInjected { .. }))
        );
    }

    #[tokio::test]
    async fn tool_context_carries_the_full_step_batch() {
        // The engine forwards the step's tool-call batch so external tools
        // can validate the round (AllDone mixed-call rejection).
        struct CaptureExecutor(std::sync::Arc<std::sync::Mutex<Vec<Vec<ToolCall>>>>);
        #[async_trait::async_trait]
        impl ToolExecutor for CaptureExecutor {
            async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult {
                self.0.lock().unwrap().push(ctx.tool_calls.clone());
                ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: "ok".to_string(),
                    is_error: false,
                    stop_turn: false,
                    updates: vec![],
                }
            }
        }

        // One step with TWO tool calls: each execution must see the batch.
        struct TwoCallsClient(std::sync::Arc<std::sync::Mutex<Vec<Vec<LlmMessage>>>>);
        #[async_trait::async_trait]
        impl LlmClient for TwoCallsClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                self.0.lock().unwrap().push(request.messages.clone());
                let mut events = Vec::new();
                let mut tool_calls = Vec::new();
                for i in 0..2 {
                    let id = format!("call_{i}");
                    // Distinct args per call so the batch-context assertion is
                    // not confounded by toolDedupe (two identical calls in
                    // one step would suppress the second).
                    let args = format!(r#"{{"probe":{i}}}"#);
                    events.push(LlmStreamEvent::ToolCall {
                        tool_call_id: id.clone(),
                        name: Some("Probe".to_string()),
                        arguments_part: Some(args.clone()),
                    });
                    tool_calls.push(crate::types::LlmToolCall {
                        id,
                        call_type: Some("function".to_string()),
                        function: crate::types::LlmToolCallFunction {
                            name: "Probe".to_string(),
                            arguments: args,
                        },
                    });
                }
                events.push(LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                });
                if tool_calls.is_empty() {
                    // never reached — two-call step always continues
                    return Ok(StreamedTurn {
                        events: vec![LlmStreamEvent::Finish {
                            finish_reason: Some("stop".to_string()),
                        }],
                        assistant: crate::llm::AssistantTurn {
                            tool_calls: vec![],
                            text: "done".to_string(),
                            thinking: String::new(),
                        },
                    });
                }
                Ok(StreamedTurn {
                    events,
                    assistant: crate::llm::AssistantTurn {
                        tool_calls,
                        text: String::new(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let llm = TwoCallsClient(std::sync::Arc::clone(&recorded));
        let tools = CaptureExecutor(std::sync::Arc::clone(&captured));
        // max_steps=1: the step with two calls runs; the turn then stops at
        // the max-steps guard (the second call never happens — the client
        // would otherwise return text for call 2 and end normally).
        let input = review_input(vec![user_message("go")], 10, Some(1));
        let mut session = TurnSession::new(input);
        let _progress = session
            .run(&llm, &tools, &policy_auto(), &mut |_| {})
            .await;
        let batches = captured.lock().unwrap();
        assert_eq!(batches.len(), 2, "both calls of the step executed");
        for batch in batches.iter() {
            assert_eq!(batch.len(), 2, "each execution sees the full batch");
            assert_eq!(batch[0].id, "call_0");
            assert_eq!(batch[1].id, "call_1");
        }
    }
}

#[cfg(test)]
mod compaction_tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, StreamedTurn};
    use crate::types::ProviderConfig;

    fn msg(role: &str, text: &str) -> LlmMessage {
        LlmMessage {
            role: role.to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    #[tokio::test]
    async fn compaction_runs_before_the_step_when_the_window_is_crossed() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<Vec<LlmMessage>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                calls.push(request.messages.clone());
                // Call 1 = the compaction round: answer with the summary.
                // Call 2 = the actual step: finish, no tool calls.
                let segment = if calls.len() == 1 {
                    vec![LlmStreamEvent::Text {
                        delta: "compacted summary".to_string(),
                    }]
                } else {
                    vec![]
                };
                let mut text = String::new();
                for event in &segment {
                    if let LlmStreamEvent::Text { delta } = event {
                        text.push_str(delta);
                    }
                }
                Ok(StreamedTurn {
                    events: segment,
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text,
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Vec<LlmMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        // History of assistant/tool exchanges that only the summary keeps:
        // ~1830 tokens against a 2000-token window (trigger 1700).
        let tool_blob = "z".repeat(300); // 75 tokens per tool result
        let mut messages = vec![msg("system", "sys"), msg("user", "u2")];
        for i in 0..20 {
            messages.push(msg("assistant", &format!("a{i}{}", "y".repeat(60))));
            messages.push(msg("tool", &tool_blob));
        }
        let input = EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(2000),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let progress = session
            .run(&llm, &__bash, &policy, &mut |event| {
                events.push(event);
            })
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)));

        // Compaction happened: summary event emitted.
        let compacted = events.iter().find_map(|event| match event {
            EngineEvent::ContextCompacted {
                summary,
                tokens_before,
                tokens_after,
                compacted_count,
                ..
            } => Some((summary.clone(), *tokens_before, *tokens_after, *compacted_count)),
            _ => None,
        });
        let (summary, tokens_before, tokens_after, compacted_count) =
            compacted.expect("context.compacted event");
        assert_eq!(summary, "compacted summary");
        assert!(tokens_before > tokens_after, "before={tokens_before} after={tokens_after}");
        assert_eq!(compacted_count, 42); // system + u2 + 20 assistant/tool pairs

        // Two LLM calls: compaction round first, then the real step.
        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let texts: Vec<&str> = requests[0]
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("Write a first-person handoff note")),
            "first request should carry the compaction instruction: {texts:?}"
        );
        // The step request uses the compacted shape: recent user message u2
        // + summary message, and the assistant/tool exchanges are gone.
        let step_texts: Vec<&str> = requests[1]
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(step_texts.iter().any(|t| t.contains("compacted to free up context")));
        assert!(
            !step_texts.iter().any(|t| t.contains(&tool_blob)),
            "tool results must be folded into the summary"
        );
    }

    #[tokio::test]
    async fn compact_retries_with_dropped_prefix_when_summary_is_empty() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<Vec<LlmMessage>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                calls.push(request.messages.clone());
                // Call 1 = compaction round returns an EMPTY summary → the
                // engine must drop the oldest message and retry.
                // Call 2 = compaction retry returns the real summary.
                // Call 3+ = the actual step.
                let (delta, is_retry) = match calls.len() {
                    1 => (String::new(), false),
                    2 => ("retried summary".to_string(), true),
                    _ => (String::new(), false),
                };
                let segment = if !delta.is_empty() || !is_retry {
                    vec![LlmStreamEvent::Text {
                        delta: delta.clone(),
                    }]
                } else {
                    vec![]
                };
                let text = delta;
                Ok(StreamedTurn {
                    events: segment,
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text,
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Vec<LlmMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        // ~1830 tokens against a 2000-token window (trigger 1700).
        let tool_blob = "z".repeat(300);
        let mut messages = vec![msg("system", "sys"), msg("user", "u2")];
        for i in 0..20 {
            messages.push(msg("assistant", &format!("a{i}{}", "y".repeat(60))));
            messages.push(msg("tool", &tool_blob));
        }
        let input = EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(2000),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let progress = session
            .run(&llm, &__bash, &policy, &mut |event| {
                events.push(event);
            })
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)));

        // Compaction succeeded with the retried summary.
        let summary = events.iter().find_map(|event| match event {
            EngineEvent::ContextCompacted { summary, .. } => Some(summary.clone()),
            _ => None,
        });
        assert_eq!(summary.as_deref(), Some("retried summary"));
        // Three LLM calls: empty compaction round, retried compaction round,
        // then the real step.
        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 3);
        // The retry dropped the oldest message: the second compaction request
        // has one fewer message than the first.
        assert!(
            requests[1].len() < requests[0].len(),
            "retry must drop the oldest message: {} -> {}",
            requests[0].len(),
            requests[1].len()
        );
    }

    #[tokio::test]
    async fn compact_fails_soft_when_every_summary_is_empty() {
        use std::sync::{Arc, Mutex};
        struct EmptySummaryClient(Arc<Mutex<usize>>);
        #[async_trait::async_trait]
        impl LlmClient for EmptySummaryClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                *calls += 1;
                // Every compaction round returns an empty summary; the final
                // call is the actual step.
                Ok(StreamedTurn {
                    events: vec![],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: String::new(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let calls = Arc::new(Mutex::new(0usize));
        let llm = EmptySummaryClient(Arc::clone(&calls));
        let tool_blob = "z".repeat(300);
        let mut messages = vec![msg("system", "sys"), msg("user", "u2")];
        for i in 0..20 {
            messages.push(msg("assistant", &format!("a{i}{}", "y".repeat(60))));
            messages.push(msg("tool", &tool_blob));
        }
        let input = EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(2000),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let progress = session
            .run(&llm, &__bash, &policy, &mut |event| {
                events.push(event);
            })
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)));
        // No compaction happened (fail soft): the turn still completes.
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, EngineEvent::ContextCompacted { .. })),
            "empty summaries must not emit context.compacted"
        );
        // COMPACTION_MAX_SHRINK_ATTEMPTS + 1 compaction rounds (0..=3 = 4),
        // then the real step.
        assert_eq!(*calls.lock().unwrap(), 5);
    }

    #[tokio::test]
    async fn no_compaction_within_the_window() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<usize>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                *self.0.lock().unwrap() += 1;
                Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "".to_string(),
                        thinking: "".to_string(),
                    },
                })
            }
        }

        let calls = Arc::new(std::sync::Mutex::new(0usize));
        let llm = RecordingClient(Arc::clone(&calls));
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "small")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(1000),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let __bash = crate::tool::BashTool::default();
        session
            .run(&llm, &__bash, &policy, &mut |event| {
                events.push(event);
            })
            .await;
        assert_eq!(*calls.lock().unwrap(), 1, "single LLM call, no compaction");
        assert!(
            !events.iter().any(|e| matches!(e, EngineEvent::ContextCompacted { .. }))
        );
    }

    #[tokio::test]
    async fn compact_closes_unresolved_tool_exchanges_in_the_summary_request() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<Vec<LlmMessage>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                calls.push(request.messages.clone());
                // Call 1 = compaction round (summary); call 2 = the step.
                let (delta, is_step) = if calls.len() == 1 {
                    ("closed-exchange summary".to_string(), false)
                } else {
                    (String::new(), true)
                };
                Ok(StreamedTurn {
                    events: if is_step {
                        vec![]
                    } else {
                        vec![LlmStreamEvent::Text {
                            delta: delta.clone(),
                        }]
                    },
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: delta,
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Vec<LlmMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        // History with an UNRESOLVED exchange: the assistant requested a tool
        // (`call_pending`) but no tool result ever arrived. Also a resolved
        // exchange so we assert only the missing one is synthesized.
        let mut messages = vec![msg("system", "sys"), msg("user", "u2")];
        messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: serde_json::Value::String("pending".to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![crate::types::LlmToolCall {
                id: "call_pending".to_string(),
                call_type: Some("function".to_string()),
                function: crate::types::LlmToolCallFunction {
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({ "command": "sleep 1" }).to_string(),
                },
            }]),
            tools: None,
            reasoning: None,
            origin: None,
        });
        messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: serde_json::Value::String("resolved".to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![crate::types::LlmToolCall {
                id: "call_resolved".to_string(),
                call_type: Some("function".to_string()),
                function: crate::types::LlmToolCallFunction {
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({ "command": "echo hi" }).to_string(),
                },
            }]),
            tools: None,
            reasoning: None,
            origin: None,
        });
        messages.push(LlmMessage {
            role: "tool".to_string(),
            content: serde_json::Value::String("hi".to_string()),
            name: None,
            tool_call_id: Some("call_resolved".to_string()),
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        });
        // Push the history over the 2000-token compaction trigger.
        for i in 0..20 {
            messages.push(msg("assistant", &format!("a{i}{}", "y".repeat(60))));
            messages.push(msg("tool", &"z".repeat(300)));
        }
        let input = EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(2000),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let progress = session
            .run(&llm, &__bash, &policy, &mut |event| {
                events.push(event);
            })
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::ContextCompacted { .. })),
            "compaction must run"
        );

        // The summary request (first LLM call) carries a synthetic tool result
        // for the unresolved exchange, with the TS-parity text.
        let requests = recorded.lock().unwrap();
        assert!(requests.len() >= 2);
        let summary_request = &requests[0];
        let synthesized = summary_request
            .iter()
            .filter(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("call_pending"))
            .collect::<Vec<_>>();
        assert_eq!(synthesized.len(), 1, "missing exchange must be synthesized");
        assert_eq!(
            synthesized[0].content.as_str(),
            Some(TOOL_INTERRUPTED_TEXT),
            "synthetic result must carry the TS-parity interrupted text"
        );
        // The resolved exchange is untouched (no extra synthetic entry).
        let resolved_count = summary_request
            .iter()
            .filter(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("call_resolved"))
            .count();
        assert_eq!(resolved_count, 1);
    }

    #[tokio::test]
    async fn compact_treats_truncated_summary_as_empty_and_retries() {
        use std::sync::{Arc, Mutex};
        struct TruncatedThenOkClient(Arc<Mutex<usize>>);
        #[async_trait::async_trait]
        impl LlmClient for TruncatedThenOkClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                *calls += 1;
                // Call 1 = compaction round returns a TRUNCATED summary
                // (finish_reason length) — must be treated as empty and
                // retried with a smaller prefix.
                // Call 2 = compaction retry returns the real summary.
                // Call 3+ = the actual step.
                if *calls == 1 {
                    return Ok(StreamedTurn {
                        events: vec![
                            LlmStreamEvent::Text {
                                delta: "truncated-".to_string(),
                            },
                            LlmStreamEvent::Finish {
                                finish_reason: Some("length".to_string()),
                            },
                        ],
                        assistant: crate::llm::AssistantTurn {
                            tool_calls: vec![],
                            text: "truncated-".to_string(),
                            thinking: String::new(),
                        },
                    });
                }
                let delta = if *calls == 2 {
                    "full summary after truncation".to_string()
                } else {
                    String::new()
                };
                Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: delta,
                        thinking: String::new(),
                    },
                })
            }
        }

        let calls = Arc::new(Mutex::new(0usize));
        let llm = TruncatedThenOkClient(Arc::clone(&calls));
        let tool_blob = "z".repeat(300);
        let mut messages = vec![msg("system", "sys"), msg("user", "u2")];
        for i in 0..20 {
            messages.push(msg("assistant", &format!("a{i}{}", "y".repeat(60))));
            messages.push(msg("tool", &tool_blob));
        }
        let input = EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(2000),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let progress = session
            .run(&llm, &__bash, &policy, &mut |event| {
                events.push(event);
            })
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)));
        // The truncated summary was discarded; the retried summary landed.
        let summary = events.iter().find_map(|event| match event {
            EngineEvent::ContextCompacted { summary, .. } => Some(summary.clone()),
            _ => None,
        });
        assert_eq!(summary.as_deref(), Some("full summary after truncation"));
        // Three LLM calls: truncated compaction round, retried compaction
        // round, then the real step.
        assert_eq!(*calls.lock().unwrap(), 3);
    }

    /// End-to-end regression for the DeepSeek 400 "insufficient tool messages
    /// following tool_calls message": a foreign user message (async
    /// notification) interleaved between an assistant `tool_calls` and its
    /// tool result must be reordered after the result in the request the
    /// engine actually sends.
    #[tokio::test]
    async fn step_request_reorders_tool_result_past_foreign_message() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<Vec<LlmMessage>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                self.0.lock().unwrap().push(request.messages.clone());
                Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: String::new(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Vec<LlmMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        // History shaped exactly like the failing wire replay: assistant made
        // a call, an async notification landed before the result arrived.
        let mut messages = vec![msg("user", "u1")];
        messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: serde_json::Value::String(String::new()),
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![crate::types::LlmToolCall {
                id: "call_interleaved".to_string(),
                call_type: Some("function".to_string()),
                function: crate::types::LlmToolCallFunction {
                    name: "Bash".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tools: None,
            reasoning: None,
            origin: None,
        });
        messages.push(msg("user", "<notification task completed>"));
        messages.push(LlmMessage {
            role: "tool".to_string(),
            content: serde_json::Value::String("result".to_string()),
            name: None,
            tool_call_id: Some("call_interleaved".to_string()),
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        });
        messages.push(msg("user", "continue"));
        let input = EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let __bash = crate::tool::BashTool::default();
        let _ = session
            .run(&llm, &__bash, &policy, &mut |event| {
                events.push(event);
            })
            .await;

        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 1, "single step request expected");
        let sent = &requests[0];
        let roles: Vec<&str> = sent.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            vec!["user", "assistant", "tool", "user", "user"],
            "tool result must sit directly after its assistant, before the notification: {roles:?}"
        );
        assert_eq!(sent[2].tool_call_id.as_deref(), Some("call_interleaved"));
        assert_eq!(sent[2].content.as_str(), Some("result"));
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, ScriptedLlmClient, StreamedTurn};
    use crate::types::ProviderConfig;

    fn msg(role: &str, text: &str) -> LlmMessage {
        LlmMessage {
            role: role.to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    fn input(messages: Vec<LlmMessage>) -> EngineTurnInput {
        EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://example.test/v1".to_string(),
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(3),
            cwd: Some(std::env::temp_dir().to_string_lossy().to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        }
    }

    mod close_unresolved_tool_exchanges_tests {
        use super::super::close_unresolved_tool_exchanges;
        use crate::types::{LlmMessage, LlmToolCall, LlmToolCallFunction};

        fn tool(id: &str, text: &str) -> LlmMessage {
            LlmMessage {
                role: "tool".to_string(),
                content: serde_json::Value::String(text.to_string()),
                name: None,
                tool_call_id: Some(id.to_string()),
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            }
        }

        fn assistant_with_calls(ids: &[&str]) -> LlmMessage {
            LlmMessage {
                role: "assistant".to_string(),
                content: serde_json::Value::String(String::new()),
                name: None,
                tool_call_id: None,
                tool_calls: Some(
                    ids.iter()
                        .map(|id| LlmToolCall {
                            id: id.to_string(),
                            call_type: Some("function".to_string()),
                            function: LlmToolCallFunction {
                                name: "Bash".to_string(),
                                arguments: "{}".to_string(),
                            },
                        })
                        .collect(),
                ),
                tools: None,
                reasoning: None,
                origin: None,
            }
        }

        fn user(text: &str) -> LlmMessage {
            LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(text.to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            }
        }

        fn roles(messages: &[LlmMessage]) -> Vec<String> {
            messages.iter().map(|m| m.role.clone()).collect()
        }

        fn tool_ids(messages: &[LlmMessage]) -> Vec<Option<&str>> {
            messages
                .iter()
                .filter(|m| m.role == "tool")
                .map(|m| m.tool_call_id.as_deref())
                .collect()
        }

        /// Regression for the 400 "insufficient tool messages following
        /// tool_calls message": an async notification (user) landed between an
        /// assistant `tool_calls` and its tool result. The result must be
        /// reordered back right after the assistant.
        #[test]
        fn reorders_tool_result_after_foreign_message() {
            let mut messages = vec![
                user("u1"),
                assistant_with_calls(&["call_1"]),
                user("<notification task completed>"),
                tool("call_1", "ok"),
            ];
            close_unresolved_tool_exchanges(&mut messages);
            assert_eq!(
                roles(&messages),
                vec!["user", "assistant", "tool", "user"],
                "tool result must be reordered before the foreign user message"
            );
            assert_eq!(tool_ids(&messages), vec![Some("call_1")]);
            assert_eq!(messages[3].role, "user");
            assert_eq!(messages[2].role, "tool");
        }

        /// Parallel calls interrupted by one notification: all results are
        /// reordered back after the assistant, preserving call order.
        #[test]
        fn reorders_parallel_results_after_foreign_message() {
            let mut messages = vec![
                user("u1"),
                assistant_with_calls(&["call_1", "call_2"]),
                user("<notification>"),
                tool("call_1", "r1"),
                tool("call_2", "r2"),
            ];
            close_unresolved_tool_exchanges(&mut messages);
            assert_eq!(
                roles(&messages),
                vec!["user", "assistant", "tool", "tool", "user"]
            );
            assert_eq!(tool_ids(&messages), vec![Some("call_1"), Some("call_2")]);
        }

        /// A result that never arrived keeps its synthesized interrupted
        /// message right after the assistant.
        #[test]
        fn synthesizes_missing_result() {
            let mut messages = vec![user("u1"), assistant_with_calls(&["call_1"])];
            close_unresolved_tool_exchanges(&mut messages);
            assert_eq!(roles(&messages), vec!["user", "assistant", "tool"]);
            assert_eq!(tool_ids(&messages), vec![Some("call_1")]);
            let content = messages[2].content.as_str().unwrap();
            assert!(
                content.contains("not available"),
                "unresolved result must carry the interrupted text, got: {content}"
            );
        }

        /// Mixed: one resolved result, one missing, with a foreign message.
        #[test]
        fn mixed_resolved_and_missing() {
            let mut messages = vec![
                user("u1"),
                assistant_with_calls(&["call_1", "call_2"]),
                user("<notification>"),
                tool("call_2", "r2"),
            ];
            close_unresolved_tool_exchanges(&mut messages);
            assert_eq!(
                roles(&messages),
                vec!["user", "assistant", "tool", "tool", "user"]
            );
            // Slots follow the assistant's declared call order: call_1's
            // missing result keeps the synthesized interrupted placeholder,
            // call_2's real result lands right after it.
            assert_eq!(tool_ids(&messages), vec![Some("call_1"), Some("call_2")]);
            let interrupted = messages[2].content.as_str().unwrap();
            assert!(interrupted.contains("not available"));
            assert_eq!(messages[3].content.as_str().unwrap(), "r2");
        }

        /// A tool message with no preceding assistant call is dropped (TS
        /// `orphan_tool_result_dropped` parity) instead of being sent as an
        /// invalid standalone tool message.
        #[test]
        fn drops_orphan_tool_result() {
            let mut messages = vec![
                tool("call_orphan", "stray"),
                user("u1"),
                assistant_with_calls(&["call_2"]),
                tool("call_2", "r2"),
            ];
            close_unresolved_tool_exchanges(&mut messages);
            assert_eq!(roles(&messages), vec!["user", "assistant", "tool"]);
            assert_eq!(tool_ids(&messages), vec![Some("call_2")]);
        }

        /// Already-correct adjacency is untouched.
        #[test]
        fn leaves_correct_adjacency_untouched() {
            let mut messages = vec![
                user("u1"),
                assistant_with_calls(&["call_1"]),
                tool("call_1", "r1"),
                user("u2"),
            ];
            close_unresolved_tool_exchanges(&mut messages);
            assert_eq!(roles(&messages), vec!["user", "assistant", "tool", "user"]);
            assert_eq!(tool_ids(&messages), vec![Some("call_1")]);
        }
    }



    #[tokio::test]
    async fn updated_tools_are_advertised_in_subsequent_requests() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<Option<Vec<serde_json::Value>>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                self.0.lock().unwrap().push(request.tools.clone());
                Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: String::new(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Option<Vec<serde_json::Value>>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "hi")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        // A tool registered after construction (external tool bridge path):
        // the next request must advertise it.
        session.update_tools(vec![crate::types::EngineTool {
            name: "Lookup".to_string(),
            description: "Look things up".to_string(),
            args_schema: serde_json::json!({ "type": "object", "properties": {} }),
        }]);
        let __bash = crate::tool::BashTool::default();
        session
            .run(&llm, &__bash, &policy, &mut |_| {})
            .await;
        let requests = recorded.lock().unwrap();
        let tools = requests[0].as_ref().expect("tools present");
        assert!(
            tools.iter().any(|t| t["name"] == "Lookup"),
            "request tools must include the registered def: {tools:?}"
        );
    }

    #[tokio::test]
    async fn active_tools_whitelist_reaches_request_tools_verbatim() {
        // The bridge pre-filters `input.tools` by `EngineTurnInput.active_tools`
        // (see `engine_tools` in dimi-bridge); the engine must advertise
        // exactly that list to the LLM — a whitelist-filtered tool (Bash here)
        // must never reappear in `request.tools`.
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<Option<Vec<serde_json::Value>>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                self.0.lock().unwrap().push(request.tools.clone());
                Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: String::new(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Option<Vec<serde_json::Value>>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "hi")],
            // The bridge's filtered def list (Bash excluded by
            // `active_tools`); the engine passes it to the request untouched.
            tools: vec![crate::types::EngineTool {
                name: "Read".to_string(),
                description: "Read files".to_string(),
                args_schema: serde_json::json!({ "type": "object", "properties": {} }),
            }],
            active_tools: Some(vec!["Read".to_string()]),
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let __bash = crate::tool::BashTool::default();
        session
            .run(&llm, &__bash, &policy, &mut |_| {})
            .await;
        let requests = recorded.lock().unwrap();
        let tools = requests[0].as_ref().expect("tools present");
        assert_eq!(
            tools.len(),
            1,
            "only the whitelisted def is advertised: {tools:?}"
        );
        assert_eq!(tools[0]["name"], "Read");
    }



    #[tokio::test]
    async fn context_overflow_compacts_and_retries_the_step() {
        use std::sync::{Arc, Mutex};
        struct OverflowThenOkClient(Arc<Mutex<usize>>);
        #[async_trait::async_trait]
        impl LlmClient for OverflowThenOkClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                *calls += 1;
                if *calls == 1 {
                    return Err(crate::llm::LlmError {
                        message: "HTTP 413: request too large".to_string(),
                        code: Some("CONTEXT_OVERFLOW".to_string()),
                        ..Default::default()
                    });
                }
                Ok(StreamedTurn {
                    events: vec![
                        LlmStreamEvent::Text {
                            delta: "recovered".to_string(),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("stop".to_string()),
                        },
                    ],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "recovered".to_string(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let calls = Arc::new(Mutex::new(0usize));
        let llm = OverflowThenOkClient(Arc::clone(&calls));
        // History below the 85% trigger (1700/2000 tokens) so the overflow
        // recovery — not the loop-top estimate check — drives compaction.
        let blob = "z".repeat(60);
        let mut messages = vec![msg("user", "u2")];
        for i in 0..5 {
            messages.push(msg("assistant", &format!("a{i}{}", "y".repeat(20))));
            messages.push(msg("tool", &blob));
        }
        let input = EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(5),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(2000),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = session
            .run(&llm, &crate::tool::BashTool::default(), &policy, &mut |event| {
                events.push(event);
            })
            .await;
        match progress {
            TurnProgress::Completed(outcome) => {
                assert_eq!(outcome.status, TurnEndReason::Completed);
                assert_eq!(outcome.error, None);
            }
            TurnProgress::NeedsApproval(_) => panic!("no approval in auto mode"),
        }
        // Three LLM calls: the overflow, the compaction summary round, and
        // the retried step.
        assert_eq!(*calls.lock().unwrap(), 3);
        // The compaction round ran (context.compacted emitted).
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::ContextCompacted { .. })),
            "overflow must trigger compaction: {events:?}"
        );
        // The retried step produced the recovered text.
        assert!(
            events.iter().any(|e| matches!(
                e,
                EngineEvent::AssistantDelta { delta, .. } if delta == "recovered"
            )),
            "retried step must stream: {events:?}"
        );
    }

    #[tokio::test]
    async fn retryable_provider_error_retries_the_step_then_completes() {
        use std::sync::{Arc, Mutex};
        struct RetryThenOkClient(Arc<Mutex<usize>>);
        #[async_trait::async_trait]
        impl LlmClient for RetryThenOkClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                *calls += 1;
                if *calls == 1 {
                    return Err(crate::llm::LlmError {
                        message: "connection error".to_string(),
                        code: Some("CONNECTION_ERROR".to_string()),
                        retryable: true,
                        retry_after_ms: Some(1),
                    });
                }
                Ok(StreamedTurn {
                    events: vec![
                        LlmStreamEvent::Text {
                            delta: "recovered".to_string(),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("stop".to_string()),
                        },
                    ],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "recovered".to_string(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let calls = Arc::new(Mutex::new(0usize));
        let llm = RetryThenOkClient(Arc::clone(&calls));
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "hi")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(3),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: Some(3),
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = session
            .run(&llm, &crate::tool::BashTool::default(), &policy, &mut |event| {
                events.push(event);
            })
            .await;
        match progress {
            TurnProgress::Completed(outcome) => {
                assert_eq!(outcome.status, TurnEndReason::Completed);
                assert_eq!(outcome.error, None);
            }
            TurnProgress::NeedsApproval(_) => panic!("no approval in auto mode"),
        }
        // First call failed retryably, second call succeeded.
        assert_eq!(*calls.lock().unwrap(), 2);
        // A step-retrying event announced the retry with the attempt counters.
        let retrying = events
            .iter()
            .find_map(|e| match e {
                EngineEvent::TurnStepRetrying {
                    failed_attempt,
                    next_attempt,
                    max_attempts,
                    error_name,
                    ..
                } => Some((*failed_attempt, *next_attempt, *max_attempts, error_name.clone())),
                _ => None,
            })
            .expect("must emit TurnStepRetrying");
        assert_eq!(retrying.0, 1);
        assert_eq!(retrying.1, 2);
        assert_eq!(retrying.2, 3);
        assert_eq!(retrying.3.as_deref(), Some("CONNECTION_ERROR"));
        // The retried step streamed its text.
        assert!(
            events.iter().any(|e| matches!(
                e,
                EngineEvent::AssistantDelta { delta, .. } if delta == "recovered"
            )),
            "retried step must stream: {events:?}"
        );
    }

    #[tokio::test]
    async fn truncated_finish_marks_the_turn_truncated() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<usize>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                *self.0.lock().unwrap() += 1;
                Ok(StreamedTurn {
                    events: vec![LlmStreamEvent::Finish {
                        finish_reason: Some("length".to_string()),
                    }],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: String::new(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let calls = Arc::new(Mutex::new(0usize));
        let llm = RecordingClient(Arc::clone(&calls));
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "hi")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(1),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = session
            .run(&llm, &crate::tool::BashTool::default(), &policy, &mut |event| {
                events.push(event);
            })
            .await;
        match progress {
            TurnProgress::Completed(outcome) => {
                assert_eq!(outcome.status, TurnEndReason::Completed);
                assert_eq!(outcome.truncated, Some(true));
            }
            TurnProgress::NeedsApproval(_) => panic!("no approval in auto mode"),
        }
        // The step reports the provider finish reason (TS normalize parity:
        // length stays "length").
        let step_completed = events.iter().find_map(|event| match event {
            EngineEvent::TurnStepCompleted { finish_reason, .. } => finish_reason.clone(),
            _ => None,
        });
        assert_eq!(step_completed.as_deref(), Some("length"));
    }

    #[tokio::test]
    async fn tool_calls_precede_tool_results_in_the_next_request() {
        use std::sync::{Arc, Mutex};
        struct RecordingClient(Arc<Mutex<Vec<Vec<LlmMessage>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                let mut calls = self.0.lock().unwrap();
                calls.push(request.messages.clone());
                // Call 1: ask for a Bash run. Call 2: finish.
                let segment = if calls.len() == 1 {
                    vec![
                        LlmStreamEvent::ToolCall {
                            tool_call_id: "call_bash".to_string(),
                            name: Some("Bash".to_string()),
                            arguments_part: Some("{\"command\":\"echo ok\"}".to_string()),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("tool_calls".to_string()),
                        },
                    ]
                } else {
                    vec![LlmStreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                    }]
                };
                let mut tool_calls = Vec::new();
                for event in &segment {
                    if let LlmStreamEvent::ToolCall {
                        tool_call_id,
                        name,
                        arguments_part,
                        ..
                    } = event
                    {
                        tool_calls.push(crate::types::LlmToolCall {
                            id: tool_call_id.clone(),
                            call_type: Some("function".to_string()),
                            function: crate::types::LlmToolCallFunction {
                                name: name.clone().unwrap_or_default(),
                                arguments: arguments_part.clone().unwrap_or_default(),
                            },
                        });
                    }
                }
                Ok(StreamedTurn {
                    events: segment,
                    assistant: crate::llm::AssistantTurn {
                        tool_calls,
                        text: String::new(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let recorded: Arc<Mutex<Vec<Vec<LlmMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let llm = RecordingClient(Arc::clone(&recorded));
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "run it")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(5),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::new(input);
        let __bash = crate::tool::BashTool::default();
        let progress = session
            .run(&llm, &__bash, &policy, &mut |_| {})
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)));

        let requests = recorded.lock().unwrap();
        assert!(requests.len() >= 2, "multi-step turn: {requests:?}");
        // Second request: the tool message must be preceded by the assistant
        // message carrying the tool_calls (provider contract).
        let second = &requests[1];
        let tool_index = second
            .iter()
            .position(|m| m.role == "tool")
            .expect("tool result present");
        let assistant_before = &second[tool_index - 1];
        assert_eq!(assistant_before.role, "assistant");
        let calls = assistant_before
            .tool_calls
            .as_ref()
            .expect("assistant carries tool_calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_bash");
        assert_eq!(calls[0].function.name, "Bash");
        // The tool result matches the call id.
        assert_eq!(second[tool_index].tool_call_id.as_deref(), Some("call_bash"));
    }

    #[tokio::test]
    async fn cancel_stops_the_turn_with_a_cancelled_outcome() {
        use std::sync::Arc;
        // The client blocks on the first request until told to answer;
        // the test cancels while it is in flight.
        #[async_trait::async_trait]
        impl LlmClient for BlockingClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                if let Some(tx) = self.started.lock().await.take() {
                    let _ = tx.send(());
                }
                std::future::pending::<()>().await;
                unreachable!("blocking client never returns");
            }
        }
        struct BlockingClient {
            started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        }

        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let client = BlockingClient {
            started: tokio::sync::Mutex::new(Some(started_tx)),
        };
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "hello")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(10),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let cancel = Arc::new(CancelSignal::new());
        let mut session = TurnSession::with_steer_and_cancel(
            input,
            None,
            Arc::clone(&cancel),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let handle = tokio::spawn(async move {
            let __bash = crate::tool::BashTool::default();
            session
                .run(&client, &__bash, &policy, &mut |_| {})
                .await
        });
        // Wait until the LLM request is in flight, then cancel.
        started_rx.await.expect("request started");
        cancel.cancel();
        let progress = handle.await.expect("run completes");
        match progress {
            TurnProgress::Completed(outcome) => {
                assert_eq!(outcome.status, TurnEndReason::Cancelled);
            }
            TurnProgress::NeedsApproval(_) => panic!("no approval in auto mode"),
        }
    }

    #[tokio::test]
    async fn step_cancel_resets_retry_budget_before_the_next_step() {
        struct RetryThenCancelThenOk {
            calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl LlmClient for RetryThenCancelThenOk {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                match self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) {
                    0 | 1 => Err(crate::llm::LlmError {
                        message: "transient".to_string(),
                        code: Some("CONNECTION_ERROR".to_string()),
                        retry_after_ms: Some(if self.calls.load(std::sync::atomic::Ordering::Relaxed) == 1 {
                            60_000
                        } else {
                            1
                        }),
                        retryable: true,
                    }),
                    _ => Ok(StreamedTurn {
                        events: vec![
                            LlmStreamEvent::Text {
                                delta: "recovered after reset".to_string(),
                            },
                            LlmStreamEvent::Finish {
                                finish_reason: Some("stop".to_string()),
                            },
                        ],
                        assistant: crate::llm::AssistantTurn {
                            tool_calls: vec![],
                            text: "recovered after reset".to_string(),
                            thinking: String::new(),
                        },
                    }),
                }
            }
        }

        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "reset retry budget")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(4),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: Some(1),
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let active_step = std::sync::Arc::new(std::sync::Mutex::new(None));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let llm = RetryThenCancelThenOk {
            calls: std::sync::Arc::clone(&calls),
        };
        let mut session = TurnSession::with_steer_and_cancel_and_step_cancel(
            input,
            None,
            std::sync::Arc::new(CancelSignal::new()),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::sync::Arc::clone(&active_step),
        );
        let mut cancelled = false;
        let mut retry_steps = Vec::new();
        let progress = session
            .run(&llm, &crate::tool::BashTool::default(), &crate::permission::PolicyConfig {
                mode: crate::permission::PermissionMode::Auto,
                rules: vec![],
                session_approved_patterns: vec![],
            }, &mut |event| {
                if let EngineEvent::TurnStepRetrying { step, .. } = event {
                    retry_steps.push(step);
                    if !cancelled {
                        cancelled = true;
                        active_step
                            .lock()
                            .unwrap()
                            .as_ref()
                            .expect("active step signal")
                            .cancel();
                    }
                }
            })
            .await;

        assert!(matches!(progress, TurnProgress::Completed(ref outcome) if outcome.status == TurnEndReason::Completed));
        assert_eq!(retry_steps, vec![1, 2]);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn turn_cancel_interrupts_inflight_compaction() {
        struct BlockingCompaction {
            started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        }

        #[async_trait::async_trait]
        impl LlmClient for BlockingCompaction {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                if let Some(sender) = self.started.lock().await.take() {
                    let _ = sender.send(());
                }
                std::future::pending::<()>().await;
                unreachable!("compaction request is cancelled");
            }
        }

        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let cancel = std::sync::Arc::new(CancelSignal::new());
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", &"large history ".repeat(500))],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(2),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(200),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let llm = BlockingCompaction {
            started: tokio::sync::Mutex::new(Some(started_sender)),
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let mut session = TurnSession::with_steer_and_cancel(
            input,
            None,
            std::sync::Arc::clone(&cancel),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_for_run = std::sync::Arc::clone(&events);
        let mut handle = tokio::spawn(async move {
            session
                .run(
                    &llm,
                    &crate::tool::BashTool::default(),
                    &policy,
                    &mut |event| events_for_run.lock().unwrap().push(event),
                )
                .await
        });
        started_receiver.await.expect("compaction request started");
        cancel.cancel();
        let progress = tokio::time::timeout(std::time::Duration::from_secs(1), &mut handle)
            .await
            .expect("turn cancellation must interrupt compaction")
            .expect("turn task must not panic");
        assert!(matches!(progress, TurnProgress::Completed(outcome) if outcome.status == TurnEndReason::Cancelled));
        let events = events.lock().unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::TurnStepInterrupted { step: 1, reason, .. } if reason == "aborted"
        )));
    }

    #[tokio::test]
    async fn turn_cancel_emits_a_result_for_an_inflight_tool() {
        struct BlockingTool {
            started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        }

        #[async_trait::async_trait]
        impl ToolExecutor for BlockingTool {
            async fn execute(&self, _call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
                if let Some(sender) = self.started.lock().await.take() {
                    let _ = sender.send(());
                }
                std::future::pending::<ToolResult>().await
            }
        }

        let llm = ScriptedLlmClient::new(vec![vec![
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_cancelled_tool".to_string(),
                name: Some("Bash".to_string()),
                arguments_part: Some(r#"{"command":"sleep 5"}"#.to_string()),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("tool_calls".to_string()),
            },
        ]]);
        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let cancel = std::sync::Arc::new(CancelSignal::new());
        let mut session = TurnSession::with_steer_and_cancel(
            input(vec![msg("user", "cancel the tool")]),
            None,
            std::sync::Arc::clone(&cancel),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let executor = BlockingTool {
            started: tokio::sync::Mutex::new(Some(started_sender)),
        };
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_for_run = std::sync::Arc::clone(&events);
        let handle = tokio::spawn(async move {
            session
                .run(
                    &llm,
                    &executor,
                    &crate::permission::PolicyConfig {
                        mode: crate::permission::PermissionMode::Auto,
                        rules: vec![],
                        session_approved_patterns: vec![],
                    },
                    &mut |event| events_for_run.lock().unwrap().push(event),
                )
                .await
        });
        started_receiver.await.expect("tool started");
        cancel.cancel();
        let progress = handle
            .await
            .expect("turn task must not panic");
        assert!(matches!(progress, TurnProgress::Completed(outcome) if outcome.status == TurnEndReason::Cancelled));
        let events = events.lock().unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::ToolResult { tool_call_id, is_error: Some(true), .. }
                if tool_call_id == "call_cancelled_tool"
        )));
    }

    #[tokio::test]
    async fn step_cancel_during_compaction_emits_interrupted_before_continuing() {
        struct BlockingThenSummary {
            calls: std::sync::atomic::AtomicUsize,
            started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        }

        #[async_trait::async_trait]
        impl LlmClient for BlockingThenSummary {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                if self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    if let Some(sender) = self.started.lock().await.take() {
                        let _ = sender.send(());
                    }
                    std::future::pending::<()>().await;
                    unreachable!("the first compaction request is cancelled");
                }
                let is_compaction = request
                    .messages
                    .iter()
                    .any(|message| message.role == "user" && message.content.to_string().contains("summarize"));
                if is_compaction {
                    Ok(StreamedTurn {
                        events: vec![
                            LlmStreamEvent::Text {
                                delta: "summary after cancelled compaction".to_string(),
                            },
                            LlmStreamEvent::Finish {
                                finish_reason: Some("stop".to_string()),
                            },
                        ],
                        assistant: crate::llm::AssistantTurn {
                            tool_calls: vec![],
                            text: "summary after cancelled compaction".to_string(),
                            thinking: String::new(),
                        },
                    })
                } else {
                    Ok(StreamedTurn {
                        events: vec![
                            LlmStreamEvent::Text {
                                delta: "completed after compaction cancellation".to_string(),
                            },
                            LlmStreamEvent::Finish {
                                finish_reason: Some("stop".to_string()),
                            },
                        ],
                        assistant: crate::llm::AssistantTurn {
                            tool_calls: vec![],
                            text: "completed after compaction cancellation".to_string(),
                            thinking: String::new(),
                        },
                    })
                }
            }
        }

        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let cancel = std::sync::Arc::new(CancelSignal::new());
        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", &"large history ".repeat(500))],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(3),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: Some(200),
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let mut session = TurnSession::with_steer_and_cancel(
            input,
            None,
            std::sync::Arc::clone(&cancel),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let active_step = session.active_step_cancel_handle();
        let llm = BlockingThenSummary {
            calls: std::sync::atomic::AtomicUsize::new(0),
            started: tokio::sync::Mutex::new(Some(started_sender)),
        };
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_for_run = std::sync::Arc::clone(&events);
        let handle = tokio::spawn(async move {
            session
                .run(
                    &llm,
                    &crate::tool::BashTool::default(),
                    &policy,
                    &mut |event| events_for_run.lock().unwrap().push(event),
                )
                .await
        });
        started_receiver.await.expect("compaction request started");
        active_step
            .lock()
            .unwrap()
            .as_ref()
            .expect("active step signal")
            .cancel();
        let progress = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("step cancellation must interrupt compaction")
            .expect("turn task must not panic");
        assert!(matches!(progress, TurnProgress::Completed(outcome) if outcome.status == TurnEndReason::Completed));
        let events = events.lock().unwrap();
        let interrupted = events
            .iter()
            .position(|event| matches!(event, EngineEvent::TurnStepInterrupted { step: 1, .. }))
            .expect("cancelled compaction step must be interrupted");
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, EngineEvent::TurnStepStarted { step: 1, .. })),
            "compaction happens before step.started"
        );
        assert!(
            events
                .iter()
                .position(|event| matches!(event, EngineEvent::TurnStepStarted { step: 2, .. }))
                .is_some_and(|started| interrupted < started),
            "step 1 interruption must precede the next step: {events:?}"
        );
    }

    #[tokio::test]
    async fn step_cancel_wins_when_a_tool_completes_at_the_boundary() {
        // Reproduce a cancellation racing the last tool result: the cancel
        // arrives before the batch returns, so the step must be interrupted
        // and the same turn must continue with the next scripted request.
        struct CancelDuringTool {
            active_step: std::sync::Arc<
                std::sync::Mutex<Option<std::sync::Arc<CancelSignal>>>,
            >,
        }

        #[async_trait::async_trait]
        impl ToolExecutor for CancelDuringTool {
            async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
                let signal = self
                    .active_step
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("step signal must be installed before tool execution");
                signal.cancel();
                ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: "cancelled at the boundary".to_string(),
                    is_error: false,
                    stop_turn: false,
                    updates: vec![],
                }
            }
        }

        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "cancel this step")],
            tools: vec![crate::types::EngineTool {
                name: "Probe".to_string(),
                description: "probe".to_string(),
                args_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                }),
            }],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(3),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let llm = ScriptedLlmClient::new(vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_boundary".to_string(),
                    name: Some("Probe".to_string()),
                    arguments_part: Some("{}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![
                LlmStreamEvent::Text {
                    delta: "continued".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let mut session = TurnSession::new(input);
        let active_step = session.active_step_cancel_handle();
        let executor = CancelDuringTool { active_step };
        let mut events = Vec::new();
        let progress = session
            .run(&llm, &executor, &crate::permission::PolicyConfig {
                mode: crate::permission::PermissionMode::Auto,
                rules: vec![],
                session_approved_patterns: vec![],
            }, &mut |event| events.push(event))
            .await;

        assert!(matches!(progress, TurnProgress::Completed(ref outcome) if outcome.status == TurnEndReason::Completed));
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::TurnStepInterrupted { step: 1, reason, .. } if reason == "aborted"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::AssistantDelta { delta, .. } if delta == "continued"
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            EngineEvent::TurnStepCompleted { step: 1, .. }
        )));
    }

    #[tokio::test]
    async fn step_cancel_interrupts_inflight_llm_and_continues() {
        struct CancelableClient {
            calls: std::sync::atomic::AtomicUsize,
            started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        }

        #[async_trait::async_trait]
        impl LlmClient for CancelableClient {
            async fn stream_chat(
                &self,
                _request: &ChatRequest,
            ) -> Result<StreamedTurn, crate::llm::LlmError> {
                if self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    if let Some(sender) = self.started.lock().await.take() {
                        let _ = sender.send(());
                    }
                    std::future::pending::<()>().await;
                    unreachable!("the first request is cancelled");
                }
                Ok(StreamedTurn {
                    events: vec![
                        LlmStreamEvent::Text {
                            delta: "continued after llm cancellation".to_string(),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("stop".to_string()),
                        },
                    ],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "continued after llm cancellation".to_string(),
                        thinking: String::new(),
                    },
                })
            }
        }

        let input = EngineTurnInput {
            turn_id: 1,
            messages: vec![msg("user", "cancel the request")],
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://x".to_string(),
                api_key: "k".to_string(),
                model: "m".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(3),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };
        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let client = CancelableClient {
            calls: std::sync::atomic::AtomicUsize::new(0),
            started: tokio::sync::Mutex::new(Some(started_sender)),
        };
        let mut session = TurnSession::new(input);
        let active_step = session.active_step_cancel_handle();
        let policy = crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        };
        let handle = tokio::spawn(async move {
            session
                .run(&client, &crate::tool::BashTool::default(), &policy, &mut |_| {})
                .await
        });
        started_receiver.await.expect("first request started");
        active_step
            .lock()
            .unwrap()
            .as_ref()
            .expect("active step signal")
            .cancel();
        let progress = handle.await.expect("run completes");
        let TurnProgress::Completed(outcome) = progress else {
            panic!("step cancellation must not pause for approval: {progress:?}");
        };
        assert_eq!(outcome.status, TurnEndReason::Completed);
    }

    #[test]
    fn cancel_signal_records_the_stop_reason() {
        // F3.5: the per-task cancel signal carries the TaskStop reason so the
        // killed settle can report it instead of the hardcoded default.
        let signal = CancelSignal::new();
        assert!(!signal.is_cancelled());
        assert_eq!(signal.reason(), None);

        signal.cancel_with_reason(Some("user abort".to_string()));
        assert!(signal.is_cancelled());
        assert_eq!(signal.reason().as_deref(), Some("user abort"));

        // The first reason wins; a later cancel cannot overwrite it.
        signal.cancel_with_reason(Some("later reason".to_string()));
        assert_eq!(signal.reason().as_deref(), Some("user abort"));

        // A plain cancel carries no reason (the settle falls back to the
        // TS default string).
        let plain = CancelSignal::new();
        plain.cancel();
        assert!(plain.is_cancelled());
        assert_eq!(plain.reason(), None);

        // An EMPTY reason is treated as no reason: the settle falls back to
        // "Stopped by TaskStop" instead of settling with `error: ""`, and an
        // empty reason cannot block a later real one.
        let empty = CancelSignal::new();
        empty.cancel_with_reason(Some(String::new()));
        assert!(empty.is_cancelled());
        assert_eq!(empty.reason(), None);
        empty.cancel_with_reason(Some("user abort".to_string()));
        assert_eq!(empty.reason().as_deref(), Some("user abort"));
    }
}

#[cfg(test)]
mod approval_batch_tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, ScriptedLlmClient};
    use crate::permission::PermissionMode;
    use crate::types::ProviderConfig;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn msg(role: &str, text: &str) -> LlmMessage {
        LlmMessage {
            role: role.to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    fn result(call_id: &str, name: &str, output: &str, stop_turn: bool) -> ToolResult {
        ToolResult {
            tool_call_id: call_id.to_string(),
            tool_name: name.to_string(),
            output: output.to_string(),
            is_error: false,
            stop_turn,
            updates: vec![],
        }
    }

    /// Records which tool calls were executed and returns scripted results
    /// (calls without a scripted result get a plain success result).
    struct RecordingToolExecutor {
        executed: Arc<Mutex<Vec<String>>>,
        results: HashMap<String, ToolResult>,
    }

    impl RecordingToolExecutor {
        fn new(results: Vec<ToolResult>) -> Self {
            Self {
                executed: Arc::new(Mutex::new(Vec::new())),
                results: results
                    .into_iter()
                    .map(|r| (r.tool_call_id.clone(), r))
                    .collect(),
            }
        }
    }

    #[async_trait::async_trait]
    impl ToolExecutor for RecordingToolExecutor {
        async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
            self.executed.lock().unwrap().push(call.id.clone());
            self.results.get(&call.id).cloned().unwrap_or_else(|| {
                result(&call.id, &call.name, &format!("ran {}", call.name), false)
            })
        }
    }

    /// Every assistant `tool_calls` entry must have a matching `tool` result
    /// (providers reject a dangling tool_call on the next request).
    fn assert_no_dangling_tool_calls(messages: &[LlmMessage]) {
        for message in messages {
            let Some(calls) = &message.tool_calls else { continue };
            for call in calls {
                let has_result = messages.iter().any(|m| {
                    m.role == "tool" && m.tool_call_id.as_deref() == Some(call.id.as_str())
                });
                assert!(
                    has_result,
                    "dangling tool_call {} (no tool result) in: {:?}",
                    call.id, messages
                );
            }
        }
    }

    fn input(messages: Vec<LlmMessage>) -> EngineTurnInput {
        EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://example.test/v1".to_string(),
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: Some(5),
            cwd: Some("/tmp".to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        }
    }

    fn policy(mode: PermissionMode) -> PolicyConfig {
        PolicyConfig {
            mode,
            rules: vec![],
            session_approved_patterns: vec![],
        }
    }

    /// One assistant message with two tool calls: Bash (manual mode asks) +
    /// Read (default-approve sibling).
    fn batch_segment() -> Vec<LlmStreamEvent> {
        vec![
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_bash".to_string(),
                name: Some("Bash".to_string()),
                arguments_part: Some("{\"command\":\"echo hi\"}".to_string()),
            },
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_read".to_string(),
                name: Some("Read".to_string()),
                arguments_part: Some("{\"path\":\"hello.txt\"}".to_string()),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("tool_calls".to_string()),
            },
        ]
    }

    /// One final text step ("done") — the deny / rejection flows need a
    /// second LLM step after the tool result to end the turn.
    fn done_segment() -> Vec<LlmStreamEvent> {
        vec![
            LlmStreamEvent::Text {
                delta: "done".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]
    }

    /// One Bash tool-call step followed by a done step — used twice (two
    /// sessions, with and without the worker-guidance flag).
    fn bash_then_done_segments() -> Vec<Vec<LlmStreamEvent>> {
        vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_bash".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo hi\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            done_segment(),
        ]
    }

    #[tokio::test]
    async fn approval_resume_continues_the_tool_batch() {
        // P1-1: an approval pauses the batch — the remaining siblings must
        // still run after the Approved resume (TS parity: approval pauses,
        // it does not cancel the round). Bash asks in manual mode; Read is a
        // default-approve tool.
        let llm = ScriptedLlmClient::new(vec![
            batch_segment(),
            vec![
                LlmStreamEvent::Text {
                    delta: "done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash", "Bash", "bash output", false),
            result("call_read", "Read", "read output", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        let TurnProgress::NeedsApproval(request) = progress else {
            panic!("expected NeedsApproval, got {progress:?}");
        };
        assert_eq!(request.tool_call_id, "call_bash");
        // The batch call itself has not run before the pause.
        assert_eq!(*executor.executed.lock().unwrap(), Vec::<String>::new());

        let progress = session
            .resume(
                ApprovalDecision::Approved,
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        // BOTH batch calls ran — the sibling is no longer dropped.
        assert_eq!(
            *executor.executed.lock().unwrap(),
            vec!["call_bash".to_string(), "call_read".to_string()]
        );
        // The next request has no dangling tool_call: every assistant
        // tool_calls entry has a matching tool result.
        assert_no_dangling_tool_calls(session.messages());
        let tool_results: Vec<_> = session
            .messages()
            .iter()
            .filter(|m| m.role == "tool")
            .collect();
        assert_eq!(tool_results.len(), 2, "both batch calls have results");
        assert_eq!(tool_results[0].tool_call_id.as_deref(), Some("call_bash"));
        assert_eq!(tool_results[1].tool_call_id.as_deref(), Some("call_read"));
    }

    #[tokio::test]
    async fn stop_turn_in_batch_synthesizes_skipped_sibling_results() {
        // P2-7: when a batch call stops the turn, the unrun siblings still
        // get a synthetic error result so no tool_call dangles (TS
        // toolExecutorService parity).
        let llm = ScriptedLlmClient::once(batch_segment());
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash", "Bash", "stopping now", true),
            // call_read must NOT run.
            result("call_read", "Read", "should not run", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Auto),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        // Only the stopping call ran.
        assert_eq!(
            *executor.executed.lock().unwrap(),
            vec!["call_bash".to_string()]
        );
        // The sibling has a synthetic tool result — nothing dangles.
        assert_no_dangling_tool_calls(session.messages());
        let sibling = session
            .messages()
            .iter()
            .find(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("call_read"))
            .expect("sibling tool result");
        assert!(
            sibling
                .content
                .as_str()
                .unwrap()
                .contains("Tool skipped because a previous tool call stopped the turn."),
            "sibling result: {:?}",
            sibling.content
        );
    }

    /// Counts `ToolCallStarted` events for a call id.
    fn started_count(events: &[EngineEvent], call_id: &str) -> usize {
        events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EngineEvent::ToolCallStarted { tool_call_id, .. } if tool_call_id == call_id
                )
            })
            .count()
    }

    /// Counts `ToolResult` events for a call id.
    fn result_count(events: &[EngineEvent], call_id: &str) -> usize {
        events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EngineEvent::ToolResult { tool_call_id, .. } if tool_call_id == call_id
                )
            })
            .count()
    }

    /// The `ToolResult` event's output for a call id (panics when missing).
    fn result_output(events: &[EngineEvent], call_id: &str) -> String {
        events
            .iter()
            .find_map(|e| match e {
                EngineEvent::ToolResult { tool_call_id, output, .. } if tool_call_id == call_id => {
                    Some(output.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no tool.result for {call_id} in {events:?}"))
    }

    /// A manual-mode policy that denies `Bash` outright (user-configured
    /// deny rule with a reason — the `PolicyDecision::Deny` path).
    fn deny_policy() -> PolicyConfig {
        PolicyConfig {
            mode: PermissionMode::Manual,
            rules: vec![crate::permission::PermissionRule {
                decision: crate::permission::RuleDecision::Deny,
                scope: "user".to_string(),
                pattern: "Bash".to_string(),
                reason: Some("no bash".to_string()),
            }],
            session_approved_patterns: vec![],
        }
    }

    #[tokio::test]
    async fn approval_resume_emits_tool_started_exactly_once() {
        // P2-2 (final review): an Approved resume must not emit a second
        // `ToolCallStarted` for the pending call — the started event lands
        // exactly once per call, at the dispatch point after the approval
        // decision (TS toolExecutorService parity).
        let llm = ScriptedLlmClient::new(vec![
            batch_segment(),
            vec![
                LlmStreamEvent::Text {
                    delta: "done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash", "Bash", "bash output", false),
            result("call_read", "Read", "read output", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::NeedsApproval(_)), "{progress:?}");
        let progress = session
            .resume(
                ApprovalDecision::Approved,
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        // Each call got exactly one started + one result — no duplicate
        // started from pause + resume.
        assert_eq!(started_count(&events, "call_bash"), 1, "events: {events:?}");
        assert_eq!(result_count(&events, "call_bash"), 1, "events: {events:?}");
        assert_eq!(started_count(&events, "call_read"), 1, "events: {events:?}");
        assert_eq!(result_count(&events, "call_read"), 1, "events: {events:?}");
    }

    #[tokio::test]
    async fn skipped_sibling_emits_started_and_result() {
        // P2-1 (final review): a synthetic skipped sibling must be announced
        // (`tool.call.started`) before its result, exactly like TS
        // `prepareSkippedToolCall` — no orphan `tool.result` on the wire.
        let llm = ScriptedLlmClient::once(batch_segment());
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash", "Bash", "stopping now", true),
            result("call_read", "Read", "should not run", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Auto),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        assert_eq!(started_count(&events, "call_read"), 1, "events: {events:?}");
        assert_eq!(result_count(&events, "call_read"), 1, "events: {events:?}");
    }

    #[tokio::test]
    async fn paused_step_emits_step_completed_after_resume() {
        // P2-3 (final review): the step that paused for approval must still
        // emit `TurnStepCompleted` once the resumed batch finishes (TS loop
        // parity: the approval round-trip happens inside the step's tool
        // phase, and `step.end` is emitted after the batch resolves), so the
        // next LLM step starts cleanly.
        let llm = ScriptedLlmClient::new(vec![
            batch_segment(),
            vec![
                LlmStreamEvent::Text {
                    delta: "done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash", "Bash", "bash output", false),
            result("call_read", "Read", "read output", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::NeedsApproval(_)), "{progress:?}");
        let progress = session
            .resume(
                ApprovalDecision::Approved,
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        let completed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::TurnStepCompleted { step, finish_reason, .. } => {
                    Some((*step, finish_reason.clone()))
                }
                _ => None,
            })
            .collect();
        assert!(
            completed.iter().any(|(step, _)| *step == 1),
            "paused step 1 never completed; completed: {completed:?}, events: {events:?}"
        );
        // The step that carried the paused batch finishes as tool_calls
        // (normalized to `tool_use` like the non-paused path).
        assert_eq!(
            completed
                .iter()
                .find(|(step, _)| *step == 1)
                .map(|(_, reason)| reason.clone()),
            Some(Some("tool_use".to_string())),
            "step 1 finish_reason: {completed:?}"
        );
        // Ordering: the paused step's completion lands strictly before the
        // next step starts, so the transcript never sees a step open across
        // the approval boundary.
        let step1_completed = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    EngineEvent::TurnStepCompleted { step: 1, .. }
                )
            })
            .expect("step 1 completed");
        let step2_started = events
            .iter()
            .position(|e| matches!(e, EngineEvent::TurnStepStarted { step: 2, .. }))
            .expect("step 2 started");
        assert!(
            step1_completed < step2_started,
            "step 1 completed at {step1_completed} must precede step 2 started at {step2_started}: {events:?}"
        );
    }

    #[tokio::test]
    async fn resume_honors_session_pattern_added_before_resume() {
        // P1-6 (review): the bridge records a session-scope approval into
        // the live policy BEFORE resume — the resumed batch must auto-approve
        // the same tool instead of surfacing a second approval request.
        // Both batch calls are Ask tools in manual mode.
        let llm = ScriptedLlmClient::new(vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_bash_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo 1\"}".to_string()),
                },
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_bash_2".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo 2\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![
                LlmStreamEvent::Text {
                    delta: "done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash_1", "Bash", "out1", false),
            result("call_bash_2", "Bash", "out2", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |_| {},
            )
            .await;
        assert!(
            matches!(progress, TurnProgress::NeedsApproval(ref r) if r.tool_call_id == "call_bash_1"),
            "{progress:?}"
        );
        // The bridge appended the session pattern to the live policy before
        // resuming (add_session_approval + the engine re-reads the policy).
        let mut live = policy(PermissionMode::Manual);
        live.session_approved_patterns = vec!["Bash".to_string()];
        let progress = session
            .resume(
                ApprovalDecision::Approved,
                &llm,
                &executor,
                &live,
                &mut |_| {},
            )
            .await;
        assert!(
            matches!(progress, TurnProgress::Completed(_)),
            "second Bash must auto-approve via the session pattern, got {progress:?}"
        );
        // Both calls ran; no dangling tool_call.
        assert_eq!(
            *executor.executed.lock().unwrap(),
            vec!["call_bash_1".to_string(), "call_bash_2".to_string()]
        );
        assert_no_dangling_tool_calls(session.messages());
    }

    #[tokio::test]
    async fn rejected_resume_emits_started_once_before_result() {
        // P2-2 (final review, rejected path): a rejected resume announces the
        // call exactly once and only after the decision, before the result
        // (TS toolExecutorService dispatch parity) — the fold still gets a
        // complete started + result pair.
        let llm = ScriptedLlmClient::new(vec![
            batch_segment(),
            vec![
                LlmStreamEvent::Text {
                    delta: "done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash", "Bash", "bash output", false),
            result("call_read", "Read", "read output", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::NeedsApproval(_)), "{progress:?}");
        let progress = session
            .resume(
                ApprovalDecision::Rejected {
                    feedback: Some("no thanks".to_string()),
                },
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        assert_eq!(started_count(&events, "call_bash"), 1, "events: {events:?}");
        assert_eq!(result_count(&events, "call_bash"), 1, "events: {events:?}");
        let started_idx = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    EngineEvent::ToolCallStarted { tool_call_id, .. } if tool_call_id == "call_bash"
                )
            })
            .expect("call_bash started");
        let result_idx = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    EngineEvent::ToolResult { tool_call_id, .. } if tool_call_id == "call_bash"
                )
            })
            .expect("call_bash result");
        assert!(
            started_idx < result_idx,
            "started at {started_idx} must precede result at {result_idx}: {events:?}"
        );
        // The sibling still ran after the rejected pending call.
        assert_eq!(
            *executor.executed.lock().unwrap(),
            vec!["call_read".to_string()]
        );
    }

    #[tokio::test]
    async fn deny_output_appends_worker_guidance_when_requested() {
        // TS `formatDenyMessage` parity: with `uses_worker_rejection_guidance`
        // (subagent/worker turns) the permission-deny tool output carries the
        // guidance suffix; without it the deny message is unchanged.
        // Two sessions, each consuming a Bash-call step + a done step.
        let llm = ScriptedLlmClient::new([
            bash_then_done_segments(),
            bash_then_done_segments(),
        ].concat());
        let executor = RecordingToolExecutor::new(vec![]);
        let base = "Tool \"Bash\" was denied by permission rule. Reason: no bash";

        // Worker guidance on: the suffix is appended (leading space, exact TS
        // text).
        let mut session = TurnSession::new(input(vec![msg("user", "run")]));
        session.input.uses_worker_rejection_guidance = true;
        let mut events = Vec::new();
        let progress = session
            .run(&llm, &executor, &deny_policy(), &mut |event| events.push(event))
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        assert_eq!(
            result_output(&events, "call_bash"),
            format!("{base}{WORKER_REJECTION_GUIDANCE_SUFFIX}"),
            "events: {events:?}"
        );

        // Guidance off: the deny message is unchanged.
        let mut session = TurnSession::new(input(vec![msg("user", "run")]));
        session.input.uses_worker_rejection_guidance = false;
        let mut events = Vec::new();
        let progress = session
            .run(&llm, &executor, &deny_policy(), &mut |event| events.push(event))
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        assert_eq!(result_output(&events, "call_bash"), base);
    }

    #[tokio::test]
    async fn rejected_resume_appends_worker_guidance_when_requested() {
        // TS `formatApprovalRejectionMessage` parity: a rejected approval
        // carries the guidance suffix for worker turns (with feedback, the
        // suffix lands after ` Reason: …`, exactly like TS) and stays
        // unchanged otherwise.
        // Two sessions, each consuming the approval batch + a done step.
        let llm = ScriptedLlmClient::new(vec![
            batch_segment(),
            done_segment(),
            batch_segment(),
            done_segment(),
        ]);
        let executor = RecordingToolExecutor::new(vec![result(
            "call_read",
            "Read",
            "read output",
            false,
        )]);
        let base =
            "Tool \"Bash\" was not run because the user rejected the approval request. Reason: no thanks";

        // Worker guidance on: suffix appended after ` Reason: no thanks`.
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        session.input.uses_worker_rejection_guidance = true;
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::NeedsApproval(_)), "{progress:?}");
        let progress = session
            .resume(
                ApprovalDecision::Rejected {
                    feedback: Some("no thanks".to_string()),
                },
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        assert_eq!(
            result_output(&events, "call_bash"),
            format!("{base}{WORKER_REJECTION_GUIDANCE_SUFFIX}"),
            "events: {events:?}"
        );

        // Guidance off: the rejection message is unchanged.
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        session.input.uses_worker_rejection_guidance = false;
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::NeedsApproval(_)), "{progress:?}");
        let progress = session
            .resume(
                ApprovalDecision::Rejected {
                    feedback: Some("no thanks".to_string()),
                },
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        assert_eq!(result_output(&events, "call_bash"), base);
    }

    #[tokio::test]
    async fn approved_stop_turn_resume_synthesizes_skipped_and_completes_step() {
        // P2-1/P2-3 (final review, Path A): when the approved pending call
        // itself stops the turn, the unrun siblings still get announced
        // (started) + synthetic result, and the paused step completes with
        // `end_turn` before the turn ends.
        let llm = ScriptedLlmClient::once(batch_segment());
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash", "Bash", "stopping after approval", true),
            result("call_read", "Read", "should not run", false),
        ]);
        let mut session = TurnSession::new(input(vec![msg("user", "run both")]));
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::NeedsApproval(_)), "{progress:?}");
        let progress = session
            .resume(
                ApprovalDecision::Approved,
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(matches!(progress, TurnProgress::Completed(_)), "{progress:?}");
        // Only the approved pending call ran.
        assert_eq!(
            *executor.executed.lock().unwrap(),
            vec!["call_bash".to_string()]
        );
        // The skipped sibling is announced before its synthetic result.
        let sibling_started = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    EngineEvent::ToolCallStarted { tool_call_id, .. } if tool_call_id == "call_read"
                )
            })
            .expect("sibling started");
        let sibling_result = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    EngineEvent::ToolResult { tool_call_id, .. } if tool_call_id == "call_read"
                )
            })
            .expect("sibling result");
        assert!(
            sibling_started < sibling_result,
            "sibling started at {sibling_started} must precede result at {sibling_result}: {events:?}"
        );
        // The paused step completes with end_turn, and no next step starts.
        let completed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::TurnStepCompleted { step, finish_reason, .. } => {
                    Some((*step, finish_reason.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            completed,
            vec![(1, Some("end_turn".to_string()))],
            "events: {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(e, EngineEvent::TurnStepStarted { step: 2, .. })),
            "turn must end after step 1: {events:?}"
        );
    }

    #[tokio::test]
    async fn cancel_during_pending_approval_stops_the_turn() {
        // P1-5 (review): once the user cancelled (TaskStop / session close),
        // a resume must not keep executing the batch or re-ask — the engine
        // checks the cancel signal and finishes cancelled. Both batch calls
        // are Ask tools: without the cancel check the resumed batch would
        // surface a SECOND NeedsApproval (the user already cancelled), and
        // with the in-flight select race a sibling could still run.
        let llm = ScriptedLlmClient::new(vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_bash_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo 1\"}".to_string()),
                },
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_bash_2".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo 2\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![
                LlmStreamEvent::Text {
                    delta: "done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let executor = RecordingToolExecutor::new(vec![
            result("call_bash_1", "Bash", "out1", false),
            result("call_bash_2", "Bash", "out2", false),
        ]);
        let cancel = std::sync::Arc::new(CancelSignal::new());
        let mut session = TurnSession::with_steer_and_cancel(
            input(vec![msg("user", "run both")]),
            None,
            std::sync::Arc::clone(&cancel),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut events = Vec::new();
        let progress = session
            .run(
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        assert!(
            matches!(progress, TurnProgress::NeedsApproval(ref r) if r.tool_call_id == "call_bash_1"),
            "{progress:?}"
        );
        // The user cancelled while the approval was pending.
        cancel.cancel();
        let progress = session
            .resume(
                ApprovalDecision::Cancelled,
                &llm,
                &executor,
                &policy(PermissionMode::Manual),
                &mut |event| events.push(event),
            )
            .await;
        match progress {
            TurnProgress::Completed(outcome) => {
                assert_eq!(outcome.status, TurnEndReason::Cancelled, "{outcome:?}");
            }
            other => panic!("expected Completed(Cancelled), got {other:?}"),
        }
        // No sibling ran after the cancellation.
        assert_eq!(
            *executor.executed.lock().unwrap(),
            Vec::<String>::new(),
            "no sibling may run after the user cancelled"
        );
        // P2-6 (review): the paused step is interrupted on cancel (like the
        // in-flight cancel paths), so the transcript does not leave it open.
        assert!(
            events.iter().any(|e| matches!(
                e,
                EngineEvent::TurnStepInterrupted { reason, .. } if reason == "aborted"
            )),
            "cancel must interrupt the paused step: {events:?}"
        );
    }
}

#[cfg(test)]
mod dedupe_tests {
    use super::*;
    use crate::dedupe::{REMINDER_TEXT_1, REMINDER_TEXT_3, make_reminder_text_2};
    use crate::llm::{LlmStreamEvent, ScriptedLlmClient, StreamedTurn};
    use crate::permission::{PermissionMode, PolicyConfig};
    use crate::types::ProviderConfig;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    fn input(messages: Vec<LlmMessage>) -> EngineTurnInput {
        EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
            active_tools: None,
            provider: ProviderConfig {
                base_url: "http://example.test/v1".to_string(),
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: None,
            cwd: Some(std::env::temp_dir().to_string_lossy().to_string()),
            shell: Some("/bin/sh".to_string()),
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            max_retries_per_step: None,
            completion_review: None,
            origin: TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        }
    }

    fn user_message(text: &str) -> LlmMessage {
        LlmMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    fn policy_auto() -> PolicyConfig {
        PolicyConfig {
            mode: PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        }
    }

    /// One step's LLM segment: `(id, command)` tool calls, then the
    /// `tool_calls` finish.
    fn tool_call_segment(calls: &[(&str, &str)]) -> Vec<LlmStreamEvent> {
        let mut events = Vec::new();
        for (id, command) in calls {
            events.push(LlmStreamEvent::ToolCall {
                tool_call_id: id.to_string(),
                name: Some("Bash".to_string()),
                arguments_part: Some(format!(r#"{{"command":"{command}"}}"#)),
            });
        }
        events.push(LlmStreamEvent::Finish {
            finish_reason: Some("tool_calls".to_string()),
        });
        events
    }

    fn text_segment(text: &str) -> Vec<LlmStreamEvent> {
        vec![
            LlmStreamEvent::Text {
                delta: text.to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]
    }

    /// Scripted client that also records every request's messages (so the
    /// tests can assert what the LLM actually saw).
    struct RecordingScripted {
        recorded: Arc<Mutex<Vec<Vec<LlmMessage>>>>,
        inner: ScriptedLlmClient,
    }

    #[async_trait::async_trait]
    impl LlmClient for RecordingScripted {
        async fn stream_chat(
            &self,
            request: &ChatRequest,
        ) -> Result<StreamedTurn, crate::llm::LlmError> {
            self.recorded.lock().unwrap().push(request.messages.clone());
            self.inner.stream_chat(request).await
        }
    }

    /// Counts executions and returns a per-call-id output (so shared vs
    /// executed results are distinguishable).
    struct CountingExecutor {
        executed: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ToolExecutor for CountingExecutor {
        async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
            self.executed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: format!("executed-{}", call.id),
                is_error: false,
                stop_turn: false,
                updates: vec![],
            }
        }
    }

    fn tool_results(events: &[EngineEvent]) -> Vec<&EngineEvent> {
        events
            .iter()
            .filter(|event| matches!(event, EngineEvent::ToolResult { .. }))
            .collect()
    }

    fn tool_call_started(events: &[EngineEvent]) -> Vec<&EngineEvent> {
        events
            .iter()
            .filter(|event| matches!(event, EngineEvent::ToolCallStarted { .. }))
            .collect()
    }

    struct CancelFirstExecutor {
        active_step: Arc<Mutex<Option<Arc<CancelSignal>>>>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ToolExecutor for CancelFirstExecutor {
        async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                self.active_step
                    .lock()
                    .unwrap()
                    .as_ref()
                    .expect("active step signal")
                    .cancel();
            }
            ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: format!("result-{}", call.id),
                is_error: false,
                stop_turn: false,
                updates: vec![],
            }
        }
    }

    #[tokio::test]
    async fn same_step_duplicate_is_suppressed_and_shares_the_result() {
        // Step 1: the model emits TWO identical Bash calls in one step. The
        // second must not execute; both `tool.call.started` + `tool.result`
        // pairs must exist and the second result shares the first's output.
        let llm = RecordingScripted {
            recorded: Arc::new(Mutex::new(Vec::new())),
            inner: ScriptedLlmClient::new(vec![
                tool_call_segment(&[("call_1", "echo hi"), ("call_2", "echo hi")]),
                text_segment("done"),
            ]),
        };
        let executed = Arc::new(AtomicUsize::new(0));
        let tools = CountingExecutor {
            executed: Arc::clone(&executed),
        };
        let mut events = Vec::new();
        let engine = Engine::default();
        let outcome = engine
            .run_turn(
                &input(vec![user_message("go")]),
                &llm,
                &tools,
                &policy_auto(),
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Completed);
        assert_eq!(outcome.steps, 2);
        assert_eq!(
            executed.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "the duplicate must not execute the tool again"
        );

        let started = tool_call_started(&events);
        assert_eq!(started.len(), 2, "both calls are announced");
        let started_values: Vec<serde_json::Value> =
            started.iter().map(|e| serde_json::to_value(e).unwrap()).collect();
        assert_eq!(started_values[0]["toolCallId"], "call_1");
        assert_eq!(started_values[1]["toolCallId"], "call_2");

        let results = tool_results(&events);
        assert_eq!(results.len(), 2, "both calls have a result");
        let result_values: Vec<serde_json::Value> =
            results.iter().map(|e| serde_json::to_value(e).unwrap()).collect();
        assert_eq!(result_values[0]["toolCallId"], "call_1");
        assert_eq!(result_values[1]["toolCallId"], "call_2");
        assert_eq!(result_values[0]["output"], "executed-call_1");
        assert_eq!(
            result_values[1]["output"], "executed-call_1",
            "the duplicate shares the original's output"
        );

        // The model's next request carries BOTH tool messages with the same
        // content (TS pushes a tool message per dispatched call).
        let recorded = llm.recorded.lock().unwrap();
        assert_eq!(recorded.len(), 2, "one request per step");
        let tool_messages: Vec<&LlmMessage> = recorded[1]
            .iter()
            .filter(|m| m.role == "tool")
            .collect();
        assert_eq!(tool_messages.len(), 2);
        assert_eq!(tool_messages[0].content, tool_messages[1].content);
        assert_eq!(
            tool_messages[0].tool_call_id.as_deref(),
            Some("call_1")
        );
        assert_eq!(
            tool_messages[1].tool_call_id.as_deref(),
            Some("call_2")
        );
    }

    #[tokio::test]
    async fn cross_step_repeats_append_reminders_at_3_5_and_8() {
        // One identical call per step: the 3rd result gets REMINDER_TEXT_1,
        // the 5th makeReminderText2(5), the 8th REMINDER_TEXT_3.
        let mut segments = Vec::new();
        for step in 1..=8u32 {
            segments.push(tool_call_segment(&[(&format!("call_{step}"), "echo x")]));
        }
        segments.push(text_segment("done"));
        let llm = ScriptedLlmClient::new(segments);
        let tools = CountingExecutor {
            executed: Arc::new(AtomicUsize::new(0)),
        };
        let mut events = Vec::new();
        let engine = Engine::default();
        let outcome = engine
            .run_turn(
                &input(vec![user_message("go")]),
                &llm,
                &tools,
                &policy_auto(),
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Completed);
        assert_eq!(outcome.steps, 9);
        assert_eq!(
            tools.executed.load(std::sync::atomic::Ordering::Relaxed),
            8,
            "every repeat still executes (cross-step repeats are reminders, not suppression)"
        );

        let results = tool_results(&events);
        assert_eq!(results.len(), 8);
        let outputs: Vec<String> = results
            .iter()
            .map(|e| {
                serde_json::to_value(e).unwrap()["output"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        // 1st and 2nd: no reminder.
        assert_eq!(outputs[0], "executed-call_1");
        assert_eq!(outputs[1], "executed-call_2");
        // 3rd and 4th: REMINDER_TEXT_1.
        assert_eq!(outputs[2], format!("executed-call_3{REMINDER_TEXT_1}"));
        assert_eq!(outputs[3], format!("executed-call_4{REMINDER_TEXT_1}"));
        // 5th-7th: makeReminderText2 with the streak.
        assert_eq!(outputs[4], format!("executed-call_5{}", make_reminder_text_2(5)));
        assert_eq!(outputs[5], format!("executed-call_6{}", make_reminder_text_2(6)));
        assert_eq!(outputs[6], format!("executed-call_7{}", make_reminder_text_2(7)));
        // 8th: REMINDER_TEXT_3.
        assert_eq!(outputs[7], format!("executed-call_8{REMINDER_TEXT_3}"));
    }

    #[tokio::test]
    async fn twelve_consecutive_repeats_force_stop_the_turn() {
        // The 12th identical call force-stops the turn (REPEAT_FORCE_STOP_STREAK):
        // the turn completes right after step 12 with no further LLM request.
        let mut segments = Vec::new();
        for step in 1..=12u32 {
            segments.push(tool_call_segment(&[(&format!("call_{step}"), "echo x")]));
        }
        let llm = RecordingScripted {
            recorded: Arc::new(Mutex::new(Vec::new())),
            inner: ScriptedLlmClient::new(segments),
        };
        let tools = CountingExecutor {
            executed: Arc::new(AtomicUsize::new(0)),
        };
        let mut events = Vec::new();
        let engine = Engine::default();
        let outcome = engine
            .run_turn(
                &input(vec![user_message("go")]),
                &llm,
                &tools,
                &policy_auto(),
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Completed);
        assert_eq!(outcome.steps, 12, "the turn stops after the 12th step");
        let recorded = llm.recorded.lock().unwrap();
        assert_eq!(recorded.len(), 12, "no step-13 LLM request after the stop");
        drop(recorded);

        let results = tool_results(&events);
        assert_eq!(results.len(), 12);
        let last = serde_json::to_value(results.last().unwrap()).unwrap();
        assert_eq!(last["toolCallId"], "call_12");
        assert_eq!(
            last["output"],
            format!("executed-call_12{REMINDER_TEXT_3}"),
            "the force-stop result carries REMINDER_TEXT_3"
        );
        // The earlier results are unaffected.
        let third = serde_json::to_value(&results[2]).unwrap();
        assert_eq!(
            third["output"],
            format!("executed-call_3{REMINDER_TEXT_1}")
        );
    }

    #[tokio::test]
    async fn different_args_are_not_deduplicated() {
        // Two calls with different args in the same step both execute and
        // produce their own results.
        let llm = ScriptedLlmClient::new(vec![
            tool_call_segment(&[("call_a", "echo a"), ("call_b", "echo b")]),
            text_segment("done"),
        ]);
        let tools = CountingExecutor {
            executed: Arc::new(AtomicUsize::new(0)),
        };
        let mut events = Vec::new();
        let engine = Engine::default();
        let outcome = engine
            .run_turn(
                &input(vec![user_message("go")]),
                &llm,
                &tools,
                &policy_auto(),
                &mut |event| events.push(event),
            )
            .await;

        assert_eq!(outcome.status, TurnEndReason::Completed);
        assert_eq!(
            tools.executed.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "different args are distinct calls"
        );
        let results = tool_results(&events);
        assert_eq!(results.len(), 2);
        let values: Vec<serde_json::Value> =
            results.iter().map(|e| serde_json::to_value(e).unwrap()).collect();
        assert_eq!(values[0]["output"], "executed-call_a");
        assert_eq!(values[1]["output"], "executed-call_b");
        assert_ne!(values[0]["output"], values[1]["output"]);
    }

    #[tokio::test]
    async fn cancelled_tool_step_does_not_advance_the_cross_step_dedupe_streak() {
        let repeated = |id: &str| {
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: id.to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some(r#"{"command":"echo same"}"#.to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ]
        };
        let llm = ScriptedLlmClient::new(vec![
            repeated("cancelled"),
            repeated("second"),
            repeated("third"),
            text_segment("done"),
        ]);
        let mut session = TurnSession::new(input(vec![user_message("go")]));
        let active_step = session.active_step_cancel_handle();
        let tools = CancelFirstExecutor {
            active_step,
            calls: AtomicUsize::new(0),
        };
        let mut events = Vec::new();
        let outcome = session
            .run(&llm, &tools, &policy_auto(), &mut |event| events.push(event))
            .await;

        let TurnProgress::Completed(outcome) = outcome else {
            panic!("turn must complete: {outcome:?}");
        };
        assert_eq!(outcome.status, TurnEndReason::Completed);
        let results = tool_results(&events);
        assert_eq!(results.len(), 3);
        let third_output = serde_json::to_value(results[2]).unwrap()["output"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            !third_output.contains(REMINDER_TEXT_1),
            "the interrupted first step must not count toward the streak: {events:?}"
        );
    }
}
