//! Turn orchestration core — the M3 swap-in socket for the TS loop domain.
//!
//! `Engine::run_turn` reproduces the TS `loopService.run` minimal closed
//! loop: step loop (LLM request → stream → tool execution → next step),
//! max-steps control, usage accumulation and the exact engine event stream
//! the transcript projection layer consumes. Context assembly stays on the
//! TS side (slice 1): the engine receives the assembled messages and appends
//! tool results as the turn progresses.
//!
//! Effect boundaries are injected (no DI container — plain trait objects):
//! `LlmClient` for models and `ToolExecutor` for tools. Slice 1 ships the
//! scripted LLM (differential tests) and the Bash tool over dimi-exec.

use std::time::Instant;

use dimi_wire::model::{TranscriptUsage, TurnOrigin};

use crate::events::{EngineEvent, FinishReason};
use crate::llm::{ChatRequest, LlmClient, LlmStreamEvent};
use crate::tool::{ToolCall, ToolContext, ToolExecutor};
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
    /// Tool calls were executed; loop back for another step.
    Continue,
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
    /// Run one turn. `llm` and `tools` are the injected effect boundaries;
    /// `on_event` receives the engine event stream in order. The returned
    /// outcome carries the terminal state.
    pub fn run_turn(
        &self,
        input: &EngineTurnInput,
        llm: &dyn LlmClient,
        tools: &dyn ToolExecutor,
        on_event: &mut dyn FnMut(EngineEvent),
    ) -> TurnOutcome {
        let started_at = Instant::now();
        let turn_id = input.turn_id;
        let origin = TurnOrigin::User { payload: None };
        let prompt = last_user_text(&input.messages);

        emit(
            on_event,
            EngineEvent::TurnStarted {
                turn_id,
                origin: origin.clone(),
                prompt,
            },
        );

        let mut messages = input.messages.clone();
        let mut steps: u32 = 0;

        let mut tool_ctx = ToolContext {
            cwd: input.cwd.clone().unwrap_or_else(|| ".".to_string()),
            shell: input.shell.clone().unwrap_or_else(|| self.shell.clone()),
        };
        if tool_ctx.shell.is_empty() {
            tool_ctx.shell = "/bin/sh".to_string();
        }

        let outcome = loop {
            // max-steps guard (loopService.beginLoopStep).
            if let Some(max) = self.max_steps_per_turn {
                if max > 0 && steps >= max {
                    emit(
                        on_event,
                        EngineEvent::TurnStepInterrupted {
                            turn_id,
                            step: steps as i64,
                            step_id: None,
                            reason: "max_steps".to_string(),
                            message: Some(format!(
                                "Turn exceeded maxSteps={max}. If max_steps_per_turn is too small, raise it in config.toml (loop_control.max_steps_per_turn)"
                            )),
                        },
                    );
                    break TurnOutcome {
                        status: TurnEndReason::Failed,
                        steps,
                        error: Some(format!("Turn exceeded maxSteps={max}")),
                        error_code: Some("LOOP_MAX_STEPS_EXCEEDED".to_string()),
                        truncated: None,
                    };
                }
            }

            steps += 1;
            let step_number = steps;
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
                messages: messages.clone(),
                tools: None,
                model: Some(input.provider.model.clone()),
                thinking_effort: input.provider.thinking_effort.clone(),
            };

            let (disposition, tool_messages) = match execute_step(
                turn_id, &tool_ctx, llm, tools, &request, &mut usage, on_event,
            ) {
                Ok(parts) => parts,
                Err(error) => {
                    let message = error.message.clone();
                    let code = error.code.clone().unwrap_or_default();
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
                    break TurnOutcome {
                        status: TurnEndReason::Failed,
                        steps: step_number,
                        error: Some(message),
                        error_code: Some(code),
                        truncated: None,
                    };
                }
            };
            messages.extend(tool_messages);

            let step_finish = match &disposition {
                StepDisposition::Complete => FinishReason::Completed,
                StepDisposition::Continue => FinishReason::ToolCalls,
            };
            emit(
                on_event,
                EngineEvent::TurnStepCompleted {
                    turn_id,
                    step: step_number as i64,
                    step_id: None,
                    usage: usage.transcript_usage(),
                    finish_reason: Some(normalize_finish_reason(step_finish).to_string()),
                },
            );

            match disposition {
                StepDisposition::Complete => {
                    break TurnOutcome {
                        status: TurnEndReason::Completed,
                        steps: step_number,
                        error: None,
                        error_code: None,
                        truncated: Some(step_finish == FinishReason::Truncated),
                    };
                }
                StepDisposition::Continue => {
                    // Tool results were appended to messages; loop back.
                }
            }
        };

        emit(
            on_event,
            EngineEvent::TurnEnded {
                turn_id,
                reason: match outcome.status {
                    TurnEndReason::Completed => "completed".to_string(),
                    TurnEndReason::Cancelled => "cancelled".to_string(),
                    TurnEndReason::Failed => "failed".to_string(),
                    TurnEndReason::Blocked => "blocked".to_string(),
                },
                error: outcome
                    .error
                    .as_ref()
                    .map(|message| serde_json::json!({ "message": message })),
                duration_ms: Some(started_at.elapsed().as_millis() as i64),
            },
        );

        outcome
    }
}

