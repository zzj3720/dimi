//! Controllers — pure state machines ported from the TS TUI controllers
//! (`apps/dimi/src/tui/controllers/*`).
//!
//! Slice-4 scope: session-event routing, streaming render state, replay, and
//! the smaller controller state machines. The Rust side has NO real
//! SDK/engine or component tree yet, so every controller here is a *pure*
//! state machine: it owns the event-routing tables, draft/flush/batch
//! policies, replay projection helpers, and the smaller state machines, and
//! surfaces render-hook effects as a typed [`StreamingEffect`] log that a real
//! component tree (later slice) will consume. SDK / async / terminal touches
//! are annotated `// TODO(legacy): …` inline.
//!
//! Submodules:
//! - [`events`] — the event taxonomy (`Event` + payload structs).
//! - [`streaming`] — draft accumulation, flush batching, tool-call grouping.
//! - [`event_handler`] — the `handle_event` routing table → state updates.
//! - [`replay`] — replay state machine + pure projection helpers.
//! - [`subagent`], [`tasks_browser`], [`btw`], [`auth_flow`],
//!   [`editor_keyboard`], [`clipboard_image_hint`], [`plugin_update`] — the
//!   smaller controller state machines.
//!
//! Shared data types (transcript entries, tool call blocks, background task
//! snapshots, …) live here so every controller speaks the same model.

pub mod auth_flow;
pub mod btw;
pub mod clipboard_image_hint;
pub mod editor_keyboard;
pub mod event_handler;
pub mod events;
pub mod plugin_update;
pub mod replay;
pub mod streaming;
pub mod subagent;
pub mod tasks_browser;

use serde_json::Value;

/// Free-form JSON object used for tool-call arguments (`Record<string,
/// unknown>` in TS).
pub type Args = serde_json::Map<String, Value>;

/// Normalize a JSON value to an object record, mirroring `argsRecord` in
/// `apps/dimi/src/tui/utils/event-payload.ts`.
pub fn args_record(args: &Value) -> Args {
    match args.as_object() {
        Some(map) => map.clone(),
        None => Args::new(),
    }
}

/// Build an `Args` map from a JSON literal (test/construction helper).
pub fn args_json(json: Value) -> Args {
    args_record(&json)
}

// ---------------------------------------------------------------------------
// Streaming phases / live pane
// ---------------------------------------------------------------------------

/// `AppState['streamingPhase']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamingPhase {
    #[default]
    Idle,
    Waiting,
    Thinking,
    Composing,
    Shell,
}

impl StreamingPhase {
    pub fn is_idle(&self) -> bool {
        matches!(self, StreamingPhase::Idle)
    }
}

/// `LivePaneState['mode']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LivePaneMode {
    #[default]
    Idle,
    Waiting,
    Thinking,
    Tool,
    Session,
}

/// `LivePaneState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePaneState {
    pub mode: LivePaneMode,
}

impl Default for LivePaneState {
    fn default() -> Self {
        LivePaneState {
            mode: LivePaneMode::Idle,
        }
    }
}

/// `PermissionMode` from the SDK (`'manual' | 'yolo' | 'auto'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionMode {
    Manual,
    Yolo,
    #[default]
    Auto,
}

// ---------------------------------------------------------------------------
// Queued messages
// ---------------------------------------------------------------------------

/// `QueuedMessage['mode']` — `undefined` in TS means prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageMode {
    Prompt,
    Bash,
}

impl MessageMode {
    pub fn is_bash(&self) -> bool {
        matches!(self, MessageMode::Bash)
    }
}

/// A message waiting in the send queue (`QueuedMessage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub text: String,
    pub mode: MessageMode,
}

impl QueuedMessage {
    pub fn prompt(text: impl Into<String>) -> Self {
        QueuedMessage {
            text: text.into(),
            mode: MessageMode::Prompt,
        }
    }

