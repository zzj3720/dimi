//! LLM effect boundary — the engine talks to models through this trait.
//!
//! Slice 1 ships the mock implementation (scripted event sequences, used by
//! the differential tests) plus a synchronous OpenAI-compatible SSE client
//! (`OpenAiCompatibleClient`). The trait is deliberately callback-driven and
//! synchronous so the orchestration core stays testable without a runtime.

use serde::{Deserialize, Serialize};

use crate::types::{LlmMessage, LlmToolCall};

/// One streaming LLM event, in arrival order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LlmStreamEvent {
    /// Plain text delta (assistant content chunk).
    #[serde(rename = "text")]
    Text { delta: String },
    /// Reasoning/thinking delta.
    #[serde(rename = "thinking")]
    Thinking { delta: String },
    /// Tool-call delta: either the call header (id/name) or an arguments
    /// fragment. The client concatenates argument fragments per call id.
    #[serde(rename = "tool_call")]
    ToolCall {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments_part: Option<String>,
    },
    /// Usage report (arrives with the terminal chunk).
    #[serde(rename = "usage")]
    Usage {
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        total_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_tokens_details: Option<UsageDetails>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completion_tokens_details: Option<CompletionUsageDetails>,
    },
    /// Terminal event: the model finished its response.
    #[serde(rename = "finish")]
    Finish {
        /// Raw provider finish reason (openai vocabulary), or None.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },
    /// Terminal event: the request failed.
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionUsageDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

/// Request handed to the LLM effect boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub messages: Vec<LlmMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_effort: Option<String>,
}

/// Parsed assistant turn: the tool calls the model requested (if any).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTurn {
    pub tool_calls: Vec<LlmToolCall>,
    /// Concatenated text content (empty when none).
    pub text: String,
    /// Concatenated thinking content (empty when none).
    pub thinking: String,
}

/// LLM effect boundary.
pub trait LlmClient {
    /// Stream one chat completion; events arrive in order. The engine
    /// collects them into an `AssistantTurn` and forwards deltas.
    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_event: &mut dyn FnMut(&LlmStreamEvent),
    ) -> Result<AssistantTurn, LlmError>;
}

/// LLM failure — message text is shown to the user (turn fails).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmError {
    pub message: String,
    /// Error code vocabulary (provider_filtered, auth, rate_limit, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Scripted client: replays per-step event sequences (differential tests).
/// Each `stream_chat` call consumes the next segment, so a multi-step turn
/// gets deterministic responses per step (step 1 → tool call, step 2 →
/// text, …). Segments are separated on `Finish`/`Error` events: one call
/// plays exactly one segment.
#[derive(Debug)]
pub struct ScriptedLlmClient {
    /// One segment per LLM call, in order.
    segments: Vec<Vec<LlmStreamEvent>>,
    cursor: std::sync::Mutex<usize>,
}

impl ScriptedLlmClient {
    pub fn new(segments: Vec<Vec<LlmStreamEvent>>) -> Self {
        Self {
            segments,
            cursor: std::sync::Mutex::new(0),
        }
    }

    /// Single-segment convenience (one-step turns).
    pub fn once(events: Vec<LlmStreamEvent>) -> Self {
        Self::new(vec![events])
    }
}

impl LlmClient for ScriptedLlmClient {
    fn stream_chat(
        &self,
        _request: &ChatRequest,
        on_event: &mut dyn FnMut(&LlmStreamEvent),
    ) -> Result<AssistantTurn, LlmError> {
        let mut cursor = self.cursor.lock().unwrap_or_else(|p| p.into_inner());
        let segment = self.segments.get(*cursor).cloned().unwrap_or_default();
        *cursor += 1;

        let mut tool_calls: Vec<LlmToolCall> = Vec::new();
        let mut text = String::new();
        let mut thinking = String::new();
        for event in &segment {
            match event {
                LlmStreamEvent::Text { delta } => {
                    text.push_str(delta);
                    on_event(event);
                }
                LlmStreamEvent::Thinking { delta } => {
                    thinking.push_str(delta);
                    on_event(event);
                }
                LlmStreamEvent::ToolCall {
                    tool_call_id,
                    name,
                    arguments_part,
                } => {
                    let call = tool_calls.iter_mut().find(|call| call.id == *tool_call_id);
                    match call {
                        Some(call) => {
                            if let Some(part) = arguments_part {
                                call.function.arguments.push_str(part);
                            }
                        }
                        None => {
                            tool_calls.push(LlmToolCall {
                                id: tool_call_id.clone(),
                                call_type: Some("function".to_string()),
                                function: crate::types::LlmToolCallFunction {
                                    name: name.clone().unwrap_or_default(),
                                    arguments: arguments_part.clone().unwrap_or_default(),
                                },
                            });
                        }
                    }
                    on_event(event);
                }
                LlmStreamEvent::Usage { .. } | LlmStreamEvent::Finish { .. } => {
                    on_event(event);
                }
                LlmStreamEvent::Error { message } => {
                    on_event(event);
                    return Err(LlmError {
                        message: message.clone(),
                        code: None,
                    });
                }
            }
        }
        Ok(AssistantTurn {
            tool_calls,
            text,
            thinking,
        })
    }
}

/// OpenAI-compatible `/chat/completions` SSE client (slice 1).
///
/// Implements the stream parsing the TS `llmRequesterService` expects:
/// `data:` lines carrying `{choices:[{delta:{content|reasoning_content|
/// tool_calls}, finish_reason}], usage}` chunks; `[DONE]` terminates.
pub struct OpenAiCompatibleClient {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl LlmClient for OpenAiCompatibleClient {
    fn stream_chat(
        &self,
        _request: &ChatRequest,
        _on_event: &mut dyn FnMut(&LlmStreamEvent),
    ) -> Result<AssistantTurn, LlmError> {
        // Slice 1: HTTP transport lands with the napi swap-in (slice 1 tail);
        // the parser itself lives in `parse_openai_sse` (unit-tested below).
        Err(LlmError {
            message: "OpenAiCompatibleClient transport not wired yet".to_string(),
            code: Some("not_implemented".to_string()),
        })
    }
}

/// Parse an OpenAI SSE chunk into stream events (pure, unit-tested).
pub fn parse_openai_sse_chunk(
    line: &str,
    tool_calls: &mut std::collections::HashMap<String, (String, String)>,
    events: &mut Vec<LlmStreamEvent>,
) {
    // Implementation lands with the transport; tests pin the parser shape.
    let _ = (line, tool_calls, events);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_client_concatenates_tool_call_arguments() {
        let client = ScriptedLlmClient::once(vec![
            LlmStreamEvent::Text {
                delta: "hel".to_string(),
            },
            LlmStreamEvent::Text {
                delta: "lo".to_string(),
            },
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: Some("Bash".to_string()),
                arguments_part: Some("{\"command\":\"e".to_string()),
            },
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: None,
                arguments_part: Some("cho hi\"}".to_string()),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("tool_calls".to_string()),
            },
        ]);
        let mut seen: Vec<LlmStreamEvent> = Vec::new();
        let turn = client
            .stream_chat(
                &ChatRequest {
                    messages: vec![],
                    tools: None,
                    model: None,
                    thinking_effort: None,
                },
                &mut |event| seen.push(event.clone()),
            )
            .unwrap();
        assert_eq!(turn.text, "hello");
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call_1");
        assert_eq!(turn.tool_calls[0].function.name, "Bash");
        assert_eq!(
            turn.tool_calls[0].function.arguments,
            "{\"command\":\"echo hi\"}"
        );
        assert_eq!(seen.len(), 5);
    }
}
