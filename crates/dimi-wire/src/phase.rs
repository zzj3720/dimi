//! The agent phase meta model (`agentPhaseMetaSchema`, `schema.ts`
//! 254–316) — the `meta.agent.phase` wire shape, re-declared here the same
//! way the transcript package re-declares it (the transcript package must not
//! import the server).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `agentPhaseMetaSchema` `stream` (`schema.ts` 268).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Assistant,
    Thinking,
    ToolCall,
}

/// `agentPhaseMetaSchema` interrupted `reason` (`schema.ts` 305).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptReason {
    Aborted,
    MaxSteps,
    Error,
}

/// `agentPhaseMetaSchema` ended `reason` (`schema.ts` 312).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Completed,
    Cancelled,
    Failed,
    Blocked,
}

/// `agentPhaseMetaSchema` (`schema.ts` 254–316) — discriminated on `kind`.
/// `turnId` / `step` / `since` / `at` are integer-valued `z.number()` fields
/// (engine emits turn ordinals and `Date.now()` epoch ms).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentPhase {
    Idle,
    #[serde(rename_all = "camelCase")]
    Running {
        turn_id: i64,
        step: i64,
        step_id: String,
        since: i64,
    },
    #[serde(rename_all = "camelCase")]
    Streaming {
        turn_id: i64,
        step: i64,
        step_id: String,
        stream: StreamKind,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        tool_call_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        tool_name: Option<String>,
        since: i64,
    },
    #[serde(rename_all = "camelCase")]
    ToolCall {
        turn_id: i64,
        step: i64,
        tool_call_id: String,
        name: String,
        since: i64,
    },
    #[serde(rename_all = "camelCase")]
    Retrying {
        turn_id: i64,
        step: i64,
        step_id: String,
        failed_attempt: i64,
        next_attempt: i64,
        max_attempts: i64,
        delay_ms: i64,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        error_name: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        status_code: Option<i64>,
        since: i64,
    },
    #[serde(rename_all = "camelCase")]
    AwaitingApproval {
        turn_id: i64,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        step: Option<i64>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        approval: Option<Value>,
        since: i64,
    },
    #[serde(rename_all = "camelCase")]
    Interrupted {
        turn_id: i64,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        step: Option<i64>,
        reason: InterruptReason,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        message: Option<String>,
        at: i64,
    },
    #[serde(rename_all = "camelCase")]
    Ended {
        turn_id: i64,
        reason: EndReason,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        duration_ms: Option<i64>,
        at: i64,
    },
}
