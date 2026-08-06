//! LLM effect boundary — the engine talks to models through this trait.
//!
//! Slice 1 ships the mock implementation (scripted event sequences, used by
//! the differential tests) plus a synchronous OpenAI-compatible SSE client
//! (`OpenAiCompatibleClient`). The trait is deliberately callback-driven and
//! synchronous so the orchestration core stays testable without a runtime.

use serde::{Deserialize, Serialize};
use std::io::BufRead;

use crate::types::{LlmMessage, LlmToolCall, LlmToolCallFunction};

/// One streaming LLM event, in arrival order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LlmStreamEvent {
    /// Plain text delta (assistant content chunk).
    #[serde(rename = "text", rename_all = "camelCase")]
    Text { delta: String },
    /// Reasoning/thinking delta.
    #[serde(rename = "thinking", rename_all = "camelCase")]
    Thinking { delta: String },
    /// Tool-call delta: either the call header (id/name) or an arguments
    /// fragment. The client concatenates argument fragments per call id.
    #[serde(rename = "tool_call", rename_all = "camelCase")]
    ToolCall {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments_part: Option<String>,
    },
    /// Usage report (arrives with the terminal chunk).
    #[serde(rename = "usage", rename_all = "camelCase")]
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
    #[serde(rename = "finish", rename_all = "camelCase")]
    Finish {
        /// Raw provider finish reason (openai vocabulary), or None.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },
    /// Terminal event: the request failed.
    #[serde(rename = "error", rename_all = "camelCase")]
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
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

/// A streamed chat completion: the raw event sequence plus the parsed turn.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamedTurn {
    /// Every stream event, in arrival order (deltas, usage, finish, …).
    pub events: Vec<LlmStreamEvent>,
    /// Parsed assistant turn (concatenated text/thinking + tool calls).
    pub assistant: AssistantTurn,
}

/// LLM effect boundary.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Stream one chat completion and return the full event sequence plus
    /// the parsed assistant turn. The engine forwards the events and drives
    /// the loop from the parsed turn.
    async fn stream_chat(&self, request: &ChatRequest) -> Result<StreamedTurn, LlmError>;
}

/// LLM failure — message text is shown to the user (turn fails).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmError {
    pub message: String,
    /// Error code vocabulary (provider_filtered, auth, rate_limit, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Retry-after hint (ms) from the provider (e.g. 429 rate-limit
    /// headers). `None` = fall back to exponential backoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// Whether this failure is transient (connection / rate limit / 5xx /
    /// timeout) and the step may be retried. Mirrors the TS
    /// `isRetryableGenerateError` verdict; the engine retries retryable
    /// step failures up to `max_retries_per_step`.
    #[serde(default)]
    pub retryable: bool,
}

impl Default for LlmError {
    fn default() -> Self {
        Self {
            message: String::new(),
            code: None,
            retry_after_ms: None,
            retryable: false,
        }
    }
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

#[async_trait::async_trait]
impl LlmClient for ScriptedLlmClient {
    async fn stream_chat(&self, _request: &ChatRequest) -> Result<StreamedTurn, LlmError> {
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
                }
                LlmStreamEvent::Thinking { delta } => {
                    thinking.push_str(delta);
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
                }
                LlmStreamEvent::Usage { .. } | LlmStreamEvent::Finish { .. } => {}
                LlmStreamEvent::Error { message } => {
                    return Err(LlmError {
                        message: message.clone(),
                        code: None,
                        ..Default::default()
                    });
                }
            }
        }
        Ok(StreamedTurn {
            events: segment,
            assistant: AssistantTurn {
                tool_calls,
                text,
                thinking,
            },
        })
    }
}

