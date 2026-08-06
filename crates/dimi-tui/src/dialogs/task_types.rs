//! Shared background-task types used by the tasks browser and the task output
//! viewer — a Rust-side mirror of the `BackgroundTaskInfo` /
//! `BackgroundTaskStatus` / task `kind` shapes from `@dimi-agent/dimi-sdk`
//! (only the fields the TUI reads).

use crate::theme::ColorToken;

/// `BackgroundTaskStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTaskStatus {
    Running,
    Completed,
    Failed,
    TimedOut,
    Killed,
    Lost,
}

impl BackgroundTaskStatus {
    /// `STATUS_LABEL`.
    pub fn label(&self) -> &'static str {
        match self {
            BackgroundTaskStatus::Running => "running",
            BackgroundTaskStatus::Completed => "completed",
            BackgroundTaskStatus::Failed => "failed",
            BackgroundTaskStatus::TimedOut => "timed out",
            BackgroundTaskStatus::Killed => "killed",
            BackgroundTaskStatus::Lost => "lost",
        }
    }

    /// `statusColor` → the semantic token.
    pub fn color(&self) -> ColorToken {
        match self {
            BackgroundTaskStatus::Running => ColorToken::Success,
            BackgroundTaskStatus::Completed => ColorToken::TextMuted,
            BackgroundTaskStatus::Failed
            | BackgroundTaskStatus::TimedOut
            | BackgroundTaskStatus::Killed
            | BackgroundTaskStatus::Lost => ColorToken::Error,
        }
    }

    /// `isTerminal` — a task in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            BackgroundTaskStatus::Completed
                | BackgroundTaskStatus::Failed
                | BackgroundTaskStatus::TimedOut
                | BackgroundTaskStatus::Killed
                | BackgroundTaskStatus::Lost
        )
    }
}

/// Task `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Process,
    Agent,
    Question,
}

/// `BackgroundTaskInfo` (fields the TUI reads).
#[derive(Debug, Clone)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub status: BackgroundTaskStatus,
    pub kind: TaskKind,
    pub description: String,
    pub command: Option<String>,
    pub exit_code: Option<i64>,
    pub pid: i64,
    /// Epoch ms (from the SDK wire).
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub agent_id: Option<String>,
    pub subagent_type: Option<String>,
    pub question_count: i64,
    pub tool_call_id: Option<String>,
    pub stop_reason: Option<String>,
    /// `detached` — false for foreground tasks (shown in the transcript).
    pub detached: bool,
}

impl BackgroundTaskInfo {
    /// `formatRelativeTime` — "just now" / "5m ago" / "2h ago" / "3d ago".
    pub fn format_relative_time(&self, ts: Option<i64>, now_ms: i64) -> String {
        let Some(ts) = ts else {
            return String::new();
        };
        if ts <= 0 {
            return String::new();
        }
        let diff_sec = (now_ms.saturating_sub(ts)).max(0) / 1000;
        if diff_sec < 60 {
            return "just now".to_owned();
        }
        let minutes = diff_sec / 60;
        if minutes < 60 {
            return format!("{minutes}m ago");
        }
        let hours = minutes / 60;
        if hours < 24 {
            return format!("{hours}h ago");
        }
        let days = hours / 24;
        format!("{days}d ago")
    }
}