/// Execute one step: LLM stream → parse → tool calls → tool results.
/// Returns the disposition for the outer loop plus the tool messages to
/// append to the conversation.
fn execute_step(
    turn_id: i64,
    tool_ctx: &ToolContext,
    llm: &dyn LlmClient,
    tools: &dyn ToolExecutor,
    request: &ChatRequest,
    usage: &mut UsageAccumulator,
    on_event: &mut dyn FnMut(EngineEvent),
) -> Result<(StepDisposition, Vec<LlmMessage>), crate::llm::LlmError> {
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    let assistant = llm.stream_chat(request, &mut |event| match event {
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
        LlmStreamEvent::Finish { .. } => {}
        LlmStreamEvent::Error { .. } => {}
    })?;

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
        return Ok((StepDisposition::Complete, Vec::new()));
    }

    let mut tool_messages = Vec::new();
    for call in &tool_calls {
        on_event(EngineEvent::ToolCallStarted {
            turn_id,
            tool_call_id: call.id.clone(),
            name: call.name.clone(),
            args: Some(call.arguments.clone()),
            description: None,
        });
        let result = tools.execute(call, tool_ctx);
        for update in &result.updates {
            on_event(EngineEvent::ToolProgress {
                turn_id,
                tool_call_id: call.id.clone(),
                update: update.clone(),
            });
        }
        on_event(EngineEvent::ToolResult {
            turn_id,
            tool_call_id: call.id.clone(),
            output: result.output.clone(),
            is_error: Some(result.is_error),
            synthetic: None,
        });
        // Append the tool message to the conversation
        // (loopEventFold.createToolMessage).
        tool_messages.push(LlmMessage {
            role: "tool".to_string(),
            content: serde_json::Value::String(result.output.clone()),
            name: Some(call.name.clone()),
            tool_call_id: Some(call.id.clone()),
            tool_calls: None,
            reasoning: None,
        });
        if result.stop_turn {
            return Ok((StepDisposition::Complete, tool_messages));
        }
    }

    Ok((StepDisposition::Continue, tool_messages))
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

/// Normalize a finish reason to the transcript vocabulary
/// (loopService.normalizeFinishReason).
pub fn normalize_finish_reason(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Completed => "completed",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::Other => "other",
        FinishReason::Truncated => "truncated",
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
    use crate::tool::BashTool;
    use crate::types::ProviderConfig;

    fn input(messages: Vec<LlmMessage>) -> EngineTurnInput {
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
            max_steps_per_turn: None,
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

    #[test]
    fn single_step_completes_with_text() {
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
        let outcome = engine.run_turn(
            &input(vec![user_message("hi")]),
            &llm,
            &BashTool,
            &mut |event| events.push(event),
        );

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
        assert_eq!(step_completed["finishReason"], "completed");
        // turn.ended reason completed.
        let ended = serde_json::to_value(&events[5]).unwrap();
        assert_eq!(ended["reason"], "completed");
    }

    #[test]
    fn tool_call_runs_bash_and_loops() {
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
        let outcome = engine.run_turn(
            &input(vec![user_message("run a command")]),
            &llm,
            &BashTool,
            &mut |event| events.push(event),
        );

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
        assert_eq!(step_completed["finishReason"], "tool_calls");
    }

    #[test]
    fn max_steps_fails_the_turn() {
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
        let outcome = engine.run_turn(
            &input(vec![user_message("loop")]),
            &llm,
            &BashTool,
            &mut |event| events.push(event),
        );

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

    #[test]
    fn tool_error_is_error_result() {
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
        let outcome = engine.run_turn(
            &input(vec![user_message("fail")]),
            &llm,
            &BashTool,
            &mut |event| events.push(event),
        );

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