    pub fn bash(text: impl Into<String>) -> Self {
        QueuedMessage {
            text: text.into(),
            mode: MessageMode::Bash,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool calls / results
// ---------------------------------------------------------------------------

/// `ToolCallBlockData` — the subset of fields the TS handlers actually read.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallBlockData {
    pub id: String,
    pub name: String,
    pub args: Args,
    pub description: Option<String>,
    pub streaming_arguments: Option<String>,
    pub streaming_started_at_ms: Option<u64>,
    pub result: Option<ToolResultBlockData>,
    pub step: Option<u32>,
    pub turn_id: Option<String>,
    /// Set when the step ended (e.g. max_tokens) before the tool call's
    /// arguments finished streaming.
    pub truncated: bool,
}

impl ToolCallBlockData {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        ToolCallBlockData {
            id: id.into(),
            name: name.into(),
            args: Args::new(),
            description: None,
            streaming_arguments: None,
            streaming_started_at_ms: None,
            result: None,
            step: None,
            turn_id: None,
            truncated: false,
        }
    }
}

/// `ToolResultBlockData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultBlockData {
    pub tool_call_id: String,
    pub output: String,
    pub is_error: bool,
    pub synthetic: bool,
}

impl ToolResultBlockData {
    pub fn new(tool_call_id: impl Into<String>, output: impl Into<String>) -> Self {
        ToolResultBlockData {
            tool_call_id: tool_call_id.into(),
            output: output.into(),
            is_error: false,
            synthetic: false,
        }
    }
}

/// Serialize a tool result output value to its transcript string, mirroring
/// `serializeToolResultOutput` in `event-payload.ts`.
pub fn serialize_tool_result_output(output: &Value) -> String {
    match output.as_str() {
        Some(s) => s.to_owned(),
        None => serde_json::to_string_pretty(output).unwrap_or_else(|_| String::new()),
    }
}

// ---------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------

/// `BackgroundTaskStatus` from the SDK.
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

/// `BackgroundTaskInfo['kind']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTaskKind {
    Agent,
    Process,
    Tool,
    Question,
}

/// `BackgroundTaskInfo` — the subset of fields the TUI handlers read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub kind: BackgroundTaskKind,
    pub status: BackgroundTaskStatus,
    pub agent_id: Option<String>,
    pub description: Option<String>,
    pub exit_code: Option<i32>,
    pub stop_reason: Option<String>,
    pub detached: Option<bool>,
}

impl BackgroundTaskInfo {
    pub fn new(
        task_id: impl Into<String>,
        kind: BackgroundTaskKind,
        status: BackgroundTaskStatus,
    ) -> Self {
        BackgroundTaskInfo {
            task_id: task_id.into(),
            kind,
            status,
            agent_id: None,
            description: None,
            exit_code: None,
            stop_reason: None,
            detached: None,
        }
    }

    /// `isTerminalBackgroundTask` in `message-replay.ts`.
    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }
}

/// Transcript card phase for a background agent / task
/// (`BackgroundAgentStatusPhase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTaskPhase {
    Started,
    Completed,
    Failed,
}

/// `BackgroundAgentStatusData` — headline + dim detail line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgentStatusData {
    pub phase: BackgroundTaskPhase,
    pub headline: String,
    pub detail: Option<String>,
}

/// `BackgroundAgentMetadata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgentMetadata {
    pub agent_id: String,
    pub parent_tool_call_id: String,
    pub agent_name: Option<String>,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Transcript
// ---------------------------------------------------------------------------

/// `TranscriptEntryKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptEntryKind {
    Welcome,
    User,
    Assistant,
    ToolCall,
    Thinking,
    Status,
    SkillActivation,
    PluginCommand,
    Cron,
}

/// `TranscriptEntry['renderMode']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Markdown,
    Plain,
    Notice,
}

/// `SkillActivationTrigger`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillActivationTrigger {
    UserSlash,
    ModelTool,
    NestedSkill,
}

/// `CompactionTranscriptData`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompactionData {
    pub result: Option<CompactionResultKind>,
    pub summary: Option<String>,
    pub tokens_before: Option<u64>,
    pub tokens_after: Option<u64>,
    pub instruction: Option<String>,
}

/// `CompactionResult['result']` — only `'cancelled'` is ever set in the
/// transcript data (`CompactionTranscriptData`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionResultKind {
    Cancelled,
}

/// `CronTranscriptData`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CronData {
    pub job_id: Option<String>,
    pub cron: Option<String>,
    pub recurring: Option<bool>,
    pub coalesced_count: Option<u64>,
    pub stale: Option<bool>,
    pub missed_count: Option<u64>,
}

