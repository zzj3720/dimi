//! `dimi-wire` — byte-exact Rust mirror of the dimi transcript wire contract.
//!
//! The single source of truth is `packages/transcript/src/contract/schema.ts`
//! (zod). Every type here mirrors one schema 1:1: same JSON shape (field
//! names, ordering, optional semantics), same discrimination (`kind` tags),
//! same id rules.
//!
//! ## Mirror discipline (M0)
//!
//! - Field order follows the zod shape declaration order, so a
//!   parse → serialize round-trip is byte-identical on schema-ordered input
//!   (fixture tests enforce this).
//! - Optional (`z.foo().optional()`) maps to `Option<T>` with
//!   `#[serde(default, skip_serializing_if = "Option::is_none")]`: absent in,
//!   absent out. JSON `null` is rejected on parse (zod rejects it too) via
//!   [`de::strict_option`].
//! - `z.number().int()` and integer-valued `z.number()` fields (token counts,
//!   milliseconds, epoch ms, ordinals, attempts) are typed `i64` — the engine
//!   only ever emits integers for them (`Date.now()` deltas, token counters).
//!   A non-integer JSON number for these is rejected (stricter than zod; the
//!   M0.5 differential runner guards this against real wire data).
//! - Ambiguously-fractional `z.number()` fields (`percent`, `cost`) are typed
//!   [`serde_json::Number`] so the original integer/float representation
//!   round-trips byte-exactly.
//! - Open content envelopes (`payload`, `input`, `output`, `display`,
//!   `detail`, `customData`, …) are [`serde_json::Value`] — validated as
//!   `z.unknown()`. `preserve_order` keeps their key order byte-exact.
//! - Strings stay plain `String` (timestamps are ISO strings, never parsed).
//! - Unknown JSON fields are ignored on parse, like zod's default `strip`
//!   object behavior.
//!
//! ## M0 schema audit (schema.ts → dimi-wire)
//!
//! | schema.ts export | dimi-wire type |
//! |---|---|
//! | turnIdSchema / stepIdSchema / frameIdSchema / taskIdSchema / agentIdSchema | [`id::TurnId`] / [`id::StepId`] / [`id::FrameId`] / [`id::TaskId`] / [`id::AgentId`] |
//! | isPlainAgentId | [`id::is_plain_agent_id`] |
//! | turnOriginSchema | [`model::TurnOrigin`] |
//! | transcriptUsageSchema | [`model::TranscriptUsage`] |
//! | stepUsageSchema | [`model::StepUsage`] |
//! | stepTimingSchema | [`model::StepTiming`] |
//! | stepRetrySchema | [`model::StepRetry`] |
//! | turnStateSchema / stepStateSchema | [`model::TurnState`] / [`model::StepState`] |
//! | textFrameSchema / thinkingFrameSchema / toolCallFrameSchema / noticeFrameSchema | [`frame::Frame`] (+ [`frame::ToolFrameProgress`], [`frame::AgentRef`]) |
//! | transcriptStepSchema | [`item::Step`] |
//! | transcriptTurnSchema | [`item::Item::Turn`] |
//! | transcriptMarkerSchema | [`item::Item::Marker`] |
//! | transcriptTaskRefSchema | [`item::Item::TaskRef`] |
//! | transcriptItemSchema | [`item::Item`] |
//! | transcriptTaskSchema | [`task::Task`] |
//! | agentPhaseMetaSchema | [`phase::AgentPhase`] |
//!
//! Out of M0 scope (land with their milestones): attachment / todo / prompt /
//! interaction / meta (goal, modes, agent status), ops, subscription and REST
//! shapes, turn/step headers (M1), transcripts ops batch (M1).

pub mod de;
pub mod entity;
pub mod frame;
pub mod id;
pub mod item;
pub mod model;
pub mod op;
pub mod phase;
pub mod record;
pub mod snapshot;
pub mod task;

pub use de::strict_option;
pub use entity::{
    ActivityState, AgentStatusMeta, AgentUsageMeta, Attachment, AttachmentSource, GoalMeta,
    GoalStatus, Interaction, InteractionKind, InteractionState, ModesMeta, ModesMetaMerge,
    PermissionMode, PlanMeta, Prompt, PromptStatus, SwarmMeta, Todo, TodoItem, TodoItemStatus,
    TranscriptMeta, TranscriptMetaMerge,
};
pub use frame::{
    AgentRef, Frame, NoticeLevel, ProgressKind, TextRole, ToolFrameProgress, ToolState,
};
pub use id::{AgentId, FrameId, StepId, TaskId, TurnId, is_plain_agent_id};
pub use item::{Item, Step, StepKind};
pub use model::{
    StepRetry, StepState, StepTiming, StepUsage, TranscriptUsage, TurnOrigin, TurnState,
};
pub use op::{AppendTarget, OpBatch, Operation, StepHeader, TurnHeader, TurnKind};
pub use phase::{AgentPhase, EndReason, InterruptReason, StreamKind};
pub use record::{HistoryMessage, MessageOrigin, ToolCall, WireMetadataRecord, WireRecord};
pub use snapshot::AgentTranscriptSnapshot;
pub use task::{Task, TaskKind, TaskState};
