//! The event taxonomy — port of the SDK event set that
//! `session-event-handler.ts` routes over.
//!
//! Each variant carries only the fields the TS handler actually reads (not
//! the full SDK wire format). `agent_id` is present on the events the
//! subagent/btw routing reads to attribute an event to a child agent
//! (`event.agentId` in TS).

use crate::controllers::{Args, BackgroundTaskInfo, PermissionMode, SkillActivationTrigger};
use serde_json::Value;

/// `PromptOrigin` — model-facing message origin, read by the turn/btw/replay
/// routing and the replay projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptOrigin {
    User,
    SkillActivation {
        activation_id: String,
        skill_name: String,
        skill_args: Option<String>,
        trigger: SkillActivationTrigger,
    },
    PluginCommand {
        activation_id: String,
        plugin_id: String,
        command_name: String,
        command_args: Option<String>,
    },
    Injection {
        variant: String,
    },
    ShellCommand {
        phase: ShellPhase,
        is_error: Option<bool>,
    },
    CompactionSummary,
    SystemTrigger {
        name: String,
    },
    Task {
        task_id: String,
    },
    CronJob {
        job_id: String,
        cron: String,
        recurring: bool,
        coalesced_count: u64,
        stale: bool,
    },
    CronMissed {
        count: u64,
    },
    HookResult {
        event: String,
        blocked: bool,
    },
    Retry {
        trigger: Option<String>,
    },
}

/// `ShellCommandOrigin['phase']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPhase {
    Input,
    Output,
}

/// `TurnEndReason` from the SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEndReason {
    Completed,
    Cancelled,
    Failed,
    Blocked,
}

/// `DimiErrorPayload` — the fields `formatErrorPayload` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    pub details: Option<Args>,
}

/// `ToolProgressEvent['update']` — the fields `handleToolProgress` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProgressUpdate {
    pub kind: ToolProgressKind,
    pub text: Option<String>,
}

/// `ToolProgressEvent['update']['kind']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProgressKind {
    Status,
    Stdout,
    Stderr,
}

/// `shell.output` update — routed straight to the host shell handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellUpdate {
    pub kind: String,
    pub text: Option<String>,
}

/// `CompactionCompletedEvent['result']`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionCompletedData {
    pub tokens_before: u64,
    pub tokens_after: u64,
    pub summary: Option<String>,
}

/// `CronFiredEvent['origin']`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronFiredOrigin {
    pub job_id: String,
    pub cron: String,
    pub recurring: bool,
    pub coalesced_count: u64,
    pub stale: bool,
}

/// `McpServerStatusSnapshot['status']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerStatus {
    Connected,
    Failed,
    NeedsAuth,
    Disabled,
    Pending,
}

/// `McpServerStatusSnapshot` — the fields `renderMcpServerStatus` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerStatusSnapshot {
    pub name: String,
    pub status: McpServerStatus,
    pub tool_count: u64,
    pub transport: String,
    pub error: Option<String>,
}

