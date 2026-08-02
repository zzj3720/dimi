//! Transcript operations and op batches (`schema.ts` lines 395–446).

use serde::{Deserialize, Serialize};

use crate::entity::{Attachment, Interaction, Prompt, Todo, TranscriptMetaMerge};
use crate::frame::Frame;
use crate::id::{AgentId, FrameId, StepId, TaskId, TurnId};
use crate::item::{Item, StepKind};
use crate::model::{TurnOrigin, TurnState};
use crate::snapshot::AgentTranscriptSnapshot;

/// `transcriptTurnSchema` minus `steps` — `turnHeaderSchema`
/// (`schema.ts` 395). `kind` stays: the wire header carries `"kind":"turn"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnHeader {
    pub kind: TurnKind,
    pub turn_id: TurnId,
    pub ordinal: i64,
    pub state: TurnState,
    pub origin: TurnOrigin,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub prompt: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub attachment_ids: Option<Vec<String>>,
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
    pub usage: Option<crate::model::TranscriptUsage>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub duration_ms: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub error: Option<String>,
}

/// `transcriptTurnSchema.kind` — the literal `"turn"` tag (`schema.ts` 179).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnKind {
    Turn,
}

/// `transcriptStepSchema` minus `frames` — `stepHeaderSchema`
/// (`schema.ts` 396). `kind` stays: `"kind":"step"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepHeader {
    pub kind: StepKind,
    pub step_id: StepId,
    pub turn_id: TurnId,
    pub ordinal: i64,
    pub state: crate::model::StepState,
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
    pub usage: Option<crate::model::StepUsage>,
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
    pub timing: Option<crate::model::StepTiming>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub retry: Option<crate::model::StepRetry>,
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

/// `appendTargetSchema` (`schema.ts` 398–406).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppendTarget {
    #[serde(rename_all = "camelCase")]
    Frame {
        turn_id: TurnId,
        step_id: StepId,
        frame_id: FrameId,
    },
    #[serde(rename_all = "camelCase")]
    Task { task_id: TaskId },
}

/// `transcriptOperationSchema` — the op vocabulary (`schema.ts` 408–441).
/// Discriminated on `op`; op names contain dots, so variants are renamed
/// explicitly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    #[serde(rename_all = "camelCase")]
    Reset {
        agent_id: AgentId,
        snapshot: AgentTranscriptSnapshot,
    },
    #[serde(rename = "turn.upsert", rename_all = "camelCase")]
    TurnUpsert { turn: TurnHeader },
    #[serde(rename = "step.upsert", rename_all = "camelCase")]
    StepUpsert { turn_id: TurnId, step: StepHeader },
    #[serde(rename = "frame.upsert", rename_all = "camelCase")]
    FrameUpsert {
        turn_id: TurnId,
        step_id: StepId,
        frame: Frame,
    },
    #[serde(rename = "append", rename_all = "camelCase")]
    Append {
        target: AppendTarget,
        offset: i64,
        text: String,
    },
    #[serde(rename = "marker.upsert", rename_all = "camelCase")]
    MarkerUpsert {
        item: Item,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        before_turn: Option<i64>,
    },
    #[serde(rename = "taskref.upsert", rename_all = "camelCase")]
    TaskRefUpsert {
        item: Item,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::de::strict_option"
        )]
        before_turn: Option<i64>,
    },
    #[serde(rename = "task.upsert", rename_all = "camelCase")]
    TaskUpsert { task: crate::task::Task },
    #[serde(rename = "interaction.upsert", rename_all = "camelCase")]
    InteractionUpsert { interaction: Interaction },
    #[serde(rename = "attachment.upsert", rename_all = "camelCase")]
    AttachmentUpsert { attachment: Attachment },
    #[serde(rename = "todo.upsert", rename_all = "camelCase")]
    TodoUpsert { todo: Todo },
    #[serde(rename = "prompt.upsert", rename_all = "camelCase")]
    PromptUpsert { prompt: Prompt },
    #[serde(rename = "meta.merge", rename_all = "camelCase")]
    MetaMerge { meta: TranscriptMetaMerge },
    #[serde(rename = "items.remove", rename_all = "camelCase")]
    ItemsRemove { ids: Vec<String> },
}

/// `transcriptOpBatchSchema` (`schema.ts` 443–446).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpBatch {
    pub agent_id: AgentId,
    pub ops: Vec<Operation>,
}
