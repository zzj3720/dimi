//! Global entities and meta mirrors (`schema.ts` lines 136–143, 231–340).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::StepUsage;
use crate::phase::AgentPhase;

// ------------------------------------------------------------ interaction

/// `interactionSchema` state (`schema.ts` 140).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionState {
    Pending,
    Approved,
    Rejected,
    Cancelled,
    Answered,
    Dismissed,
}

/// `interactionSchema` (`schema.ts` 136–143).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interaction {
    pub interaction_id: String,
    /// `interactionKind` — `'approval' | 'question'`.
    pub interaction_kind: InteractionKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub tool_call_id: Option<String>,
    pub state: InteractionState,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub request: Option<Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub response: Option<Value>,
}

/// `interactionSchema.interactionKind` (`schema.ts` 138).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    Approval,
    Question,
}

// ------------------------------------------------------------ attachment

/// `attachmentSchema` source (`schema.ts` 353–358).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentSource {
    Url {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    File {
        file_id: String,
    },
}

/// `attachmentSchema` (`schema.ts` 348–360).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub attachment_id: String,
    pub media_type: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub size: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub source: Option<AttachmentSource>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub placeholder: Option<String>,
}

// ------------------------------------------------------------ todo

/// `todoItemSchema` (`schema.ts` 362–365).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub title: String,
    pub status: TodoItemStatus,
}

/// `todoItemSchema.status` (`schema.ts` 364).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoItemStatus {
    Pending,
    InProgress,
    Done,
}

/// `todoSchema` (`schema.ts` 367–371).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub todo_id: String,
    pub items: Vec<TodoItem>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub updated_at: Option<String>,
}

// ------------------------------------------------------------ prompt

/// `transcriptPromptSchema.status` (`schema.ts` 375).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStatus {
    Running,
    Queued,
    Blocked,
    Completed,
    Failed,
    Aborted,
}

/// `transcriptPromptSchema` (`schema.ts` 373–381).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub prompt_id: String,
    pub status: PromptStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub user_message_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub content: Option<Value>,
    pub created_at: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub finished_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub steered_at: Option<String>,
}

// ------------------------------------------------------------ meta

/// `goalMetaSchema` status (`schema.ts` 234).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Blocked,
    Complete,
}

/// `goalMetaSchema` (`schema.ts` 231–237).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalMeta {
    pub objective: String,
    pub status: GoalStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub completion_criterion: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub budget_used: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub budget_limit: Option<i64>,
}

/// `modesMetaSchema` plan badge (`schema.ts` 240, 246–248).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanMeta {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub review_path: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub version: Option<i64>,
}

/// `modesMetaSchema` swarm badge (`schema.ts` 241, 250).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmMeta {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub trigger: Option<String>,
}

/// `modesMetaSchema` (`schema.ts` 239–242).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModesMeta {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub plan: Option<PlanMeta>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub swarm: Option<SwarmMeta>,
}

/// `modesMetaMergeSchema` (`schema.ts` 245–251) — the `meta.merge` shape.
///
/// This is the one place in the contract where JSON `null` is legal: a mode
/// key set to `null` clears that badge. Hence the tri-state
/// `Option<Option<T>>` (absent = keep, `null` = clear, object = replace)
/// via [`crate::de::nullable_optional`] — standard `Option` deserialization
/// swallows `null` as absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModesMetaMerge {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::nullable_optional"
    )]
    pub plan: Option<Option<PlanMeta>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::nullable_optional"
    )]
    pub swarm: Option<Option<SwarmMeta>>,
}

/// `agentUsageMetaSchema` (`schema.ts` 318–322).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageMeta {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub by_model: Option<std::collections::BTreeMap<String, StepUsage>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub current_turn: Option<StepUsage>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub total: Option<StepUsage>,
}

/// `agentStatusMetaSchema.permission` (`schema.ts` 331).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Manual,
    Yolo,
    Auto,
}

/// `agentStatusMetaSchema` (`schema.ts` 324–333).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusMeta {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub model: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub thinking_effort: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub usage: Option<AgentUsageMeta>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub context_tokens: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub max_context_tokens: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub context_usage: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub permission: Option<PermissionMode>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub phase: Option<AgentPhase>,
}

/// `transcriptMetaSchema.activity` (`schema.ts` 338).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    Idle,
    Turn,
    Disposing,
    Unknown,
}

/// `transcriptMetaSchema` (`schema.ts` 335–340).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMeta {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub goal: Option<GoalMeta>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub modes: Option<ModesMeta>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub activity: Option<ActivityState>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub agent: Option<AgentStatusMeta>,
}

/// `transcriptMetaMergeSchema` (`schema.ts` 342–344) — `meta.merge` payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMetaMerge {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub goal: Option<GoalMeta>,
    /// `modes` here uses the nullable merge shape (null clears a badge).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub modes: Option<ModesMetaMerge>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub activity: Option<ActivityState>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub agent: Option<AgentStatusMeta>,
}