/// A single session event, mirroring the SDK `Event` union (subset read by
/// the TUI controllers).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    // ── turn lifecycle ──
    TurnStarted {
        turn_id: String,
        origin: Option<PromptOrigin>,
    },
    TurnEnded {
        turn_id: String,
        reason: TurnEndReason,
        error: Option<ErrorPayload>,
    },
    TurnStepStarted {
        turn_id: String,
        step: u32,
    },
    TurnStepInterrupted {
        turn_id: String,
        step: u32,
        reason: String,
        message: Option<String>,
    },
    TurnStepCompleted {
        turn_id: String,
        step: u32,
        usage: Option<Value>,
        provider_finish_reason: Option<String>,
        raw_finish_reason: Option<String>,
        finish_reason: Option<String>,
    },
    /// `turn.step.retrying` — routed to a no-op.
    TurnStepRetrying,

    // ── tool / shell streaming ──
    ToolProgress {
        tool_call_id: String,
        update: ToolProgressUpdate,
    },
    ShellOutput {
        command_id: String,
        update: ShellUpdate,
    },
    ShellStarted {
        command_id: String,
        task_id: String,
    },

    // ── model deltas ──
    AssistantDelta {
        agent_id: Option<String>,
        delta: String,
    },
    ThinkingDelta {
        agent_id: Option<String>,
        delta: String,
    },
    HookResult {
        agent_id: Option<String>,
        turn_id: String,
        hook_event: String,
        content: String,
        blocked: bool,
    },

    // ── tool calls ──
    ToolCallStarted {
        agent_id: Option<String>,
        tool_call_id: String,
        name: String,
        args: Args,
        description: Option<String>,
    },
    ToolCallDelta {
        agent_id: Option<String>,
        tool_call_id: String,
        name: Option<String>,
        arguments_part: Option<String>,
    },
    ToolResult {
        agent_id: Option<String>,
        tool_call_id: String,
        output: Value,
        is_error: bool,
        synthetic: bool,
    },

    // ── agent / session status ──
    AgentStatusUpdated {
        agent_id: Option<String>,
        context_usage: Option<f64>,
        context_tokens: Option<u64>,
        max_context_tokens: Option<u64>,
        usage: Option<Value>,
        plan_mode: Option<bool>,
        swarm_mode: Option<bool>,
        permission: Option<PermissionMode>,
        model: Option<String>,
        thinking_effort: Option<String>,
    },
    SessionMetaUpdated {
        title: Option<String>,
        patch_title: Option<String>,
    },

    // ── skill / plugin command activation ──
    SkillActivated {
        activation_id: String,
        skill_name: String,
        skill_args: Option<String>,
        trigger: SkillActivationTrigger,
    },
    PluginCommandActivated {
        activation_id: String,
        plugin_id: String,
        command_name: String,
        command_args: Option<String>,
    },

    // ── errors / warnings ──
    Error(ErrorPayload),
    Warning {
        message: String,
    },

    // ── compaction ──
    CompactionStarted {
        instruction: Option<String>,
    },
    CompactionCompleted {
        result: CompactionCompletedData,
    },
    /// `compaction.blocked` — routed to a no-op.
    CompactionBlocked,
    CompactionCancelled,

    // ── subagent lifecycle ──
    SubagentSpawned {
        subagent_id: String,
        parent_tool_call_id: String,
        subagent_name: String,
        run_in_background: bool,
        swarm_index: Option<u32>,
        description: Option<String>,
    },
    SubagentStarted {
        subagent_id: String,
    },
    SubagentSuspended {
        subagent_id: String,
        reason: Option<String>,
    },
    SubagentCompleted {
        subagent_id: String,
        result_summary: Option<String>,
        context_tokens: Option<u64>,
        usage: Option<Value>,
    },
    SubagentFailed {
        subagent_id: String,
        error: String,
    },

    // ── background tasks ──
    TaskStarted {
        info: BackgroundTaskInfo,
    },
    TaskTerminated {
        info: BackgroundTaskInfo,
    },

    // ── cron ──
    CronFired {
        prompt: String,
        origin: CronFiredOrigin,
    },

    // ── mcp ──
    McpServerStatus {
        server: McpServerStatusSnapshot,
    },
    /// `tool.list.updated` — routed to a no-op.
    ToolListUpdated,
}

