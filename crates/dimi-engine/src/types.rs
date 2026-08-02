//! Engine input/output types — the TS↔Rust swap-in socket surface.
//!
//! The engine is deliberately thin at the edges: TS hands it the assembled
//! LLM messages (context assembly stays on the TS side until slice 3) and
//! receives a stream of engine events (the same shapes the TS loop publishes
//! on its event bus). Transcript projection, broadcasting and telemetry
//! remain on the TS side.

use serde::{Deserialize, Serialize};

/// One LLM chat message — the OpenAI-compatible wire shape the TS side
/// assembles from its context (roles: system/user/assistant/tool).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<LlmToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<serde_json::Value>,
}

/// A tool call inside an assistant message (OpenAI wire shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolCall {
    pub id: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    pub function: LlmToolCallFunction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Tool definition handed to the LLM (OpenAI `tools` array entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolDef {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<String>,
    pub function: LlmToolDefFunction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolDefFunction {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// One registered engine tool (Bash in slice 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineTool {
    pub name: String,
    pub description: String,
    pub args_schema: serde_json::Value,
}

/// Provider configuration for the LLM effect boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_effort: Option<String>,
}

/// Everything the engine needs to run one turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineTurnInput {
    /// Turn id assigned by the TS side (ordinal, 1-based).
    pub turn_id: i64,
    /// Assembled conversation so far — the engine appends the user prompt
    /// and tool results as the turn progresses.
    pub messages: Vec<LlmMessage>,
    /// Tools registered for this turn (slice 1: Bash only).
    #[serde(default)]
    pub tools: Vec<EngineTool>,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub max_steps_per_turn: Option<u32>,
    /// Working directory for tool execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Shell used for Bash tool execution (default `/bin/sh`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
}

/// Why the turn ended (mirrors `TurnEndReason` in turnEvents.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnEndReason {
    Completed,
    Cancelled,
    Failed,
    Blocked,
}

/// Outcome of `Engine::run_turn`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnOutcome {
    pub status: TurnEndReason,
    pub steps: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}