/// `SkillActivationProjection` (replay projection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillActivationProjection {
    pub activation_id: String,
    pub skill_name: String,
    pub skill_args: Option<String>,
    pub trigger: SkillActivationTrigger,
}

/// `PluginCommandProjection` (replay projection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommandProjection {
    pub activation_id: String,
    pub plugin_id: String,
    pub command_name: String,
    pub command_args: Option<String>,
}

/// `TranscriptEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    pub id: String,
    pub kind: TranscriptEntryKind,
    pub turn_id: Option<String>,
    pub render_mode: RenderMode,
    pub content: String,
    /// True only for real model-authored text (created by the assistant
    /// stream); hook-result entries share kind `Assistant` but are not
    /// replies.
    pub model_text: bool,
    pub detail: Option<String>,
    /// Optional leading-bullet override for `User` entries; empty string
    /// suppresses the bullet.
    pub bullet: Option<String>,
    pub tool_call_data: Option<ToolCallBlockData>,
    pub background_agent_status: Option<BackgroundAgentStatusData>,
    pub compaction_data: Option<CompactionData>,
    pub cron_data: Option<CronData>,
    pub skill_activation_id: Option<String>,
    pub skill_name: Option<String>,
    pub skill_args: Option<String>,
    pub skill_trigger: Option<SkillActivationTrigger>,
    pub plugin_command_data: Option<PluginCommandProjection>,
}

impl TranscriptEntry {
    pub fn new(
        id: impl Into<String>,
        kind: TranscriptEntryKind,
        turn_id: Option<String>,
        render_mode: RenderMode,
        content: impl Into<String>,
    ) -> Self {
        TranscriptEntry {
            id: id.into(),
            kind,
            turn_id,
            render_mode,
            content: content.into(),
            model_text: false,
            detail: None,
            bullet: None,
            tool_call_data: None,
            background_agent_status: None,
            compaction_data: None,
            cron_data: None,
            skill_activation_id: None,
            skill_name: None,
            skill_args: None,
            skill_trigger: None,
            plugin_command_data: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Subagent bookkeeping
// ---------------------------------------------------------------------------

/// `SubagentInfo` — remembered at `subagent.spawned`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentInfo {
    pub parent_tool_call_id: String,
    pub name: String,
    pub run_in_background: bool,
    pub swarm_index: Option<u32>,
}

// ---------------------------------------------------------------------------
// App state (the subset the controllers read/write)
// ---------------------------------------------------------------------------

/// The `AppState` fields the controllers actually touch, as a plain struct.
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub model: String,
    pub session_id: String,
    pub work_dir: String,
    pub streaming_phase: StreamingPhase,
    pub streaming_start_time: u64,
    pub is_compacting: bool,
    pub is_replaying: bool,
    pub plan_mode: bool,
    pub swarm_mode: bool,
    pub permission_mode: PermissionMode,
    pub thinking_effort: String,
    pub context_usage: Option<f64>,
    pub context_tokens: Option<u64>,
    pub max_context_tokens: Option<u64>,
    pub session_usage: Option<Value>,
    pub latest_prompt_usage: Option<Value>,
    pub session_title: Option<String>,
    pub mcp_servers_summary: Option<String>,
    /// `queuedMessageDispatchPending` — guards a message popped from the queue
    /// but not yet dispatched.
    pub queued_message_dispatch_pending: bool,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            model: String::new(),
            session_id: String::new(),
            work_dir: String::new(),
            streaming_phase: StreamingPhase::Idle,
            streaming_start_time: 0,
            is_compacting: false,
            is_replaying: false,
            plan_mode: false,
            swarm_mode: false,
            permission_mode: PermissionMode::Auto,
            thinking_effort: "off".to_owned(),
            context_usage: None,
            context_tokens: None,
            max_context_tokens: None,
            session_usage: None,
            latest_prompt_usage: None,
            session_title: None,
            mcp_servers_summary: None,
            queued_message_dispatch_pending: false,
        }
    }
}

/// A monotonic clock source so the pure controllers don't depend on
/// `Instant::now` (injected, testable).
#[derive(Debug, Clone, Copy, Default)]
pub struct Now(pub u64);

impl Now {
    pub fn ms(&self) -> u64 {
        self.0
    }
}