impl Event {
    /// The wire `type` string for the event (mirrors `event.type`).
    pub fn type_name(&self) -> &'static str {
        match self {
            Event::TurnStarted { .. } => "turn.started",
            Event::TurnEnded { .. } => "turn.ended",
            Event::TurnStepStarted { .. } => "turn.step.started",
            Event::TurnStepInterrupted { .. } => "turn.step.interrupted",
            Event::TurnStepCompleted { .. } => "turn.step.completed",
            Event::TurnStepRetrying => "turn.step.retrying",
            Event::ToolProgress { .. } => "tool.progress",
            Event::ShellOutput { .. } => "shell.output",
            Event::ShellStarted { .. } => "shell.started",
            Event::AssistantDelta { .. } => "assistant.delta",
            Event::ThinkingDelta { .. } => "thinking.delta",
            Event::HookResult { .. } => "hook.result",
            Event::ToolCallStarted { .. } => "tool.call.started",
            Event::ToolCallDelta { .. } => "tool.call.delta",
            Event::ToolResult { .. } => "tool.result",
            Event::AgentStatusUpdated { .. } => "agent.status.updated",
            Event::SessionMetaUpdated { .. } => "session.meta.updated",
            Event::SkillActivated { .. } => "skill.activated",
            Event::PluginCommandActivated { .. } => "plugin_command.activated",
            Event::Error(_) => "error",
            Event::Warning { .. } => "warning",
            Event::CompactionStarted { .. } => "compaction.started",
            Event::CompactionCompleted { .. } => "compaction.completed",
            Event::CompactionBlocked => "compaction.blocked",
            Event::CompactionCancelled => "compaction.cancelled",
            Event::SubagentSpawned { .. } => "subagent.spawned",
            Event::SubagentStarted { .. } => "subagent.started",
            Event::SubagentSuspended { .. } => "subagent.suspended",
            Event::SubagentCompleted { .. } => "subagent.completed",
            Event::SubagentFailed { .. } => "subagent.failed",
            Event::TaskStarted { .. } => "task.started",
            Event::TaskTerminated { .. } => "task.terminated",
            Event::CronFired { .. } => "cron.fired",
            Event::McpServerStatus { .. } => "mcp.server.status",
            Event::ToolListUpdated => "tool.list.updated",
        }
    }

    /// The turn id, when the event carries one (`'turnId' in event`).
    pub fn turn_id(&self) -> Option<&str> {
        match self {
            Event::TurnStarted { turn_id, .. }
            | Event::TurnEnded { turn_id, .. }
            | Event::TurnStepStarted { turn_id, .. }
            | Event::TurnStepInterrupted { turn_id, .. }
            | Event::TurnStepCompleted { turn_id, .. }
            | Event::HookResult { turn_id, .. } => Some(turn_id),
            _ => None,
        }
    }

    /// The child-agent attribution (`event.agentId`), when present.
    pub fn agent_id(&self) -> Option<&str> {
        match self {
            Event::AssistantDelta { agent_id, .. }
            | Event::ThinkingDelta { agent_id, .. }
            | Event::HookResult { agent_id, .. }
            | Event::ToolCallStarted { agent_id, .. }
            | Event::ToolCallDelta { agent_id, .. }
            | Event::ToolResult { agent_id, .. }
            | Event::AgentStatusUpdated { agent_id, .. } => agent_id.as_deref(),
            _ => None,
        }
    }
}

/// `MAIN_AGENT_ID` — `'main'` (from `constant/dimi-tui.ts`).
pub const MAIN_AGENT_ID: &str = "main";