/// OpenAI-compatible `/chat/completions` SSE client.
///
/// Implements the stream parsing the TS `llmRequesterService` expects:
/// `data:` lines carrying `{choices:[{delta:{content|reasoning_content|
/// tool_calls}, finish_reason}], usage}` chunks; `[DONE]` terminates.
///
/// Transport: a synchronous `ureq` POST to `{base_url}/chat/completions`
/// (`Authorization: Bearer {api_key}`, `Content-Type: application/json`,
/// `Accept: text/event-stream`) run inside `tokio::task::spawn_blocking`, so
/// the async `LlmClient` boundary stays non-blocking.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleClient {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl OpenAiCompatibleClient {
    /// One blocking chat-completions round trip: POST → read SSE lines →
    /// decode chunks with [`parse_openai_sse_chunk`] → assemble the turn.
    fn stream_chat_blocking(&self, request: &ChatRequest) -> Result<StreamedTurn, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let mut body = serde_json::json!({
            "model": model,
            "stream": true,
            "stream_options": { "include_usage": true },
            "messages": request.messages,
        });
        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                body["tools"] =
                    serde_json::Value::Array(tools.iter().map(openai_tool_def).collect());
            }
        }
        if let Some(effort) = &request.thinking_effort {
            body["reasoning_effort"] = serde_json::Value::String(effort.clone());
        }

        let response = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream")
            .timeout(std::time::Duration::from_secs(300))
            .send_string(&body.to_string())
            .map_err(transport_llm_error)?;

        // Read the SSE body line-by-line, feeding each `data:` payload to the
        // chunk decoder. `[DONE]` (or EOF) terminates the stream.
        let reader = std::io::BufReader::new(response.into_reader());
        let mut reader = reader;
        let mut line = String::new();
        let mut tool_calls: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        let mut events: Vec<LlmStreamEvent> = Vec::new();
        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|e| connection_error(format!("failed reading SSE stream: {e}")))?;
            if read == 0 {
                break;
            }
            let trimmed = line.trim();
            let Some(payload) = trimmed.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                break;
            }
            if payload.is_empty() {
                continue;
            }
            parse_openai_sse_chunk(payload, &mut tool_calls, &mut events);
        }

        // Assemble the parsed assistant turn from the decoded events and the
        // accumulated tool calls (internal marker keys are skipped).
        let mut text = String::new();
        let mut thinking = String::new();
        for event in &events {
            match event {
                LlmStreamEvent::Text { delta } => text.push_str(delta),
                LlmStreamEvent::Thinking { delta } => thinking.push_str(delta),
                _ => {}
            }
        }
        let mut keys: Vec<&String> = tool_calls.keys().collect();
        keys.sort();
        let mut calls: Vec<LlmToolCall> = Vec::new();
        for key in keys {
            if key.starts_with(TOOL_INDEX_MARKER_PREFIX) {
                continue;
            }
            let (name, arguments) = &tool_calls[key];
            if name.is_empty() && arguments.is_empty() {
                continue;
            }
            calls.push(LlmToolCall {
                id: key.clone(),
                call_type: Some("function".to_string()),
                function: LlmToolCallFunction {
                    name: name.clone(),
                    arguments: arguments.clone(),
                },
            });
        }

        Ok(StreamedTurn {
            events,
            assistant: AssistantTurn {
                tool_calls: calls,
                text,
                thinking,
            },
        })
    }
}

/// Map a `ureq` failure to the engine's provider error vocabulary (TS
/// `isRetryableGenerateError` parity): HTTP 4xx/5xx carry their status code
/// (auth / rate-limit / timeout / overflow are classified), transport-level
/// failures become retryable connection errors.
fn transport_llm_error(err: ureq::Error) -> LlmError {
    match err {
        ureq::Error::Status(code, response) => {
            let status = code as usize;
            let retry_after_ms = response
                .header("retry-after")
                .and_then(|v| v.parse::<u64>().ok())
                .map(|secs| secs.saturating_mul(1000));
            let text = response.into_string().unwrap_or_default();
            LlmError {
                message: format!("provider returned HTTP {status}: {}", truncate(&text, 500)),
                code: Some(match status {
                    401 | 403 => "provider.auth_error".to_string(),
                    429 => "provider.rate_limit".to_string(),
                    408 => "provider.timeout".to_string(),
                    413 => "CONTEXT_OVERFLOW".to_string(),
                    _ => "provider.api_error".to_string(),
                }),
                retry_after_ms,
                retryable: status == 408 || status == 429 || status >= 500,
            }
        }
        ureq::Error::Transport(err) => connection_error(format!("LLM transport error: {err}")),
    }
}

