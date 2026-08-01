//! Transcript frames: the `Frame` discriminated union plus its nested
//! shapes (`schema.ts` 89–159).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{FrameId, TaskId};

/// `textFrameSchema` role (`schema.ts` 92).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextRole {
    Assistant,
    User,
}

/// `toolFrameProgressSchema` (`schema.ts` 109–115).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressKind {
    Stdout,
    Stderr,
    Progress,
    Status,
    Custom,
}

/// `toolFrameProgressSchema` (`schema.ts` 109–115). `percent` may be
/// fractional (tools report arbitrary numbers), so it is kept as
/// [`serde_json::Number`] for byte-exact round-trips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFrameProgress {
    pub kind: ProgressKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub text: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub percent: Option<serde_json::Number>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub custom_kind: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub custom_data: Option<Value>,
}

/// `agentRefSchema` (`schema.ts` 104–107).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRef {
    pub agent_id: crate::id::AgentId,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub role: Option<AgentRefRole>,
}

/// `agentRefSchema` role (`schema.ts` 106).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRefRole {
    Child,
    Member,
}

/// `toolCallFrameSchema` state (`schema.ts` 123).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    Running,
    Done,
    Error,
}

/// `noticeFrameSchema` level (`schema.ts` 148).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Error,
    Warning,
    Info,
}

/// `transcriptFrameSchema` — the frame discriminated union
/// (`schema.ts` 154–159). Discriminated on `kind`.
///
/// Note: unlike `Item` and `TurnOrigin`, `Step` carries its `kind` as a real
/// field (see [`crate::item::Step`]) — frames never appear standalone outside
/// a step, so the tag lives here only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // wire-faithful flat fields; boxing adds no wire benefit
pub enum Frame {
    #[serde(rename_all = "camelCase")]
    Text {
        frame_id: FrameId,
        role: TextRole,
        text: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        attachment_ids: Option<Vec<String>>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        task_id: Option<TaskId>,
    },
    #[serde(rename_all = "camelCase")]
    Thinking { frame_id: FrameId, text: String },
    #[serde(rename_all = "camelCase")]
    Tool {
        frame_id: FrameId,
        tool_call_id: String,
        name: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        view: Option<String>,
        state: ToolState,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        input: Option<Value>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        output: Option<Value>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        display: Option<Value>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        error: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        input_text: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        progress: Option<ToolFrameProgress>,
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
        approval_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        todo_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        agent_refs: Option<Vec<AgentRef>>,
    },
    #[serde(rename_all = "camelCase")]
    Notice {
        frame_id: FrameId,
        level: NoticeLevel,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        source: Option<String>,
        message: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        detail: Option<Value>,
    },
}
