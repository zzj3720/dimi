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
            shell: "/bin/sh".to_string(),
        }
    }
}

/// How a step ended and what the outer loop should do next.
#[derive(Debug, Clone, PartialEq)]
enum StepDisposition {
    /// No tool calls (or a stop_turn result) — the turn is complete.
    Complete,
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

/// One in-flight turn: messages, step counter and the pending approval (if
/// the policy asked and the turn paused).
pub struct TurnSession {
    input: EngineTurnInput,
    messages: Vec<LlmMessage>,
    steps: u32,
    started_at: Instant,
    pending: Option<PendingApproval>,
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
        Self {
            input,
            messages: Vec::new(),
            steps: 0,
            started_at: Instant::now(),
            pending: None,
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
                execute_tool(pending.call.clone(), tools, &self.tool_ctx(), on_event).await
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
                .unwrap_or_else(|| "/bin/sh".to_string()),
        }
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
            let request = ChatRequest {
                messages: self.messages.clone(),
                tools: None,
                model: Some(self.input.provider.model.clone()),
                thinking_effort: self.input.provider.thinking_effort.clone(),
            };

            let disposition = match execute_step(turn_id, llm, &request, &mut usage, on_event).await
            {
                Ok(disposition) => disposition,
                Err(error) => {
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
                StepDisposition::Complete => {
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
                StepDisposition::Continue(calls) => {
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
                                let result =
                                    execute_tool(call.clone(), tools, &self.tool_ctx(), on_event)
                                        .await;
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

    fn finish_turn_with_error(
        &mut self,
        status: TurnEndReason,
        error: Option<String>,
        error_code: Option<String>,
        on_event: &mut (dyn FnMut(EngineEvent) + Send),
    ) -> TurnProgress {
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
                error: None,
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
    on_event: &mut (dyn FnMut(EngineEvent) + Send),
) -> ToolResult {
    on_event(EngineEvent::ToolCallStarted {
        turn_id: 0,
        tool_call_id: call.id.clone(),
        name: call.name.clone(),
        args: Some(call.arguments.clone()),
        description: None,
    });
    let result = tools.execute(&call, ctx);
    for update in &result.updates {
        on_event(EngineEvent::ToolProgress {
            turn_id: 0,
            tool_call_id: call.id.clone(),
            update: update.clone(),
        });
    }
    result
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
            LlmStreamEvent::Finish { .. } | LlmStreamEvent::Error { .. } => {}
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

    if tool_calls.is_empty() {
        return Ok(StepDisposition::Complete);
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
    use crate::llm::ScriptedLlmClient;
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
        let outcome = engine
            .run_turn(
                &input(vec![user_message("hi")]),
                &llm,
                &crate::tool::BashTool,
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
        let outcome = engine
            .run_turn(
                &input(vec![user_message("run a command")]),
                &llm,
                &crate::tool::BashTool,
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
        let outcome = engine
            .run_turn(
                &input_with_steps(vec![user_message("loop")], Some(1)),
                &llm,
                &crate::tool::BashTool,
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
        let outcome = engine
            .run_turn(
                &input(vec![user_message("fail")]),
                &llm,
                &crate::tool::BashTool,
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