/// A transient connection-level failure (retryable by the engine).
fn connection_error(message: String) -> LlmError {
    LlmError {
        message,
        code: Some("provider.connection_error".to_string()),
        retry_after_ms: None,
        retryable: true,
    }
}

/// Truncate an error body for display (bounds the message length).
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

/// Normalize one engine tool def into the OpenAI `/chat/completions` `tools`
/// entry shape (`{type:"function", function:{name, description, parameters}}`).
/// The engine's `ChatRequest.tools` carries the flat aimux shape
/// (`{type, name, description, input_schema}` — see `engine.rs`
/// `aimux_tools_json`), which OpenAI-compatible providers reject; already
/// OpenAI-nested defs pass through unchanged.
fn openai_tool_def(tool: &serde_json::Value) -> serde_json::Value {
    if tool.get("function").is_some() {
        return tool.clone();
    }
    let mut function = serde_json::Map::new();
    function.insert(
        "name".to_string(),
        tool.get("name").cloned().unwrap_or(serde_json::Value::Null),
    );
    function.insert(
        "description".to_string(),
        tool.get("description")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    );
    if let Some(parameters) = tool.get("input_schema") {
        function.insert("parameters".to_string(), parameters.clone());
    }
    serde_json::json!({ "type": "function", "function": function })
}

#[async_trait::async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn stream_chat(&self, request: &ChatRequest) -> Result<StreamedTurn, LlmError> {
        let client = self.clone();
        let request = request.clone();
        tokio::task::spawn_blocking(move || client.stream_chat_blocking(&request))
            .await
            .map_err(|join| connection_error(format!("LLM transport task panicked: {join}")))?
    }
}

/// Reserved key prefix for the parser's internal `index → id` bookkeeping
/// entries inside the `tool_calls` accumulator (see `parse_openai_sse_chunk`).
const TOOL_INDEX_MARKER_PREFIX: &str = "__dimi_tool_index_";

