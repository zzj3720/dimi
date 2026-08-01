//! The task model (`transcriptTaskSchema`, `schema.ts` 215–229).

use serde::{Deserialize, Serialize};

use crate::id::{AgentId, TaskId};
use crate::model::StepUsage;

/// `transcriptTaskSchema.kind` (`schema.ts` 217).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Shell,
    Subagent,
    Tool,
    Other,
}

/// `transcriptTaskSchema.state` (`schema.ts` 218).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Running,
    Completed,
    Failed,
    TimedOut,
    Killed,
    Lost,
}

/// `transcriptTaskSchema` (`schema.ts` 215–229).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub task_id: TaskId,
    pub kind: TaskKind,
    pub state: TaskState,
    pub detached: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub description: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub agent_id: Option<AgentId>,
    pub output_tail: String,
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
    pub result_summary: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub error: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub state_reason: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub usage: Option<StepUsage>,
}
