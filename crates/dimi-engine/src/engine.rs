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

use dimi_wire::model::{TranscriptUsage, TurnOrigin};

use crate::events::{EngineEvent, FinishReason};
use crate::llm::{ChatRequest, LlmClient, LlmStreamEvent};
use crate::permission::{
    ApprovalDecision, ApprovalRequest, PolicyConfig, PolicyDecision, PolicyInput, evaluate,
};
use crate::tool::{ToolCall, ToolContext, ToolExecutor, ToolResult};
use crate::types::{EngineTurnInput, LlmMessage, TurnEndReason, TurnOutcome};

/// Engine configuration.
#[derive(Debug, Clone)]
pub struct Engine {
    /// Step limit; `None` = unlimited (mirrors `maxStepsPerTurn` unset).
    pub max_steps_per_turn: Option<u32>,
    /// Shell for Bash tool execution (default `/bin/sh`).
    pub shell: String,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            max_steps_per_turn: None,
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
    /// Tool calls to run through the policy gate.
    Continue(Vec<ToolCall>),
}

/// Accumulates usage across steps; per-step usage is reported in
/// `turn.step.completed` (the projection layer folds step usages into the
/// turn header).
#[derive(Debug, Clone, Default)]
struct UsageAccumulator {
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
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
                self.reasoning_tokens += details.reasoning_tokens.unwrap_or(0);
            }
            if let Some(details) = completion_tokens_details {
                self.reasoning_tokens += details.reasoning_tokens.unwrap_or(0);
            }
        }
    }

    fn transcript_usage(&self) -> Option<TranscriptUsage> {
        if self.prompt_tokens == 0 && self.completion_tokens == 0 {
            return None;
        }
        Some(TranscriptUsage {
            input_tokens: Some((self.prompt_tokens + self.cached_tokens) as i64),
            output_tokens: Some(self.completion_tokens as i64),
            cached_tokens: Some(self.cached_tokens as i64),
            cost: None,
        })
    }
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
                        session
                            .resume(
                                ApprovalDecision::Rejected { feedback: None },
                                llm,
                                tools,
                                policy,
                                on_event,
                            )
                            .await
                            .into_completed_or_failed()
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
#[derive(Debug, Default)]
pub struct CancelSignal {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl CancelSignal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::Relaxed);
        self.notify.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::Relaxed)
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
    /// Cooperative cancellation (RPC cancel → engine).
    cancel: std::sync::Arc<CancelSignal>,
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
}

/// Where the turn stands after `run` / `resume`.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnProgress {
    Completed(TurnOutcome),
    NeedsApproval(ApprovalRequest),
}