/// Parse one OpenAI SSE `data:` payload (a JSON object string) into stream
/// events (pure, unit-tested).
///
/// Mirrors the TS `streamOpenAIChat` chunk decode (`packages/agent-core-v2/
/// src/app/providerRuntime/stream.ts`): `choices[0].delta.content` → text,
/// `reasoning_content`/`reasoning`/`reasoning_text` → thinking,
/// `delta.tool_calls[]` → tool-call fragments (accumulated across chunks),
/// `choices[0].finish_reason` → finish, `usage` → usage.
///
/// `tool_calls` accumulates in-progress tool calls across chunks. OpenAI sends
/// the full call id on the **first** delta of a call and omits it (and the
/// name) on later fragments, so the parser tracks `index → id` via internal
/// marker entries (keys prefixed with [`TOOL_INDEX_MARKER_PREFIX`] mapping to
/// the id); the transport skips those when building the parsed turn. Each
/// `ToolCall` event carries the call's final id and the fragment's name
/// (first fragment only) / arguments slice (empty slices are `None`).
pub fn parse_openai_sse_chunk(
    line: &str,
    tool_calls: &mut std::collections::HashMap<String, (String, String)>,
    events: &mut Vec<LlmStreamEvent>,
) {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };

    if let Some(choice) = data
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    {
        if let Some(delta) = choice.get("delta").and_then(|v| v.as_object()) {
            // Text content delta.
            if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    events.push(LlmStreamEvent::Text {
                        delta: text.to_owned(),
                    });
                }
            }
            // Thinking delta — first recognized reasoning field wins (TS
            // `thinkingField.find(...)` order).
            for field in ["reasoning_content", "reasoning", "reasoning_text"] {
                if let Some(thinking) = delta.get(field).and_then(|v| v.as_str()) {
                    if !thinking.is_empty() {
                        events.push(LlmStreamEvent::Thinking {
                            delta: thinking.to_owned(),
                        });
                    }
                    break;
                }
            }
            // Tool-call deltas: accumulate fragments by call (see the fn doc).
            if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for item in calls {
                    let Some(obj) = item.as_object() else {
                        continue;
                    };
                    let index = obj
                        .get("index")
                        .and_then(|v| v.as_u64())
                        .unwrap_or_default() as usize;
                    let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let function = obj.get("function").and_then(|v| v.as_object());
                    let name = function
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let arguments = function
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    let marker = format!("{TOOL_INDEX_MARKER_PREFIX}{index}");
                    let key = match tool_calls.get(&marker) {
                        Some((stored_id, _)) => stored_id.clone(),
                        None => {
                            let key = if id.is_empty() {
                                format!("call_{index}")
                            } else {
                                id.to_string()
                            };
                            tool_calls.insert(marker, (key.clone(), String::new()));
                            key
                        }
                    };
                    let entry = tool_calls
                        .entry(key.clone())
                        .or_insert_with(|| (String::new(), String::new()));
                    if !name.is_empty() {
                        entry.0.push_str(name);
                    }
                    if !arguments.is_empty() {
                        entry.1.push_str(arguments);
                    }
                    events.push(LlmStreamEvent::ToolCall {
                        tool_call_id: key,
                        name: (!name.is_empty()).then(|| name.to_owned()),
                        arguments_part: (!arguments.is_empty()).then(|| arguments.to_owned()),
                    });
                }
            }
        }
        if let Some(finish) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            if !finish.is_empty() {
                events.push(LlmStreamEvent::Finish {
                    finish_reason: Some(finish.to_owned()),
                });
            }
        }
    }

    // Usage (sent in the final chunk when `stream_options.include_usage`).
    if let Some(usage) = data.get("usage") {
        events.push(LlmStreamEvent::Usage {
            prompt_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()),
            completion_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()),
            total_tokens: usage.get("total_tokens").and_then(|v| v.as_u64()),
            prompt_tokens_details: usage.get("prompt_tokens_details").map(|d| UsageDetails {
                cached_tokens: d.get("cached_tokens").and_then(|v| v.as_u64()),
                cache_write_tokens: d
                    .get("cache_write_tokens")
                    .or_else(|| d.get("cache_creation_input_tokens"))
                    .or_else(|| d.get("cacheWriteTokens"))
                    .and_then(|v| v.as_u64()),
                reasoning_tokens: d.get("reasoning_tokens").and_then(|v| v.as_u64()),
            }),
            completion_tokens_details: usage.get("completion_tokens_details").map(|d| {
                CompletionUsageDetails {
                    reasoning_tokens: d.get("reasoning_tokens").and_then(|v| v.as_u64()),
                }
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_chunk(
        line: &str,
    ) -> (
        Vec<LlmStreamEvent>,
        std::collections::HashMap<String, (String, String)>,
    ) {
        let mut calls = std::collections::HashMap::new();
        let mut events = Vec::new();
        parse_openai_sse_chunk(line, &mut calls, &mut events);
        (events, calls)
    }

    #[test]
    fn parse_sse_text_delta_emits_text_event() {
        let (events, _) = parse_chunk(
            r#"{"choices":[{"delta":{"role":"assistant","content":"hel"},"index":0}]}"#,
        );
        assert_eq!(
            events,
            vec![LlmStreamEvent::Text {
                delta: "hel".to_string()
            }]
        );
    }

    #[test]
    fn parse_sse_reasoning_delta_emits_thinking_event() {
        for field in ["reasoning_content", "reasoning", "reasoning_text"] {
            let (events, _) = parse_chunk(&format!(
                r#"{{"choices":[{{"delta":{{"{field}":"think hard"}},"index":0}}]}}"#
            ));
            assert_eq!(
                events,
                vec![LlmStreamEvent::Thinking {
                    delta: "think hard".to_string()
                }],
                "field: {field}"
            );
        }
    }

    #[test]
    fn parse_sse_finish_reason_emits_finish_event() {
        let (events, _) =
            parse_chunk(r#"{"choices":[{"delta":{},"index":0,"finish_reason":"stop"}]}"#);
        assert_eq!(
            events,
            vec![LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string())
            }]
        );
    }

    #[test]
    fn parse_sse_usage_emits_usage_event() {
        let (events, _) = parse_chunk(
            r#"{"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#,
        );
        assert_eq!(
            events,
            vec![LlmStreamEvent::Usage {
                prompt_tokens: Some(5),
                completion_tokens: Some(3),
                total_tokens: Some(8),
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }]
        );
    }

    #[test]
    fn parse_sse_usage_details_are_carried() {
        let (events, _) = parse_chunk(
            r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14,"prompt_tokens_details":{"cached_tokens":2,"cache_write_tokens":3},"completion_tokens_details":{"reasoning_tokens":1}}}"#,
        );
        let LlmStreamEvent::Usage {
            prompt_tokens_details,
            completion_tokens_details,
            ..
        } = &events[0]
        else {
            panic!("expected usage event");
        };
        assert_eq!(
            prompt_tokens_details.as_ref().and_then(|d| d.cached_tokens),
            Some(2)
        );
        assert_eq!(
            prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cache_write_tokens),
            Some(3)
        );
        assert_eq!(
            completion_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens),
            Some(1)
        );
    }

    #[test]
    fn parse_sse_tool_call_accumulates_fragments_across_chunks() {
        let mut calls = std::collections::HashMap::new();
        let mut events = Vec::new();
        parse_openai_sse_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"Bash","arguments":""}}]},"index":0}]}"#,
            &mut calls,
            &mut events,
        );
        parse_openai_sse_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"command\":\"ech"}}]},"index":0}]}"#,
            &mut calls,
            &mut events,
        );
        parse_openai_sse_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"o hi\"}"}}]},"index":0,"finish_reason":"tool_calls"}]}"#,
            &mut calls,
            &mut events,
        );

        // The call fragments rejoin under the single real id.
        let (name, arguments) = calls.get("call_1").expect("call_1 accumulated");
        assert_eq!(name, "Bash");
        assert_eq!(arguments, "{\"command\":\"echo hi\"}");

        // The event stream mirrors the deltas: name on the first fragment,
        // arguments fragments afterwards, finish last.
        assert_eq!(
            events[0],
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: Some("Bash".to_string()),
                arguments_part: None,
            }
        );
        assert_eq!(
            events[1],
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: None,
                arguments_part: Some("{\"command\":\"ech".to_string()),
            }
        );
        assert_eq!(
            events[2],
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: None,
                arguments_part: Some("o hi\"}".to_string()),
            }
        );
        assert_eq!(
            events[3],
            LlmStreamEvent::Finish {
                finish_reason: Some("tool_calls".to_string()),
            }
        );
    }

    #[test]
    fn parse_sse_two_parallel_tool_calls_accumulate_separately() {
        let (events, calls) = parse_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"Bash","arguments":"{\"command\":\"a\"}"}},{"index":1,"id":"call_b","type":"function","function":{"name":"Bash","arguments":"{\"command\":\"b\"}"}}]},"index":0}]}"#,
        );
        assert_eq!(calls["call_a"].1, "{\"command\":\"a\"}");
        assert_eq!(calls["call_b"].1, "{\"command\":\"b\"}");
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn parse_sse_non_json_or_empty_payload_is_ignored() {
        let (events, _) = parse_chunk("not json");
        assert!(events.is_empty());
        let (events, _) = parse_chunk("");
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn scripted_client_concatenates_tool_call_arguments() {
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
        let seen: Vec<LlmStreamEvent> = Vec::new();
        let turn = client
            .stream_chat(&ChatRequest {
                messages: vec![],
                tools: None,
                model: None,
                thinking_effort: None,
            })
            .await
            .unwrap();
        assert_eq!(turn.assistant.text, "hello");
        assert_eq!(turn.assistant.tool_calls.len(), 1);
        assert_eq!(turn.assistant.tool_calls[0].id, "call_1");
        assert_eq!(turn.assistant.tool_calls[0].function.name, "Bash");
        assert_eq!(
            turn.assistant.tool_calls[0].function.arguments,
            "{\"command\":\"echo hi\"}"
        );
        assert_eq!(seen.len(), 0);
        assert_eq!(turn.events.len(), 5);
    }

    /// Spin up a one-shot HTTP server that records the raw request and replies
    /// with `response_head` + `body`. Returns (port, join handle → request).
    fn mock_http_server(
        response_head: &'static str,
        body: &'static str,
    ) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0, "client closed before headers finished");
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&request);
            let mut content_length = 0usize;
            for line in head.lines() {
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    if let Ok(n) = v.trim().parse() {
                        content_length = n;
                        break;
                    }
                }
            }
            let header_end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            // Fold the body (whether it arrived in the same read as the
            // headers or in a later TCP segment) back into the captured
            // request, so the caller always sees the full request.
            let mut body_bytes = request.split_off(header_end);
            while body_bytes.len() < content_length {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0, "client closed before the body finished");
                body_bytes.extend_from_slice(&buf[..n]);
            }
            request.extend_from_slice(&body_bytes);
            let response = format!(
                "{response_head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            request
        });
        (port, handle)
    }

    fn mock_sse_server(body: &'static str) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
        mock_http_server("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream", body)
    }

    fn openai_client(port: u16) -> OpenAiCompatibleClient {
        OpenAiCompatibleClient {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
        }
    }

    #[tokio::test]
    async fn openai_client_streams_text_and_usage_from_local_server() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"Hello \"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"from the \"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"test provider!\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"index\":0,\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3,\"total_tokens\":8}}\n\n",
            "data: [DONE]\n\n",
        );
        let (port, server) = mock_sse_server(body);
        let turn = openai_client(port)
            .stream_chat(&ChatRequest {
                messages: vec![],
                tools: None,
                model: None,
                thinking_effort: None,
            })
            .await
            .unwrap();
        assert_eq!(turn.assistant.text, "Hello from the test provider!");
        assert!(turn.assistant.tool_calls.is_empty());
        let (prompt, completion) = turn
            .events
            .iter()
            .find_map(|e| {
                if let LlmStreamEvent::Usage {
                    prompt_tokens,
                    completion_tokens,
                    ..
                } = e
                {
                    Some((*prompt_tokens, *completion_tokens))
                } else {
                    None
                }
            })
            .expect("usage event present");
        assert_eq!((prompt, completion), (Some(5), Some(3)));

        // The server saw a well-formed OpenAI-compatible POST.
        let request = String::from_utf8_lossy(&server.join().unwrap()).into_owned();
        assert!(
            request.starts_with("POST /v1/chat/completions HTTP/1.1"),
            "request head: {request}"
        );
        let head = request.split("\r\n\r\n").next().unwrap();
        let lower = head.to_ascii_lowercase();
        assert!(
            lower.contains("authorization: bearer test-key"),
            "auth header: {head}"
        );
        assert!(lower.contains("content-type: application/json"));
        assert!(lower.contains("accept: text/event-stream"));
        let body = request.split("\r\n\r\n").nth(1).unwrap();
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["model"], "test-model");
        assert_eq!(json["stream"], true);
        assert_eq!(json["messages"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn openai_client_streams_tool_calls_from_local_server() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"\"}}]},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\":\\\"ech\"}}]},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"o hi\\\"}\"}}]},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"index\":0,\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let (port, _server) = mock_sse_server(body);
        let turn = openai_client(port)
            .stream_chat(&ChatRequest {
                messages: vec![],
                tools: None,
                model: None,
                thinking_effort: None,
            })
            .await
            .unwrap();
        assert_eq!(turn.assistant.text, "");
        assert_eq!(turn.assistant.tool_calls.len(), 1);
        let call = &turn.assistant.tool_calls[0];
        assert_eq!(call.id, "call_1");
        assert_eq!(call.function.name, "Bash");
        assert_eq!(call.function.arguments, "{\"command\":\"echo hi\"}");
        assert_eq!(
            turn.events.last(),
            Some(&LlmStreamEvent::Finish {
                finish_reason: Some("tool_calls".to_string())
            })
        );
    }

    #[tokio::test]
    async fn openai_client_streams_thinking_from_local_server() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Let me think\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Done.\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"index\":0,\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let (port, _server) = mock_sse_server(body);
        let turn = openai_client(port)
            .stream_chat(&ChatRequest {
                messages: vec![],
                tools: None,
                model: None,
                thinking_effort: None,
            })
            .await
            .unwrap();
        assert_eq!(turn.assistant.thinking, "Let me think");
        assert_eq!(turn.assistant.text, "Done.");
    }

    #[tokio::test]
    async fn openai_client_surfaces_provider_auth_error() {
        let (port, server) = mock_http_server(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json",
            r#"{"error":{"message":"bad key","type":"invalid_request_error"}}"#,
        );
        let err = openai_client(port)
            .stream_chat(&ChatRequest {
                messages: vec![],
                tools: None,
                model: None,
                thinking_effort: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code.as_deref(), Some("provider.auth_error"));
        assert!(!err.retryable);
        assert!(err.message.contains("401"), "message: {}", err.message);
        let _ = server.join().unwrap();
    }

    #[tokio::test]
    async fn openai_client_marks_rate_limit_retryable() {
        let (port, server) = mock_http_server(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nRetry-After: 2",
            r#"{"error":{"message":"slow down","type":"rate_limit_error"}}"#,
        );
        let err = openai_client(port)
            .stream_chat(&ChatRequest {
                messages: vec![],
                tools: None,
                model: None,
                thinking_effort: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code.as_deref(), Some("provider.rate_limit"));
        assert!(err.retryable);
        assert_eq!(err.retry_after_ms, Some(2_000));
        let _ = server.join().unwrap();
    }

    #[tokio::test]
    async fn openai_client_sends_tool_defs_in_openai_nested_shape() {
        // The engine's ChatRequest carries the flat aimux tool shape
        // ({type,name,description,input_schema}); the transport must wrap it
        // into the OpenAI nested {function:{name,description,parameters}} form.
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"index\":0,\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let (port, server) = mock_sse_server(body);
        let tools = Some(vec![serde_json::json!({
            "type": "function",
            "name": "Bash",
            "description": "Run a shell command",
            "input_schema": { "type": "object" },
        })]);
        let _ = openai_client(port)
            .stream_chat(&ChatRequest {
                messages: vec![],
                tools,
                model: None,
                thinking_effort: Some("high".to_string()),
            })
            .await
            .unwrap();
        let request = String::from_utf8_lossy(&server.join().unwrap()).into_owned();
        let body = request.split("\r\n\r\n").nth(1).unwrap();
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["tools"][0]["function"]["name"], "Bash");
        assert_eq!(
            json["tools"][0]["function"]["description"],
            "Run a shell command"
        );
        assert_eq!(json["tools"][0]["function"]["parameters"]["type"], "object");
        assert_eq!(json["tools"][0]["type"], "function");
        assert_eq!(json["reasoning_effort"], "high");
    }
}
