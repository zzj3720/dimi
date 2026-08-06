//! Usage, timing, retry, origin and state mirrors (`schema.ts` lines 38–87).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::TaskId;

/// `transcriptUsageSchema` — per-turn aggregate usage (`schema.ts` 52–58).
///
/// Token counts are `i64`: the engine only ever emits integers for them.
/// `cost` may be fractional (USD) and is kept as [`serde_json::Number`] so
/// its integer/float representation round-trips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptUsage {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub input_tokens: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub output_tokens: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub cached_tokens: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub cache_write_tokens: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub cost: Option<serde_json::Number>,
}

/// `stepUsageSchema` — the engine's `TokenUsage` wire shape, verbatim
/// (`schema.ts` 60–65). All fields required; values are token counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepUsage {
    pub input_other: i64,
    pub output: i64,
    pub input_cache_read: i64,
    pub input_cache_creation: i64,
}

/// `stepTimingSchema` — LLM latency breakdown in milliseconds
/// (`schema.ts` 67–74). All optional; engine emits integer `Date.now()`
/// deltas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepTiming {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub llm_first_token_latency_ms: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub llm_stream_duration_ms: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub llm_request_build_ms: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub llm_server_first_token_ms: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub llm_server_decode_ms: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub llm_client_consume_ms: Option<i64>,
}

/// `stepRetrySchema` — retry bookkeeping of one step (`schema.ts` 76–84).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRetry {
    pub failed_attempt: i64,
    pub next_attempt: i64,
    pub max_attempts: i64,
    pub delay_ms: i64,
    pub error_name: String,
    pub error_message: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub status_code: Option<i64>,
}

/// `turnStateSchema` (`schema.ts` 86).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// `stepStateSchema` (`schema.ts` 87).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Running,
    Completed,
    Interrupted,
    Failed,
}

/// `turnOriginSchema` — how a turn was opened (`schema.ts` 38–50).
/// Discriminated on `kind`; `payload` is an open envelope (`z.unknown()`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnOrigin {
    #[serde(rename_all = "camelCase")]
    User {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Cron {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        task_id: Option<TaskId>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Task {
        task_id: TaskId,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Hook {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Compaction {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Side {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Other {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
    },
}
