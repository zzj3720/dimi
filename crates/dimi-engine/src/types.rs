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

/// Completion-review injection (TS `loopContinuationService` parity): after a
/// tool-free step at/after `min_steps`, the engine injects the reminder
/// message into its working history and keeps the turn alive so the model
/// must call `AllDone` instead of ending with a plain text reply. `None` =
/// disabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionReviewConfig {
    /// Step threshold (1-based, TS `COMPLETION_REVIEW_MIN_STEPS`): a tool-free
    /// step with `steps >= min_steps` triggers the review.
    pub min_steps: u32,
    /// The reminder message content (the TS `COMPLETION_REVIEW_REMINDER`
    /// wrapped as a user `<system-reminder>` message — the runner assembles
    /// the same text the TS `appendSystemReminder` would append).
    pub reminder: String,
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
    /// Step-level retry budget for transient provider failures (connection /
    /// rate limit / 5xx / timeout), mirroring the TS loop's
    /// `loop_control.maxRetriesPerStep` (default 10). `None` = engine
    /// default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_per_step: Option<u32>,
    /// Working directory for tool execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Shell used for Bash tool execution (default `/bin/sh`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Context window for per-step request assembly (tail projection;
    /// `None` = no projection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<usize>,
    /// Model context limit (tokens) driving full-history compaction; when
    /// set and the assembled request estimate crosses the trigger ratio, the
    /// engine runs a compaction round before the request (TS
    /// `fullCompaction` parity). `None` = compaction disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<u32>,
    /// First subagent agent-id number to hand out (TS `nextAvailableAgentId`
    /// parity): the runner seeds it from the session's persisted agents plus
    /// the ids this runner already handed out, so `agent-<n>` stays monotonic
    /// across turns and server restarts and never collides with TS-assigned
    /// ids. `None` = start at agent-0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_agent_id: Option<u64>,
    /// TaskStop SIGTERM grace, milliseconds (TS `task` config section
    /// `killGracePeriodMs`, default `DEFAULT_KILL_GRACE_MS`): the bridge
    /// wires it into the session's Bash tool so a cancelled background
    /// command keeps its SIGTERM cleanup window. `None` = engine default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kill_grace_ms: Option<u64>,
    /// Model override for subagents (TS `resolveSubagentBinding` parity): the
    /// resolved subagent model when it differs from the parent's provider.
    /// The Agent tool constructs a dedicated LLM client from it for every
    /// nested subagent turn. `None` = subagents inherit the parent's model
    /// (and LLM client).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_model: Option<ProviderConfig>,
    /// Allowlist of permitted subagent types (TS `subagentAllowlistFor`
    /// parity): when set, the Agent tool rejects a `subagent_type` argument
    /// outside it. `None` = no restriction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_allowlist: Option<Vec<String>>,
    /// Per-subagent timeout in milliseconds (TS `resolveSubagentTimeoutMs`,
    /// default `DEFAULT_SUBAGENT_TIMEOUT_MS` = 2h): a nested subagent turn
    /// that exceeds it settles `timed_out`. `None` = engine default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_timeout_ms: Option<u64>,
    /// Maximum concurrently running background tasks (TS
    /// `task.maxRunningTasks` parity): a subagent launch beyond the limit
    /// fails immediately without occupying a slot. `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_running_tasks: Option<u32>,
    /// Completion-review injection (TS `loopContinuationService` parity):
    /// after a tool-free step at/after the configured threshold the engine
    /// injects the reminder and keeps the turn alive until the model calls
    /// `AllDone`. `None` = disabled (the runner always passes it for runnable
    /// profiles; short turns below the threshold are unaffected).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_review: Option<CompletionReviewConfig>,
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
