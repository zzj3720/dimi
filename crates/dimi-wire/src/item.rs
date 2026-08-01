//! Transcript items: the `Step` struct and the `Item` discriminated union
//! (`schema.ts` 161–213).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::frame::Frame;
use crate::id::{StepId, TaskId, TurnId};
use crate::model::{
    StepRetry, StepState, StepTiming, StepUsage, TranscriptUsage, TurnOrigin, TurnState,
};

/// `transcriptStepSchema.kind` — the literal `"step"` tag
/// (`schema.ts` 162).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Step,
}

/// `transcriptStepSchema` (`schema.ts` 161–176). `kind` is a real field here
/// (not an injected tag): a step's wire shape always carries `"kind":"step"`,
/// and the full step appears standalone inside `Turn.steps`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub kind: StepKind,
    pub step_id: StepId,
    pub turn_id: TurnId,
    pub ordinal: i64,
    pub state: StepState,
    pub frames: Vec<Frame>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub started_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub ended_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub usage: Option<StepUsage>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub finish_reason: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub timing: Option<StepTiming>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub retry: Option<StepRetry>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub end_reason: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub end_message: Option<String>,
}

/// `transcriptItemSchema` — the item discriminated union
/// (`schema.ts` 209–213). Discriminated on `kind` (`"turn"` / `"marker"` /
/// `"taskref"`).
///
/// The full turn shape only ever appears inside this union on the wire (REST
/// items, snapshot items); standalone turn/step headers are M1's
/// `turn.upsert` / `step.upsert` shapes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // wire-faithful flat fields; boxing adds no wire benefit
pub enum Item {
    #[serde(rename_all = "camelCase")]
    Turn {
        turn_id: TurnId,
        ordinal: i64,
        state: TurnState,
        origin: TurnOrigin,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        prompt: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        attachment_ids: Option<Vec<String>>,
        steps: Vec<Step>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        started_at: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        ended_at: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        usage: Option<TranscriptUsage>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        duration_ms: Option<i64>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        error: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Marker {
        marker_id: String,
        marker: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        payload: Option<Value>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        at: Option<String>,
    },
    #[serde(rename = "taskref", rename_all = "camelCase")]
    TaskRef {
        ref_id: String,
        task_id: TaskId,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        at: Option<String>,
    },
}