impl TurnProgress {
    /// Non-interactive fallback: collapse any state into an outcome.
    pub fn into_completed_or_failed(self) -> TurnOutcome {
        match self {
            TurnProgress::Completed(outcome) => outcome,
            TurnProgress::NeedsApproval(_) => TurnOutcome {
                status: TurnEndReason::Failed,
                steps: 0,
                error: Some("approval pending without a resolver".to_string()),
                error_code: Some("APPROVAL_PENDING".to_string()),
                truncated: None,
            },
        }
    }
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
        Self {
            input,
            messages: Vec::new(),
            steps: 0,
            started_at: Instant::now(),
            pending: None,
            steer,
            last_compacted_tokens: None,
            cancel,
            finished,
        }
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
        let result = match decision {
            ApprovalDecision::Approved => {
                let ctx = self.tool_ctx();
                tokio::select! {
                    result = execute_tool(self.input.turn_id, pending.call.clone(), tools, &ctx, on_event) => result,
                    _ = self.cancel.cancelled() => {
                        tools.abort(&pending.call);
                        emit(
                            on_event,
                            EngineEvent::TurnStepInterrupted {
                                turn_id: self.input.turn_id,
                                step: self.steps as i64,
                                step_id: None,
                                reason: "aborted".to_string(),
                                message: None,
                            },
                        );
                        return self.finish_turn_with_error(
                            TurnEndReason::Cancelled,
                            None,
                            None,
                            on_event,
                        );
                    }
                }
            }
            ApprovalDecision::Rejected { feedback } => ToolResult {
                tool_call_id: pending.call.id.clone(),
                tool_name: pending.call.name.clone(),
                output: match feedback {
                    Some(reason) if !reason.is_empty() => format!(
                        "Tool \"{}\" was not run because the user rejected the approval request. Reason: {}",
                        pending.call.name, reason
                    ),
                    _ => format!(
                        "Tool \"{}\" was not run because the user rejected the approval request.",
                        pending.call.name
                    ),
                },
                is_error: true,
                stop_turn: false,
                updates: vec![],
            },
            ApprovalDecision::Cancelled => ToolResult {
                tool_call_id: pending.call.id.clone(),
                tool_name: pending.call.name.clone(),
                output: format!(
                    "Tool \"{}\" was not run because the approval request was cancelled.",
                    pending.call.name
                ),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            },
        };
        emit_tool_result(&result, self.input.turn_id, on_event);
        self.messages.push(LlmMessage {
            role: "tool".to_string(),
            content: serde_json::Value::String(result.output.clone()),
            name: Some(result.tool_name.clone()),
            tool_call_id: Some(result.tool_call_id.clone()),
            tool_calls: None,
            reasoning: None,
        });
        if result.stop_turn {
            return self.finish_turn(TurnEndReason::Completed, on_event);
        }
        self.run_loop(llm, tools, policy, on_event).await
    }

    fn tool_ctx(&self) -> ToolContext {
        ToolContext {
            cwd: self.input.cwd.clone().unwrap_or_else(|| ".".to_string()),
            shell: self
                .input
                .shell
                .clone()
                .unwrap_or_else(dimi_exec::env::default_shell),
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

        let mut history = self.messages.clone();
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
                    let text = turn.assistant.text.trim().to_string();
                    if !text.is_empty() {
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
        let origin = TurnOrigin::User { payload: None };
        let prompt = last_user_text(&self.input.messages);
        emit(
            on_event,
            EngineEvent::TurnStarted {
                turn_id,
                origin: origin.clone(),
                prompt,
            },
        );

        loop {
            // Cancellation (RPC cancel → engine): checked at every step
            // boundary; the in-flight LLM/tool awaits race it too.
            if self.cancel.is_cancelled() {
                return self.finish_turn_with_error(TurnEndReason::Cancelled, None, None, on_event);
            }

            // max-steps guard.
            if let Some(max) = self.input.max_steps_per_turn {
                if max > 0 && self.steps >= max {
                    emit(
                        on_event,
                        EngineEvent::TurnStepInterrupted {
                            turn_id,
                            step: self.steps as i64,
                            step_id: None,
                            reason: "max_steps".to_string(),
                            message: Some(format!(
                                "Turn exceeded maxSteps={max}. If max_steps_per_turn is too small, raise it in config.toml (loop_control.max_steps_per_turn)"
                            )),
                        },
                    );
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
                    if crate::compaction::should_compact(estimated, max) && !already_compacted {
                        let _ = self.compact(llm, on_event).await;
                    }
                }
            }

            self.steps += 1;
            let step_number = self.steps;
            emit(
                on_event,
                EngineEvent::TurnStepStarted {
                    turn_id,
                    step: step_number as i64,
                    step_id: None,
                },
            );

            let mut usage = UsageAccumulator::default();
            let request_messages = match self.input.context_window {
                Some(window) if window > 0 => {
                    crate::context::project_window(&self.messages, window)
                }
                _ => self.messages.clone(),
            };
            let request = ChatRequest {
                messages: request_messages,
                tools: Some(
                    self.input
                        .tools
                        .iter()
                        .map(|tool| {
                            serde_json::json!({
                                "type": "function",
                                "function": {
                                    "name": tool.name,
                                    "description": tool.description,
                                    "parameters": tool.args_schema,
                                }
                            })
                        })
                        .collect(),
                ),
                model: Some(self.input.provider.model.clone()),
                thinking_effort: self.input.provider.thinking_effort.clone(),
            };

            let disposition = match tokio::select! {
                result = execute_step(turn_id, llm, &request, &mut usage, on_event) => result,
                _ = self.cancel.cancelled() => {
                    emit(
                        on_event,
                        EngineEvent::TurnStepInterrupted {
                            turn_id,
                            step: step_number as i64,
                            step_id: None,
                            reason: "aborted".to_string(),
                            message: None,
                        },
                    );
                    return self.finish_turn_with_error(
                        TurnEndReason::Cancelled,
                        None,
                        None,
                        on_event,
                    );
                }
            } {
                Ok(disposition) => disposition,
                Err(error) => {
                    if self.cancel.is_cancelled() {
                        return self.finish_turn_with_error(
                            TurnEndReason::Cancelled,
                            None,
                            None,
                            on_event,
                        );
                    }
                    // Context-overflow recovery (TS fullCompaction parity):
                    // the provider rejected the request as too large — run a
                    // compaction round and retry the step. The last-compacted
                    // guard bounds the loop: if the estimate has not grown,
                    // compact() is a no-op and the turn fails below.
                    if error.code.as_deref() == Some("CONTEXT_OVERFLOW")
                        && self.input.max_context_tokens.is_some()
                    {
                        if self.compact(llm, on_event).await {
                            continue;
                        }
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

            match disposition {
                StepDisposition::Complete { finish } => {
                    let step_finish = normalize_finish_reason(finish).to_string();
                    let mut step_complete = || {
                        emit(
                            on_event,
                            EngineEvent::TurnStepCompleted {
                                turn_id,
                                step: step_number as i64,
                                step_id: None,
                                usage: usage.transcript_usage(),
                                finish_reason: Some(step_finish.clone()),
                            },
                        );
                    };
                    if self.has_pending_steer() {
                        // A steer arrived while this step ran (async subagent
                        // finished). The step itself is complete, but the turn
                        // keeps going so the steering message reaches the LLM.
                        step_complete();
                        continue;
                    }
                    // No pending steer: the turn is about to end. Mark it
                    // finished BEFORE the completion events are emitted, so a
                    // steer observed after them is refused (falls back to a
                    // new turn) instead of being dropped into a dead queue.
                    self.mark_finished();
                    step_complete();
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
                StepDisposition::Continue(calls) => {
                    // The assistant message carrying the tool calls must
                    // precede the tool results (providers reject a `tool`
                    // message without a preceding `tool_calls`; the TS loop
                    // pushes it the same way).
                    self.messages.push(LlmMessage {
                        role: "assistant".to_string(),
                        content: serde_json::Value::String(String::new()),
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
                        reasoning: None,
                    });
                    let mut stop_turn = false;
                    for call in &calls {
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
                        };
                        match evaluate(&input) {
                            PolicyDecision::Approve => {
                                let ctx = self.tool_ctx();
                                let result = tokio::select! {
                                    result = execute_tool(turn_id, call.clone(), tools, &ctx, on_event) => result,
                                    _ = self.cancel.cancelled() => {
                                        tools.abort(&call);
                                        emit(
                                            on_event,
                                            EngineEvent::TurnStepInterrupted {
                                                turn_id,
                                                step: step_number as i64,
                                                step_id: None,
                                                reason: "aborted".to_string(),
                                                message: None,
                                            },
                                        );
                                        return self.finish_turn_with_error(
                                            TurnEndReason::Cancelled,
                                            None,
                                            None,
                                            on_event,
                                        );
                                    }
                                };
                                emit_tool_result(&result, turn_id, on_event);
                                self.messages.push(LlmMessage {
                                    role: "tool".to_string(),
                                    content: serde_json::Value::String(result.output.clone()),
                                    name: Some(result.tool_name.clone()),
                                    tool_call_id: Some(result.tool_call_id.clone()),
                                    tool_calls: None,
                                    reasoning: None,
                                });
                                if result.stop_turn {
                                    stop_turn = true;
                                    break;
                                }
                            }
                            PolicyDecision::Deny { reason } => {
                                // The call still happened (the fold's
                                // tool.result needs the tool.call record).
                                emit_tool_call_started(&call, turn_id, on_event);
                                let result = ToolResult {
                                    tool_call_id: call.id.clone(),
                                    tool_name: call.name.clone(),
                                    output: reason,
                                    is_error: true,
                                    stop_turn: false,
                                    updates: vec![],
                                };
                                emit_tool_result(&result, turn_id, on_event);
                                self.messages.push(LlmMessage {
                                    role: "tool".to_string(),
                                    content: serde_json::Value::String(result.output),
                                    name: Some(call.name.clone()),
                                    tool_call_id: Some(call.id.clone()),
                                    tool_calls: None,
                                    reasoning: None,
                                });
                            }
                            PolicyDecision::Ask => {
                                // The call is announced before the pause (TS
                                // onToolCall semantics), so a rejected resume
                                // still folds a proper tool message.
                                emit_tool_call_started(call, turn_id, on_event);
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
                                });
                                return TurnProgress::NeedsApproval(request);
                            }
                        }
                    }
                    if stop_turn {
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
    turn_id: i64,
    call: ToolCall,
    tools: &dyn ToolExecutor,
    ctx: &ToolContext,
    on_event: &mut (dyn FnMut(EngineEvent) + Send),
) -> ToolResult {
    emit_tool_call_started(&call, turn_id, on_event);
    let result = tools.execute(&call, ctx).await;
    for update in &result.updates {
        on_event(EngineEvent::ToolProgress {
            turn_id,
            tool_call_id: call.id.clone(),
            update: update.clone(),
        });
    }
    result
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
    Ok(StepDisposition::Continue(tool_calls))
}

fn emit(on_event: &mut dyn FnMut(EngineEvent), event: EngineEvent) {
    on_event(event);
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

    fn input(messages: Vec<LlmMessage>) -> EngineTurnInput {
        input_with_steps(messages, None)
    }

    fn input_with_steps(messages: Vec<LlmMessage>, max_steps: Option<u32>) -> EngineTurnInput {
        EngineTurnInput {
            turn_id: 1,
            messages,
            tools: vec![],
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
            next_agent_id: None,
            kill_grace_ms: None,
        }
    }

    fn user_message(text: &str) -> LlmMessage {
        LlmMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning: None,
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
        let names = event_names(&events);
        assert!(names.contains(&"turn.step.interrupted".to_string()));
        let interrupted_idx = names
            .iter()
            .position(|name| name == "turn.step.interrupted")
            .unwrap();
        let interrupted = serde_json::to_value(&events[interrupted_idx]).unwrap();
        assert_eq!(interrupted["reason"], "max_steps");
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
            reasoning: None,
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
            next_agent_id: None,
            kill_grace_ms: None,
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
                        reasoning: None,
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
                reasoning: None,
            }],
            tools: vec![],
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
            next_agent_id: None,
            kill_grace_ms: None,
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
            reasoning: None,
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
            next_agent_id: None,
            kill_grace_ms: None,
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
            next_agent_id: None,
            kill_grace_ms: None,
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
}

#[cfg(test)]
mod cancel_tests {
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
            reasoning: None,
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
            next_agent_id: None,
            kill_grace_ms: None,
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
            tools.iter().any(|t| t["function"]["name"] == "Lookup"),
            "request tools must include the registered def: {tools:?}"
        );
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
            next_agent_id: None,
            kill_grace_ms: None,
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
            next_agent_id: None,
            kill_grace_ms: None,
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
            next_agent_id: None,
            kill_grace_ms: None,
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
            next_agent_id: None,
            kill_grace_ms: None,
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
}