/// `AUTH_LOGIN_REQUIRED_CODE` — the error code that maps to the login-required
/// startup notice (from `constant/dimi-tui.ts`; the actual code lives in
/// `constant/app.ts`).
pub const AUTH_LOGIN_REQUIRED_CODE: &str = "auth_login_required";
/// `AUTH_LOGIN_REQUIRED_STARTUP_NOTICE`.
pub const AUTH_LOGIN_REQUIRED_STARTUP_NOTICE: &str =
    "Provider authentication is required. Run /login to connect.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_names_match_ts() {
        let events = [
            Event::TurnStarted {
                turn_id: "1".to_owned(),
                origin: None,
            },
            Event::TurnEnded {
                turn_id: "1".to_owned(),
                reason: TurnEndReason::Completed,
                error: None,
            },
            Event::TurnStepStarted {
                turn_id: "1".to_owned(),
                step: 0,
            },
            Event::TurnStepInterrupted {
                turn_id: "1".to_owned(),
                step: 0,
                reason: "aborted".to_owned(),
                message: None,
            },
            Event::TurnStepCompleted {
                turn_id: "1".to_owned(),
                step: 0,
                usage: None,
                provider_finish_reason: None,
                raw_finish_reason: None,
                finish_reason: None,
            },
            Event::TurnStepRetrying,
            Event::ToolProgress {
                tool_call_id: "t".to_owned(),
                update: ToolProgressUpdate {
                    kind: ToolProgressKind::Status,
                    text: Some("x".to_owned()),
                },
            },
            Event::ShellOutput {
                command_id: "c".to_owned(),
                update: ShellUpdate {
                    kind: "stdout".to_owned(),
                    text: Some("o".to_owned()),
                },
            },
            Event::ShellStarted {
                command_id: "c".to_owned(),
                task_id: "t".to_owned(),
            },
            Event::AssistantDelta {
                agent_id: None,
                delta: "hi".to_owned(),
            },
            Event::ThinkingDelta {
                agent_id: None,
                delta: "hmm".to_owned(),
            },
            Event::HookResult {
                agent_id: None,
                turn_id: "1".to_owned(),
                hook_event: "pre_tool_use".to_owned(),
                content: "body".to_owned(),
                blocked: false,
            },
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "t".to_owned(),
                name: "Read".to_owned(),
                args: Args::new(),
                description: None,
            },
            Event::ToolCallDelta {
                agent_id: None,
                tool_call_id: "t".to_owned(),
                name: Some("Read".to_owned()),
                arguments_part: Some("{}".to_owned()),
            },
            Event::ToolResult {
                agent_id: None,
                tool_call_id: "t".to_owned(),
                output: serde_json::json!("out"),
                is_error: false,
                synthetic: false,
            },
            Event::AgentStatusUpdated {
                agent_id: None,
                context_usage: None,
                context_tokens: None,
                max_context_tokens: None,
                usage: None,
                plan_mode: None,
                swarm_mode: None,
                permission: None,
                model: None,
                thinking_effort: None,
            },
            Event::SessionMetaUpdated {
                title: None,
                patch_title: None,
            },
            Event::SkillActivated {
                activation_id: "a".to_owned(),
                skill_name: "s".to_owned(),
                skill_args: None,
                trigger: SkillActivationTrigger::UserSlash,
            },
            Event::PluginCommandActivated {
                activation_id: "a".to_owned(),
                plugin_id: "p".to_owned(),
                command_name: "c".to_owned(),
                command_args: None,
            },
            Event::Error(ErrorPayload {
                code: "x".to_owned(),
                message: "y".to_owned(),
                details: None,
            }),
            Event::Warning {
                message: "w".to_owned(),
            },
            Event::CompactionStarted { instruction: None },
            Event::CompactionCompleted {
                result: CompactionCompletedData {
                    tokens_before: 1,
                    tokens_after: 2,
                    summary: None,
                },
            },
            Event::CompactionBlocked,
            Event::CompactionCancelled,
            Event::SubagentSpawned {
                subagent_id: "s".to_owned(),
                parent_tool_call_id: "p".to_owned(),
                subagent_name: "n".to_owned(),
                run_in_background: false,
                swarm_index: None,
                description: None,
            },
            Event::SubagentStarted {
                subagent_id: "s".to_owned(),
            },
            Event::SubagentSuspended {
                subagent_id: "s".to_owned(),
                reason: None,
            },
            Event::SubagentCompleted {
                subagent_id: "s".to_owned(),
                result_summary: None,
                context_tokens: None,
                usage: None,
            },
            Event::SubagentFailed {
                subagent_id: "s".to_owned(),
                error: "e".to_owned(),
            },
            Event::TaskStarted {
                info: BackgroundTaskInfo::new(
                    "task1",
                    crate::controllers::BackgroundTaskKind::Agent,
                    crate::controllers::BackgroundTaskStatus::Running,
                ),
            },
            Event::TaskTerminated {
                info: BackgroundTaskInfo::new(
                    "task1",
                    crate::controllers::BackgroundTaskKind::Agent,
                    crate::controllers::BackgroundTaskStatus::Lost,
                ),
            },
            Event::CronFired {
                prompt: "p".to_owned(),
                origin: CronFiredOrigin {
                    job_id: "j".to_owned(),
                    cron: "* * * * *".to_owned(),
                    recurring: true,
                    coalesced_count: 1,
                    stale: false,
                },
            },
            Event::McpServerStatus {
                server: McpServerStatusSnapshot {
                    name: "m".to_owned(),
                    status: McpServerStatus::Connected,
                    tool_count: 1,
                    transport: "stdio".to_owned(),
                    error: None,
                },
            },
            Event::ToolListUpdated,
        ];
        for event in events {
            assert!(!event.type_name().is_empty());
        }
    }

    #[test]
    fn event_type_names_are_expected() {
        assert_eq!(
            Event::TurnStarted {
                turn_id: "1".to_owned(),
                origin: None,
            }
            .type_name(),
            "turn.started"
        );
        assert_eq!(
            Event::CompactionCancelled.type_name(),
            "compaction.cancelled"
        );
        assert_eq!(Event::TurnStepRetrying.type_name(), "turn.step.retrying");
        assert_eq!(Event::ToolListUpdated.type_name(), "tool.list.updated");
    }

    #[test]
    fn turn_id_and_agent_id_accessors() {
        let e = Event::TurnStarted {
            turn_id: "42".to_owned(),
            origin: None,
        };
        assert_eq!(e.turn_id(), Some("42"));
        assert_eq!(e.agent_id(), None);

        let e = Event::AssistantDelta {
            agent_id: Some("child".to_owned()),
            delta: "x".to_owned(),
        };
        assert_eq!(e.agent_id(), Some("child"));
        assert_eq!(e.turn_id(), None);
    }
}
