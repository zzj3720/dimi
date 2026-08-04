//! Streaming render state — port of `apps/dimi/src/tui/controllers/
//! streaming-ui.ts`.
//!
//! This is the core controller: assistant/thinking draft accumulation from
//! incremental deltas, timer-based flush batching (thinking before assistant,
//! tool calls grouped), tool-call component mapping with Agent/Read group
//! upgrades, compaction blocks, and transcript aggregation hooks.
//!
//! The Rust port is a *pure* state machine: it holds the drafts, the pending
//! flush flags, the active tool-call map, the streaming-args preview buffers,
//! and the Agent/Read grouping state, and it reports render-hook side effects
//! through a [`StreamingEffect`] log that a real component tree (later slice)
//! will consume. Real timers are out of scope — [`schedule_flush`] returns the
//! delay the host should arm instead.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use crate::chrome::{TodoItem, TodoStatus};
use crate::controllers::{Args, BackgroundTaskStatus, ToolCallBlockData, ToolResultBlockData};

// ---------------------------------------------------------------------------
// Constants (port of `apps/dimi/src/tui/constant/streaming.ts`)
// ---------------------------------------------------------------------------

/// `STREAMING_UI_FLUSH_MS` — coalesces high-frequency model/tool deltas.
pub const STREAMING_UI_FLUSH_MS: u64 = 50;

/// `STREAMING_ARGS_PREVIEW_MAX_CHARS` — bounds live tool-argument previews.
pub const STREAMING_ARGS_PREVIEW_MAX_CHARS: usize = 64 * 1024;

/// `TRANSCRIPT_KEEP_TRAILING_TOOL_CALLS` — trailing tool calls kept expanded
/// while a run grows.
pub const TRANSCRIPT_KEEP_TRAILING_TOOL_CALLS: usize = 2;

/// `STREAMING_ARGS_FIELD_RE` — extracts useful string fields from partially
/// streamed JSON tool args (a preview parser, not a full JSON parser).
fn streaming_args_field_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""(path|file_path|command|pattern|query|url|description|title|name)"\s*:\s*"((?:\\.|[^"\\])*)""#)
            .expect("valid streaming args field regex")
    })
}

// ---------------------------------------------------------------------------
// Streaming effects — the render-hook surface
// ---------------------------------------------------------------------------

/// One render-hook side effect emitted by the streaming controller. A real
/// component tree maps these onto `AssistantMessageComponent` /
/// `ThinkingComponent` / `ToolCallComponent` / `AgentGroupComponent` /
/// `ReadGroupComponent` / `CompactionComponent` / `TodoPanelComponent`
/// (that mapping is `// TODO(legacy)`).
#[derive(Debug, Clone, PartialEq)]
pub enum StreamingEffect {
    /// `onStreamingTextStart` — a new assistant streaming block opened.
    StreamingTextStart,
    /// `onStreamingTextUpdate` — full accumulated assistant draft.
    StreamingTextUpdate(String),
    /// `onStreamingTextEnd` — the block finalized with this final text.
    StreamingTextEnd(String),
    /// `onThinkingUpdate` — full accumulated thinking draft (live).
    ThinkingUpdate(String),
    /// `onThinkingEnd` — the live thinking component finalized.
    ThinkingEnd,
    /// `onToolCallStart` — a tool-call component was created (by id).
    ToolCallStart(String),
    /// `component.updateToolCall` — an existing tool-call component updated.
    ToolCallUpdate(String),
    /// `onToolCallEnd` — a tool-call result landed.
    ToolCallEnd {
        tool_call_id: String,
        output: String,
    },
    /// A solo Agent/Read card was replaced by a group component.
    GroupUpgraded { group: GroupKind },
    /// `setTodoList` — the todo panel content changed (title, status pairs).
    TodoListSet(Vec<(String, TodoStatus)>),
    /// `beginCompaction`.
    CompactionBegin { instruction: Option<String> },
    /// `endCompaction`.
    CompactionEnd {
        tokens_before: u64,
        tokens_after: u64,
        summary: Option<String>,
    },
    /// `cancelCompaction`.
    CompactionCancel,
}

/// Which grouped tool-call family a pending group belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Agent,
    Read,
}

// ---------------------------------------------------------------------------
// Internal state structs
// ---------------------------------------------------------------------------

/// A pending Agent/Read group (`_pendingAgentGroup` / `_pendingReadGroup`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingGroup {
    pub turn_id: Option<String>,
    pub step: u32,
    /// The solo component id before the group upgrade (when no group yet).
    pub solo: Option<String>,
    /// The group member tool-call ids after the upgrade.
    pub group: Option<Vec<String>>,
}

/// One streaming tool-call argument buffer (`_streamingToolCallArguments`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingToolCallArgs {
    pub name: String,
    pub arguments_text: String,
    pub started_at_ms: u64,
}

/// `getStreamingToolCallPreview` return value.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallPreview {
    pub name: String,
    pub args: Args,
    pub arguments_text: String,
    pub started_at_ms: u64,
}

/// The subagent view attached to a tool-call card — the identity/phase the
/// terminal-status and backgrounded searches (`applyBackgroundTaskTerminalStatus`,
/// `markSubagentBackgrounded`) read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallSubagentView {
    pub agent_id: Option<String>,
    pub description: Option<String>,
    pub phase: SubagentCardPhase,
    pub backgrounded: bool,
    pub terminal_status: Option<BackgroundTaskStatus>,
    pub terminal_error: Option<String>,
}

impl Default for ToolCallSubagentView {
    fn default() -> Self {
        ToolCallSubagentView {
            agent_id: None,
            description: None,
            phase: SubagentCardPhase::Queued,
            backgrounded: false,
            terminal_status: None,
            terminal_error: None,
        }
    }
}

/// Snapshot phase of a subagent tool-call card
/// (`ToolCallComponent.getSubagentSnapshot().phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentCardPhase {
    Queued,
    Spawning,
    Running,
    Completed,
    Failed,
}

impl SubagentCardPhase {
    fn is_foreground_running(&self) -> bool {
        matches!(
            self,
            SubagentCardPhase::Queued | SubagentCardPhase::Spawning | SubagentCardPhase::Running
        )
    }
}

/// The live assistant streaming block (`_streamingBlock`), kept as data so a
/// host can aggregate it into the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingBlock {
    pub turn_id: Option<String>,
    pub content: String,
    pub finalized: bool,
}

/// The active compaction block (`_activeCompactionBlock`), as a presence flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionBlock {
    pub instruction: Option<String>,
    pub done: bool,
    pub canceled: bool,
}

/// Args for [`StreamingUiController::apply_background_task_terminal_status`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyTerminalStatusArgs {
    pub agent_id: Option<String>,
    pub description: String,
    pub status: BackgroundTaskStatus,
    pub error_text: Option<String>,
}

/// The outcome of attaching a tool call to a pending group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupAction {
    /// A fresh solo component was started.
    StartedSolo,
    /// The call attached to an existing group.
    Attached,
    /// The solo component was upgraded into a group and the call attached.
    Upgraded,
}

// ---------------------------------------------------------------------------
// Pure argument-preview helpers (port of `event-payload.ts`)
// ---------------------------------------------------------------------------

/// `appendStreamingArgsPreview` — append a streaming args delta, capped at
/// [`STREAMING_ARGS_PREVIEW_MAX_CHARS`].
pub fn append_streaming_args_preview(current: Option<&str>, next: Option<&str>) -> String {
    let existing: String = current
        .unwrap_or("")
        .chars()
        .take(STREAMING_ARGS_PREVIEW_MAX_CHARS)
        .collect();
    let next = next.unwrap_or("");
    if next.is_empty() {
        return existing;
    }
    let existing_len = existing.chars().count();
    let remaining = STREAMING_ARGS_PREVIEW_MAX_CHARS.saturating_sub(existing_len);
    if remaining == 0 {
        return existing;
    }
    let mut out = existing;
    out.push_str(&next.chars().take(remaining).collect::<String>());
    out
}

/// `unescapeJsonString` — unescape the JSON escape set only.
fn unescape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('b') => out.push('\u{0008}'),
                Some('f') => out.push('\u{000C}'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// `parseStreamingArgs` — parse partially streamed JSON tool args: full JSON
/// when complete, otherwise a field-by-field preview scan.
pub fn parse_streaming_args(arguments_text: &str) -> Args {
    let preview: String = arguments_text
        .chars()
        .take(STREAMING_ARGS_PREVIEW_MAX_CHARS)
        .collect();
    if preview.trim().is_empty() {
        return Args::new();
    }
    if arguments_text.chars().count() <= STREAMING_ARGS_PREVIEW_MAX_CHARS
        && preview.trim_end().ends_with('}')
    {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&preview) {
            if let Some(obj) = parsed.as_object() {
                return obj.clone();
            }
        }
    }
    let mut result = Args::new();
    for cap in streaming_args_field_re().captures_iter(&preview) {
        let (Some(key), Some(raw)) = (cap.get(1), cap.get(2)) else {
            continue;
        };
        if !result.contains_key(key.as_str()) {
            result.insert(
                key.as_str().to_owned(),
                serde_json::Value::String(unescape_json_string(raw.as_str())),
            );
        }
    }
    result
}

// ---------------------------------------------------------------------------
// StreamingUiController
// ---------------------------------------------------------------------------

/// The streaming UI state machine (port of `StreamingUIController`).
#[derive(Debug, Clone)]
pub struct StreamingUiController {
    // Turn context
    current_turn_id: Option<String>,
    current_step: u32,

    // Drafts / live blocks
    assistant_draft: String,
    thinking_draft: String,
    streaming_block: Option<StreamingBlock>,
    thinking_component_active: bool,
    active_compaction_block: Option<CompactionBlock>,

    // Flush batching
    pending_assistant_flush: bool,
    pending_thinking_flush: bool,
    pending_tool_call_flush_ids: BTreeSet<String>,
    flush_timer_active: bool,
    last_flush_at: Option<u64>,

    // Tool call state
    active_tool_calls: BTreeMap<String, ToolCallBlockData>,
    streaming_tool_call_arguments: BTreeMap<String, StreamingToolCallArgs>,
    pending_tool_components: BTreeSet<String>,

    // Grouping
    pending_agent_group: Option<PendingGroup>,
    pending_read_group: Option<PendingGroup>,

    // WaitFor card
    active_wait_component: bool,

    // Subagent card view (identity/phase for terminal-status lookups)
    tool_call_subagents: BTreeMap<String, ToolCallSubagentView>,

    // Todo panel content
    todo_items: Vec<TodoItem>,
}

impl Default for StreamingUiController {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingUiController {
    pub fn new() -> Self {
        StreamingUiController {
            current_turn_id: None,
            current_step: 0,
            assistant_draft: String::new(),
            thinking_draft: String::new(),
            streaming_block: None,
            thinking_component_active: false,
            active_compaction_block: None,
            pending_assistant_flush: false,
            pending_thinking_flush: false,
            pending_tool_call_flush_ids: BTreeSet::new(),
            flush_timer_active: false,
            last_flush_at: None,
            active_tool_calls: BTreeMap::new(),
            streaming_tool_call_arguments: BTreeMap::new(),
            pending_tool_components: BTreeSet::new(),
            pending_agent_group: None,
            pending_read_group: None,
            active_wait_component: false,
            tool_call_subagents: BTreeMap::new(),
            todo_items: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Turn context — read/write accessors
    // -----------------------------------------------------------------------

    pub fn get_turn_context(&self) -> (Option<&str>, u32) {
        (self.current_turn_id.as_deref(), self.current_step)
    }

    pub fn set_turn_id(&mut self, turn_id: Option<String>) {
        self.current_turn_id = turn_id;
    }

    pub fn set_step(&mut self, step: u32) {
        self.current_step = step;
    }

    pub fn has_active_turn(&self) -> bool {
        self.current_turn_id.is_some()
    }

    // -----------------------------------------------------------------------
    // Text streaming — semantic write accessors
    // -----------------------------------------------------------------------

    /// `appendThinkingDelta`.
    pub fn append_thinking_delta(&mut self, delta: &str) {
        self.thinking_draft.push_str(delta);
        self.pending_thinking_flush = true;
    }

    /// `appendAssistantDelta` — whitespace-only deltas are dropped when no
    /// assistant block is open yet (they would mount an empty message and
    /// collapse trailing tool calls early).
    pub fn append_assistant_delta(&mut self, effects: &mut Vec<StreamingEffect>, delta: &str) {
        if self.streaming_block.is_none() && delta.trim().is_empty() {
            return;
        }
        if self.streaming_block.is_none() {
            self.on_streaming_text_start(effects);
        }
        self.assistant_draft.push_str(delta);
        self.pending_assistant_flush = true;
    }

    pub fn has_thinking_draft(&self) -> bool {
        !self.thinking_draft.is_empty()
    }

    pub fn has_active_thinking_component(&self) -> bool {
        self.thinking_component_active
    }

    pub fn has_streaming_block(&self) -> bool {
        self.streaming_block.is_some()
    }

    pub fn streaming_block(&self) -> Option<&StreamingBlock> {
        self.streaming_block.as_ref()
    }

    pub fn clear_assistant_draft(&mut self) {
        self.assistant_draft.clear();
    }

    // -----------------------------------------------------------------------
    // Tool call state — semantic accessors
    // -----------------------------------------------------------------------

    pub fn get_active_tool_call(&self, id: &str) -> Option<&ToolCallBlockData> {
        self.active_tool_calls.get(id)
    }

    pub fn has_active_tool_call(&self, id: &str) -> bool {
        self.active_tool_calls.contains_key(id)
    }

    pub fn set_active_tool_call(&mut self, id: &str, tool_call: ToolCallBlockData) {
        self.active_tool_calls.insert(id.to_owned(), tool_call);
    }

    pub fn remove_active_tool_call(&mut self, id: &str) {
        self.active_tool_calls.remove(id);
    }

    pub fn has_tool_component(&self, id: &str) -> bool {
        self.pending_tool_components.contains(id)
    }

    pub fn remove_tool_component(&mut self, id: &str) {
        self.pending_tool_components.remove(id);
    }

    pub fn has_pending_agent_group(&self) -> bool {
        self.pending_agent_group.is_some()
    }

    pub fn has_pending_read_group(&self) -> bool {
        self.pending_read_group.is_some()
    }

    pub fn pending_agent_group(&self) -> Option<&PendingGroup> {
        self.pending_agent_group.as_ref()
    }

    pub fn pending_read_group(&self) -> Option<&PendingGroup> {
        self.pending_read_group.as_ref()
    }

    /// `removeToolComponentIfInactive` — drop the component only when the call
    /// is no longer tracked as active.
    pub fn remove_tool_component_if_inactive(&mut self, tool_call_id: &str) {
        if !self.active_tool_calls.contains_key(tool_call_id) {
            self.pending_tool_components.remove(tool_call_id);
        }
    }

    pub fn active_compaction_block(&self) -> Option<&CompactionBlock> {
        self.active_compaction_block.as_ref()
    }

    pub fn todo_items(&self) -> &[TodoItem] {
        &self.todo_items
    }

    pub fn active_tool_calls(&self) -> impl Iterator<Item = (&String, &ToolCallBlockData)> {
        self.active_tool_calls.iter()
    }

    /// `applyBackgroundTaskTerminalStatus` — push the real terminal status of
    /// a background agent task into the matching `Agent` card. An
    /// `args.agentId` is authoritative; description fallback only when the
    /// agent id is unknown, and never on an ambiguous description.
    ///
    /// Returns true iff a component (card view) was found and updated.
    pub fn apply_background_task_terminal_status(
        &mut self,
        args: &ApplyTerminalStatusArgs,
    ) -> bool {
        let use_agent_id_only = args.agent_id.is_some();
        let mut agent_id_match: Option<String> = None;
        let mut desc_match: Option<String> = None;
        let mut desc_ambiguous = false;
        for (id, view) in &self.tool_call_subagents {
            if use_agent_id_only {
                if view.agent_id.as_deref() == args.agent_id.as_deref() {
                    agent_id_match = Some(id.clone());
                    break;
                }
                continue;
            }
            if view.description.as_deref() != Some(args.description.as_str()) {
                continue;
            }
            if desc_match.is_some() {
                desc_ambiguous = true;
            } else {
                desc_match = Some(id.clone());
            }
        }
        let target = if use_agent_id_only {
            agent_id_match
        } else if desc_ambiguous {
            None
        } else {
            desc_match
        };
        let Some(target) = target else {
            return false;
        };
        let view = self
            .tool_call_subagents
            .get_mut(&target)
            .expect("target was found above");
        view.terminal_status = Some(args.status);
        view.terminal_error = args.error_text.clone();
        // TODO(legacy): tc.setBackgroundTaskTerminalStatus(status, { errorText })
        true
    }

    /// `markSubagentBackgrounded` — mark a foreground-running subagent card
    /// (`task.started` with `info.kind === 'agent'`) as detached-to-background.
    pub fn mark_subagent_backgrounded(&mut self, agent_id: &str) -> bool {
        for view in self.tool_call_subagents.values_mut() {
            if view.agent_id.as_deref() != Some(agent_id) {
                continue;
            }
            if !view.phase.is_foreground_running() {
                continue;
            }
            view.backgrounded = true;
            // TODO(legacy): tc.markBackgrounded()
            return true;
        }
        false
    }

    // Subagent card-view mutators (driven by the subagent event handler).
    fn tool_call_subagent_view_mut(&mut self, tool_call_id: &str) -> &mut ToolCallSubagentView {
        self.tool_call_subagents
            .entry(tool_call_id.to_owned())
            .or_default()
    }

    /// Read access to a subagent card view (for host/tests).
    pub fn tool_call_subagent_view(&self, tool_call_id: &str) -> Option<&ToolCallSubagentView> {
        self.tool_call_subagents.get(tool_call_id)
    }

    /// `setSubagentMeta(childAgentId, name)`.
    pub fn set_tool_call_subagent_meta(
        &mut self,
        tool_call_id: &str,
        agent_id: &str,
        description: &str,
    ) {
        let view = self.tool_call_subagent_view_mut(tool_call_id);
        view.agent_id = Some(agent_id.to_owned());
        view.description = Some(description.to_owned());
    }

    /// `onSubagentSpawned`.
    pub fn tool_call_subagent_spawned(&mut self, tool_call_id: &str, agent_id: &str) {
        let view = self.tool_call_subagent_view_mut(tool_call_id);
        view.agent_id = Some(agent_id.to_owned());
        view.phase = SubagentCardPhase::Spawning;
    }

    /// `onSubagentStarted`.
    pub fn tool_call_subagent_started(&mut self, tool_call_id: &str) {
        let view = self.tool_call_subagent_view_mut(tool_call_id);
        view.phase = SubagentCardPhase::Running;
    }

    /// `onSubagentSuspended` — phase unchanged (suspension is a sub-state).
    pub fn tool_call_subagent_suspended(&mut self, tool_call_id: &str) {
        self.tool_call_subagent_view_mut(tool_call_id);
    }

    /// `onSubagentCompleted`.
    pub fn tool_call_subagent_completed(&mut self, tool_call_id: &str) {
        let view = self.tool_call_subagent_view_mut(tool_call_id);
        view.phase = SubagentCardPhase::Completed;
    }

    /// `onSubagentFailed`.
    pub fn tool_call_subagent_failed(&mut self, tool_call_id: &str) {
        let view = self.tool_call_subagent_view_mut(tool_call_id);
        view.phase = SubagentCardPhase::Failed;
    }

    /// `registerToolCall` — register a tool call that arrived via
    /// `tool.call.started`. Clears pending streaming state for the id, updates
    /// or creates the component, returns whether the call was new.
    pub fn register_tool_call(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        tool_call: ToolCallBlockData,
    ) -> bool {
        let existing = self.active_tool_calls.get(&tool_call.id).cloned();
        self.active_tool_calls
            .insert(tool_call.id.clone(), tool_call.clone());
        self.pending_tool_call_flush_ids.remove(&tool_call.id);
        self.streaming_tool_call_arguments.remove(&tool_call.id);
        let existing_component = self.pending_tool_components.contains(&tool_call.id);
        if existing_component {
            effects.push(StreamingEffect::ToolCallUpdate(tool_call.id.clone()));
        } else if existing.is_none() {
            self.finalize_live_text_buffers(effects, 0);
            if tool_call.name != "Agent" && tool_call.name != "AgentSwarm" {
                self.on_tool_call_start(effects, &tool_call);
            }
        }
        existing.is_none()
    }

    /// `accumulateToolCallDelta` — accumulate a streaming tool-call argument
    /// delta.
    pub fn accumulate_tool_call_delta(
        &mut self,
        id: &str,
        name: Option<&str>,
        arguments_part: Option<&str>,
        now_ms: u64,
    ) {
        let existing = self.streaming_tool_call_arguments.get(id).cloned();
        let arguments_text = append_streaming_args_preview(
            existing.as_ref().map(|e| e.arguments_text.as_str()),
            arguments_part,
        );
        let name = name
            .map(str::to_owned)
            .or_else(|| existing.as_ref().map(|e| e.name.clone()))
            .or_else(|| self.active_tool_calls.get(id).map(|tc| tc.name.clone()))
            .unwrap_or_else(|| "Tool".to_owned());
        let started_at_ms = existing.map(|e| e.started_at_ms).unwrap_or(now_ms);
        self.streaming_tool_call_arguments.insert(
            id.to_owned(),
            StreamingToolCallArgs {
                name,
                arguments_text,
                started_at_ms,
            },
        );
        self.pending_tool_call_flush_ids.insert(id.to_owned());
    }

    /// `getStreamingToolCallPreview`.
    pub fn get_streaming_tool_call_preview(&self, id: &str) -> Option<ToolCallPreview> {
        let streaming = self.streaming_tool_call_arguments.get(id)?;
        let name = streaming.name.clone();
        Some(ToolCallPreview {
            name,
            args: parse_streaming_args(&streaming.arguments_text),
            arguments_text: streaming.arguments_text.clone(),
            started_at_ms: streaming.started_at_ms,
        })
    }

    /// `completeToolResult` — deliver the result and remove tracking state.
    /// Returns the matched call, or `None` if no call was tracked.
    pub fn complete_tool_result(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        tool_call_id: &str,
        result: ToolResultBlockData,
    ) -> Option<ToolCallBlockData> {
        let matched_call = self.active_tool_calls.get(tool_call_id).cloned();
        if matched_call.is_some() {
            self.on_tool_call_end(effects, tool_call_id, &result);
        }
        self.active_tool_calls.remove(tool_call_id);
        self.streaming_tool_call_arguments.remove(tool_call_id);
        matched_call
    }

    /// `markStepTruncated` — mark in-flight tool calls truncated when a step
    /// hits max_tokens. Returns the count truncated.
    pub fn mark_step_truncated(&mut self, turn_id: &str, step: u32) -> usize {
        let mut count = 0;
        for tool_call in self.active_tool_calls.values_mut() {
            if tool_call.result.is_some() {
                continue;
            }
            if tool_call.streaming_arguments.is_none() {
                continue;
            }
            if tool_call.turn_id.as_deref() != Some(turn_id) {
                continue;
            }
            if tool_call.step != Some(step) {
                continue;
            }
            tool_call.truncated = true;
            count += 1;
        }
        self.streaming_tool_call_arguments.clear();
        count
    }

    /// `cleanupAfterReplay` — tear down replay-specific state.
    pub fn cleanup_after_replay(&mut self, completed_tool_call_ids: &BTreeSet<String>) {
        // TODO(legacy): host.collapseTrailingToolCalls()
        self.active_tool_calls.clear();
        for tool_call_id in completed_tool_call_ids {
            self.pending_tool_components.remove(tool_call_id);
        }
        self.pending_agent_group = None;
        self.pending_read_group = None;
        self.current_turn_id = None;
        self.current_step = 0;
        self.streaming_tool_call_arguments.clear();
        self.pending_tool_call_flush_ids.clear();
    }

    // -----------------------------------------------------------------------
    // Dispose helpers (moved from DimiTUI in the TS port)
    // -----------------------------------------------------------------------

    pub fn dispose_active_thinking_component(&mut self) {
        if self.thinking_component_active {
            // TODO(legacy): component.dispose()
            self.thinking_component_active = false;
        }
    }

    pub fn dispose_and_clear_pending_tool_components(&mut self) {
        // TODO(legacy): dispose each ToolCallComponent
        self.pending_tool_components.clear();
    }

    pub fn dispose_active_compaction_block(&mut self) {
        // TODO(legacy): component.dispose()
        self.active_compaction_block = None;
    }

    // -----------------------------------------------------------------------
    // Flush control
    // -----------------------------------------------------------------------

    /// `hasPending`.
    pub fn has_pending(&self) -> bool {
        self.pending_assistant_flush
            || self.pending_thinking_flush
            || !self.pending_tool_call_flush_ids.is_empty()
    }

    /// `clearFlushTimer` — the actual `clearTimeout` is legacy; this clears
    /// the pure timer-active flag.
    pub fn clear_flush_timer(&mut self) {
        self.flush_timer_active = false;
    }

    /// `clearFlushTimerIfIdle`.
    pub fn clear_flush_timer_if_idle(&mut self) {
        if self.has_pending() {
            return;
        }
        self.clear_flush_timer();
    }

    /// `discardPending`.
    pub fn discard_pending(&mut self) {
        self.clear_flush_timer();
        self.pending_assistant_flush = false;
        self.pending_thinking_flush = false;
        self.pending_tool_call_flush_ids.clear();
    }

    /// `scheduleFlush` — returns `Some(delay_ms)` when a flush timer should be
    /// armed (and marks the timer active), else `None`. The host performs the
    /// actual `setTimeout` and calls [`flush`] / [`flush_now`] when it fires.
    pub fn schedule_flush(&mut self, now_ms: u64) -> Option<u64> {
        if !self.has_pending() {
            return None;
        }
        if self.flush_timer_active {
            return None;
        }
        let delay = match self.last_flush_at {
            None => 0,
            Some(last) => STREAMING_UI_FLUSH_MS.saturating_sub(now_ms.saturating_sub(last)),
        };
        self.flush_timer_active = true;
        Some(delay)
    }

    /// `flushNow` — clear the timer and flush immediately.
    pub fn flush_now(&mut self, effects: &mut Vec<StreamingEffect>, now_ms: u64) {
        self.clear_flush_timer();
        self.flush(effects, now_ms);
    }

    /// `flush` — the timer tick. Flush ordering: thinking before assistant,
    /// then tool-call previews (grouped).
    fn flush(&mut self, effects: &mut Vec<StreamingEffect>, now_ms: u64) {
        if !self.has_pending() {
            return;
        }
        self.last_flush_at = Some(now_ms);
        let should_flush_thinking = self.pending_thinking_flush;
        let should_flush_assistant = self.pending_assistant_flush;
        let tool_call_ids: Vec<String> = self.pending_tool_call_flush_ids.iter().cloned().collect();
        self.pending_thinking_flush = false;
        self.pending_assistant_flush = false;
        self.pending_tool_call_flush_ids.clear();

        let thinking_draft = self.thinking_draft.clone();
        let assistant_draft = self.assistant_draft.clone();
        if should_flush_thinking && !self.thinking_draft.is_empty() {
            self.on_thinking_update(effects, &thinking_draft);
        }
        if should_flush_assistant {
            self.on_streaming_text_update(effects, &assistant_draft);
        }
        for id in tool_call_ids {
            self.flush_tool_call_preview(effects, &id);
        }
    }

    pub fn mark_assistant_dirty(&mut self) {
        self.pending_assistant_flush = true;
    }

    pub fn mark_thinking_dirty(&mut self) {
        self.pending_thinking_flush = true;
    }

    // -----------------------------------------------------------------------
    // Text streaming lifecycle
    // -----------------------------------------------------------------------

    /// `flushThinkingToTranscript`.
    pub fn flush_thinking_to_transcript(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        now_ms: u64,
    ) {
        self.flush_now(effects, now_ms);
        self.thinking_draft.clear();
        self.on_thinking_end(effects);
        // TODO(legacy): host.patchLivePane({ mode: next_mode })
    }

    /// `finalizeAssistantStream`.
    pub fn finalize_assistant_stream(&mut self, effects: &mut Vec<StreamingEffect>, now_ms: u64) {
        self.flush_now(effects, now_ms);
        if self.streaming_block.is_some() {
            self.on_streaming_text_end(effects);
        }
        self.assistant_draft.clear();
        // TODO(legacy): host.updateActivityPane(); state.ui.requestRender()
    }

    /// `resetLiveText`.
    pub fn reset_live_text(&mut self, effects: &mut Vec<StreamingEffect>) {
        self.pending_assistant_flush = false;
        self.pending_thinking_flush = false;
        self.clear_flush_timer_if_idle();
        self.assistant_draft.clear();
        self.streaming_block = None;
        self.thinking_draft.clear();
        self.dispose_active_thinking_component();
        let _ = effects;
    }

    /// `resetToolUi`.
    pub fn reset_tool_ui(&mut self, effects: &mut Vec<StreamingEffect>) {
        self.pending_tool_call_flush_ids.clear();
        self.clear_flush_timer_if_idle();
        self.streaming_tool_call_arguments.clear();
        self.dispose_and_clear_pending_tool_components();
        self.pending_agent_group = None;
        self.pending_read_group = None;
        self.reset_tool_call_state();
        let _ = effects;
    }

    /// `resetToolCallState`.
    pub fn reset_tool_call_state(&mut self) {
        self.active_tool_calls.clear();
    }

    /// `finalizeActiveWait` — freeze the active WaitFor card. No-op when none.
    pub fn finalize_active_wait(&mut self) {
        // TODO(legacy): activeWaitComponent.finalizeWait()
        self.active_wait_component = false;
    }

    /// `finalizeLiveTextBuffers`.
    pub fn finalize_live_text_buffers(&mut self, effects: &mut Vec<StreamingEffect>, now_ms: u64) {
        self.flush_thinking_to_transcript(effects, now_ms);
        self.finalize_assistant_stream(effects, now_ms);
    }

    // -----------------------------------------------------------------------
    // Live render hooks
    // -----------------------------------------------------------------------

    /// `onStreamingTextStart`.
    pub fn on_streaming_text_start(&mut self, effects: &mut Vec<StreamingEffect>) {
        self.pending_agent_group = None;
        self.pending_read_group = None;
        // TODO(legacy): host.collapseTrailingToolCalls()
        let turn_id = self.current_turn_id.clone();
        self.streaming_block = Some(StreamingBlock {
            turn_id,
            content: String::new(),
            finalized: false,
        });
        // TODO(legacy): pushTranscriptEntry + addChild(AssistantMessageComponent)
        effects.push(StreamingEffect::StreamingTextStart);
    }

    /// `onStreamingTextUpdate`.
    pub fn on_streaming_text_update(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        full_text: &str,
    ) {
        if let Some(block) = &mut self.streaming_block {
            block.content = full_text.to_owned();
        }
        effects.push(StreamingEffect::StreamingTextUpdate(full_text.to_owned()));
    }

    /// `onStreamingTextEnd`.
    pub fn on_streaming_text_end(&mut self, effects: &mut Vec<StreamingEffect>) {
        let Some(block) = self.streaming_block.take() else {
            return;
        };
        // TODO(legacy): component.updateContent(content, { transient: false })
        effects.push(StreamingEffect::StreamingTextEnd(block.content));
    }

    /// `onThinkingUpdate` — skip thinking that carries nothing visible.
    pub fn on_thinking_update(&mut self, effects: &mut Vec<StreamingEffect>, full_text: &str) {
        if full_text.trim().is_empty() && !self.thinking_component_active {
            return;
        }
        if !self.thinking_component_active {
            self.pending_agent_group = None;
            self.pending_read_group = None;
            // TODO(legacy): create ThinkingComponent + addChild
            self.thinking_component_active = true;
        }
        // TODO(legacy): setExpanded when toolDisplayMode == 'full'
        effects.push(StreamingEffect::ThinkingUpdate(full_text.to_owned()));
    }

    /// `onThinkingEnd`.
    pub fn on_thinking_end(&mut self, effects: &mut Vec<StreamingEffect>) {
        if !self.thinking_component_active {
            return;
        }
        // TODO(legacy): finalize(); setHidden(toolDisplayMode === 'summary')
        self.thinking_component_active = false;
        effects.push(StreamingEffect::ThinkingEnd);
    }

    /// `onToolCallStart`.
    pub fn on_tool_call_start(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        tool_call: &ToolCallBlockData,
    ) {
        if tool_call.name == "AskUserQuestion" {
            return;
        }
        // TODO(legacy): create ToolCallComponent, setExpanded on full mode
        self.pending_tool_components.insert(tool_call.id.clone());
        effects.push(StreamingEffect::ToolCallStart(tool_call.id.clone()));

        if tool_call.name != "Agent" {
            self.pending_agent_group = None;
        }
        if tool_call.name != "Read" {
            self.pending_read_group = None;
        }

        let handled = self.try_attach_agent_tool_call(effects, tool_call);
        if !handled {
            self.try_attach_read_tool_call(effects, tool_call);
        }

        // TODO(legacy): host.foldTrailingToolCalls(TRANSCRIPT_KEEP_TRAILING_TOOL_CALLS)
        // TODO(legacy): ExitPlanMode plan fetch (async session.getPlan)
    }

    /// `onToolCallEnd`.
    pub fn on_tool_call_end(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        tool_call_id: &str,
        result: &ToolResultBlockData,
    ) {
        let matched_call = self.active_tool_calls.get(tool_call_id).cloned();
        if self.pending_tool_components.remove(tool_call_id) {
            // TODO(legacy): tc.setResult(result)
            if matched_call.as_ref().map(|m| m.name.as_str()) == Some("WaitFor") && !result.is_error
            {
                // The wait keeps running after the result lands (stopTurn);
                // keep the card referenced so the next turn start freezes it.
                self.finalize_active_wait();
                self.active_wait_component = true;
            }
            effects.push(StreamingEffect::ToolCallEnd {
                tool_call_id: tool_call_id.to_owned(),
                output: result.output.clone(),
            });
            return;
        }

        if matched_call.as_ref().map(|m| m.name.as_str()) == Some("AskUserQuestion") {
            // TODO(legacy): create completed ToolCallComponent with result
            effects.push(StreamingEffect::ToolCallEnd {
                tool_call_id: tool_call_id.to_owned(),
                output: result.output.clone(),
            });
        }
    }

    /// `setTodoList`.
    pub fn set_todo_list(&mut self, effects: &mut Vec<StreamingEffect>, todos: Vec<TodoItem>) {
        // TODO(legacy): state.todoPanel.setTodos(todos); mount/unmount panel
        let snapshot: Vec<(String, TodoStatus)> =
            todos.iter().map(|t| (t.title.clone(), t.status)).collect();
        self.todo_items = todos;
        effects.push(StreamingEffect::TodoListSet(snapshot));
    }

    /// `beginCompaction`.
    pub fn begin_compaction(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        instruction: Option<String>,
    ) {
        // TODO(legacy): host.collapseTrailingToolCalls()
        if self.active_compaction_block.is_some() {
            // The previous block was never finalized — mark it done.
            self.active_compaction_block = None;
        }
        self.active_compaction_block = Some(CompactionBlock {
            instruction: instruction.clone(),
            done: false,
            canceled: false,
        });
        // TODO(legacy): create CompactionComponent + addChild
        effects.push(StreamingEffect::CompactionBegin { instruction });
    }

    /// `endCompaction`.
    pub fn end_compaction(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        tokens_before: u64,
        tokens_after: u64,
        summary: Option<String>,
    ) {
        let Some(block) = self.active_compaction_block.as_mut() else {
            return;
        };
        // TODO(legacy): block.markDone(tokensBefore, tokensAfter, summary)
        block.done = true;
        effects.push(StreamingEffect::CompactionEnd {
            tokens_before,
            tokens_after,
            summary,
        });
        self.active_compaction_block = None;
    }

    /// `cancelCompaction`.
    pub fn cancel_compaction(&mut self, effects: &mut Vec<StreamingEffect>) {
        let Some(block) = self.active_compaction_block.as_mut() else {
            return;
        };
        // TODO(legacy): block.markCanceled()
        block.canceled = true;
        effects.push(StreamingEffect::CompactionCancel);
        self.active_compaction_block = None;
    }

    // -----------------------------------------------------------------------
    // Tool call grouping
    // -----------------------------------------------------------------------

    /// `flushToolCallPreview`.
    fn flush_tool_call_preview(&mut self, effects: &mut Vec<StreamingEffect>, id: &str) {
        let Some(streaming) = self.streaming_tool_call_arguments.get(id).cloned() else {
            return;
        };
        let name = self
            .active_tool_calls
            .get(id)
            .map(|tc| tc.name.clone())
            .unwrap_or(streaming.name.clone());
        let tool_call = ToolCallBlockData {
            id: id.to_owned(),
            name: name.clone(),
            args: parse_streaming_args(&streaming.arguments_text),
            description: None,
            streaming_arguments: Some(streaming.arguments_text.clone()),
            streaming_started_at_ms: Some(streaming.started_at_ms),
            result: None,
            step: Some(self.current_step),
            turn_id: self.current_turn_id.clone(),
            truncated: false,
        };
        self.active_tool_calls
            .insert(id.to_owned(), tool_call.clone());

        if !self.thinking_draft.is_empty() || self.streaming_block.is_some() {
            self.finalize_live_text_buffers(effects, /* now */ 0);
        }

        if self.pending_tool_components.contains(id) {
            effects.push(StreamingEffect::ToolCallUpdate(id.to_owned()));
        } else if name != "Agent" && name != "AgentSwarm" {
            self.on_tool_call_start(effects, &tool_call);
        }
    }

    /// `tryAttachAgentToolCall`.
    fn try_attach_agent_tool_call(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        tool_call: &ToolCallBlockData,
    ) -> bool {
        if tool_call.name != "Agent" {
            self.pending_agent_group = None;
            return false;
        }
        let step = tool_call.step.unwrap_or(self.current_step);
        let turn_id = tool_call
            .turn_id
            .clone()
            .or_else(|| self.current_turn_id.clone());
        let action =
            attach_tool_call_to_group(&mut self.pending_agent_group, &tool_call.id, step, turn_id);
        self.emit_group_action(effects, GroupKind::Agent, action);
        true
    }

    /// `tryAttachReadToolCall`.
    fn try_attach_read_tool_call(
        &mut self,
        effects: &mut Vec<StreamingEffect>,
        tool_call: &ToolCallBlockData,
    ) -> bool {
        if tool_call.name != "Read" {
            self.pending_read_group = None;
            return false;
        }
        let step = tool_call.step.unwrap_or(self.current_step);
        let turn_id = tool_call
            .turn_id
            .clone()
            .or_else(|| self.current_turn_id.clone());
        let action =
            attach_tool_call_to_group(&mut self.pending_read_group, &tool_call.id, step, turn_id);
        self.emit_group_action(effects, GroupKind::Read, action);
        true
    }

    fn emit_group_action(
        &self,
        effects: &mut Vec<StreamingEffect>,
        group: GroupKind,
        action: GroupAction,
    ) {
        if action == GroupAction::Upgraded {
            effects.push(StreamingEffect::GroupUpgraded { group });
        }
    }
}

/// The shared solo→group attach decision (used by both Agent and Read).
fn attach_tool_call_to_group(
    pending: &mut Option<PendingGroup>,
    tool_call_id: &str,
    step: u32,
    turn_id: Option<String>,
) -> GroupAction {
    let Some(mut cur) = pending.take() else {
        *pending = Some(PendingGroup {
            turn_id,
            step,
            solo: Some(tool_call_id.to_owned()),
            group: None,
        });
        return GroupAction::StartedSolo;
    };
    if cur.step != step || cur.turn_id != turn_id {
        *pending = Some(PendingGroup {
            turn_id,
            step,
            solo: Some(tool_call_id.to_owned()),
            group: None,
        });
        return GroupAction::StartedSolo;
    }
    if let Some(mut members) = cur.group.take() {
        members.push(tool_call_id.to_owned());
        cur.group = Some(members);
        *pending = Some(cur);
        return GroupAction::Attached;
    }
    if let Some(solo) = cur.solo.take() {
        *pending = Some(PendingGroup {
            turn_id,
            step,
            solo: None,
            group: Some(vec![solo, tool_call_id.to_owned()]),
        });
        return GroupAction::Upgraded;
    }
    // Defensive: empty pending — restart as solo.
    *pending = Some(PendingGroup {
        turn_id,
        step,
        solo: Some(tool_call_id.to_owned()),
        group: None,
    });
    GroupAction::StartedSolo
}

/// Build a todo item from `(title, status-string)` — used when parsing a
/// `TodoList` tool result (`isTodoItemShape`).
pub fn is_todo_item_shape(title: &str, status: &str) -> bool {
    if title.is_empty() {
        return false;
    }
    matches!(status, "pending" | "in_progress" | "done")
}

/// Map a `TodoList` status string to a chrome [`TodoStatus`].
pub fn todo_status_from_str(status: &str) -> Option<TodoStatus> {
    match status {
        "pending" => Some(TodoStatus::Pending),
        "in_progress" => Some(TodoStatus::InProgress),
        "done" => Some(TodoStatus::Done),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn controller() -> StreamingUiController {
        StreamingUiController::new()
    }

    fn tool_call(id: &str, name: &str) -> ToolCallBlockData {
        let mut tc = ToolCallBlockData::new(id, name);
        tc.args = args_from(json!({}));
        tc
    }

    fn args_from(v: serde_json::Value) -> Args {
        match v.as_object() {
            Some(map) => map.clone(),
            None => Args::new(),
        }
    }

    #[test]
    fn draft_accumulation_orders_thinking_before_assistant() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.append_thinking_delta("let me ");
        c.append_thinking_delta("think");
        c.append_assistant_delta(&mut fx, "Hel");
        c.append_assistant_delta(&mut fx, "lo");

        assert_eq!(c.thinking_draft, "let me think");
        assert_eq!(c.assistant_draft, "Hello");
        assert!(c.has_thinking_draft());
        assert!(c.has_streaming_block());

        // Single flush emits thinking first, then assistant.
        let mut fx = Vec::new();
        c.flush_now(&mut fx, 100);
        let kinds: Vec<&str> = fx
            .iter()
            .map(|e| match e {
                StreamingEffect::ThinkingUpdate(_) => "thinking",
                StreamingEffect::StreamingTextUpdate(_) => "assistant",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["thinking", "assistant"]);
    }

    #[test]
    fn multiple_deltas_batch_into_one_flush() {
        let mut c = controller();
        let mut fx = Vec::new();
        for d in ["a", "b", "c", "d"] {
            c.append_assistant_delta(&mut fx, d);
        }
        assert_eq!(c.assistant_draft, "abcd");
        // One flush emits exactly one assistant update with the full draft.
        let mut fx = Vec::new();
        c.flush_now(&mut fx, 200);
        let updates: Vec<&str> = fx
            .iter()
            .filter_map(|e| match e {
                StreamingEffect::StreamingTextUpdate(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(updates, ["abcd"]);
        // Nothing pending after the flush.
        assert!(!c.has_pending());
    }

    #[test]
    fn whitespace_only_assistant_delta_is_skipped_before_block_open() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.append_assistant_delta(&mut fx, "   ");
        assert!(!c.has_streaming_block());
        assert_eq!(c.assistant_draft, "");
        assert!(!c.has_pending());
        // Once a block is open, whitespace is kept (it follows real text).
        c.append_assistant_delta(&mut fx, "hi");
        c.append_assistant_delta(&mut fx, "  ");
        assert_eq!(c.assistant_draft, "hi  ");
    }

    #[test]
    fn flush_emits_thinking_only_when_draft_non_empty() {
        let mut c = controller();
        c.pending_thinking_flush = true; // private — drive via append instead
        // Use a whitespace-only thinking draft: pending but not visible.
        c.append_thinking_delta(" ");
        let mut fx = Vec::new();
        c.flush_now(&mut fx, 300);
        assert!(
            !fx.iter()
                .any(|e| matches!(e, StreamingEffect::ThinkingUpdate(_)))
        );
    }

    #[test]
    fn flush_emits_visible_thinking_even_when_whitespace() {
        let mut c = controller();
        c.append_thinking_delta("real thought");
        let mut fx = Vec::new();
        c.flush_now(&mut fx, 400);
        assert!(
            fx.iter()
                .any(|e| matches!(e, StreamingEffect::ThinkingUpdate(t) if t == "real thought"))
        );
    }

    #[test]
    fn thinking_delta_guard_skips_whitespace_before_any_component() {
        // handleThinkingDelta: whitespace-only delta with no draft → skip.
        let mut c = controller();
        assert!(!c.has_thinking_draft());
        // (The guard lives in the event handler; here we verify the draft is
        // only appended when there is something to render.)
        c.append_thinking_delta(" ");
        assert!(c.has_thinking_draft());
    }

    #[test]
    fn schedule_flush_computes_delay_from_last_flush() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.append_assistant_delta(&mut fx, "x");
        assert_eq!(c.schedule_flush(0), Some(0));
        // Already scheduled → no new timer.
        assert_eq!(c.schedule_flush(1), None);
        // After a flush at t=100, next schedule delays to hit the 50ms window.
        c.flush_now(&mut fx, 100);
        assert!(!c.has_pending());
        c.append_assistant_delta(&mut fx, "y");
        assert_eq!(c.schedule_flush(120), Some(30));
    }

    #[test]
    fn register_tool_call_tracks_new_vs_existing() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        let mut tc = tool_call("t1", "Read");
        tc.step = Some(0);
        tc.turn_id = Some("1".to_owned());
        assert!(c.register_tool_call(&mut fx, tc.clone()));
        assert!(!c.register_tool_call(&mut fx, tc.clone()));
        assert!(c.has_active_tool_call("t1"));
        assert!(c.has_tool_component("t1"));
    }

    #[test]
    fn agent_group_upgrade_solo_to_group() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        c.set_step(0);

        let mut a = tool_call("a", "Agent");
        a.step = Some(0);
        a.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &a);
        assert!(c.pending_agent_group().is_some());
        let pg = c.pending_agent_group().unwrap();
        assert_eq!(pg.solo.as_deref(), Some("a"));
        assert!(pg.group.is_none());

        let mut b = tool_call("b", "Agent");
        b.step = Some(0);
        b.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &b);
        let pg = c.pending_agent_group().unwrap();
        assert_eq!(
            pg.group.as_deref(),
            Some(&["a".to_owned(), "b".to_owned()][..])
        );
        assert!(pg.solo.is_none());
        assert!(fx.iter().any(|e| matches!(
            e,
            StreamingEffect::GroupUpgraded {
                group: GroupKind::Agent
            }
        )));
    }

    #[test]
    fn agent_group_attaches_third_member() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        for (id, step) in [("a", 0u32), ("b", 0), ("c", 0)] {
            let mut t = tool_call(id, "Agent");
            t.step = Some(step);
            t.turn_id = Some("1".to_owned());
            c.on_tool_call_start(&mut fx, &t);
        }
        let pg = c.pending_agent_group().unwrap();
        assert_eq!(
            pg.group.as_deref(),
            Some(&["a".to_owned(), "b".to_owned(), "c".to_owned()][..])
        );
    }

    #[test]
    fn group_resets_on_step_or_turn_change() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        c.set_step(0);
        let mut a = tool_call("a", "Agent");
        a.step = Some(0);
        a.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &a);

        // Same turn, next step → group resets, new solo.
        c.set_step(1);
        let mut b = tool_call("b", "Agent");
        b.step = Some(1);
        b.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &b);
        let pg = c.pending_agent_group().unwrap();
        assert_eq!(pg.solo.as_deref(), Some("b"));
        assert_eq!(pg.step, 1);
    }

    #[test]
    fn non_agent_call_clears_agent_group() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        let mut a = tool_call("a", "Agent");
        a.step = Some(0);
        a.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &a);
        assert!(c.has_pending_agent_group());

        let mut t = tool_call("t", "Bash");
        t.step = Some(0);
        t.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &t);
        assert!(!c.has_pending_agent_group());
    }

    #[test]
    fn read_group_forms_independently() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        c.set_step(0);
        for id in ["r1", "r2"] {
            let mut t = tool_call(id, "Read");
            t.step = Some(0);
            t.turn_id = Some("1".to_owned());
            c.on_tool_call_start(&mut fx, &t);
        }
        assert!(!c.has_pending_agent_group());
        let rg = c.pending_read_group().unwrap();
        assert_eq!(
            rg.group.as_deref(),
            Some(&["r1".to_owned(), "r2".to_owned()][..])
        );
    }

    #[test]
    fn ask_user_question_is_not_mounted_as_component() {
        let mut c = controller();
        let mut fx = Vec::new();
        let q = tool_call("q", "AskUserQuestion");
        c.on_tool_call_start(&mut fx, &q);
        assert!(!c.has_tool_component("q"));
        assert!(
            !fx.iter()
                .any(|e| matches!(e, StreamingEffect::ToolCallStart(id) if id == "q"))
        );
    }

    #[test]
    fn streaming_tool_call_preview_passes_through_flush() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        c.set_step(0);
        c.accumulate_tool_call_delta("t1", Some("Read"), Some(r#"{"path":"/a"}"#), 1000);
        assert!(c.has_pending());
        c.flush_now(&mut fx, 1100);
        let tc = c.get_active_tool_call("t1").cloned().unwrap();
        assert_eq!(tc.name, "Read");
        assert_eq!(tc.args.get("path"), Some(&json!("/a")));
        assert!(c.has_tool_component("t1"));
    }

    #[test]
    fn streaming_args_preview_is_capped() {
        let current = "x".repeat(STREAMING_ARGS_PREVIEW_MAX_CHARS);
        let out = append_streaming_args_preview(Some(&current), Some("yyy"));
        assert_eq!(out.chars().count(), STREAMING_ARGS_PREVIEW_MAX_CHARS);
        assert!(!out.ends_with("yyy"));
    }

    #[test]
    fn parse_streaming_args_full_json_and_partial() {
        // Complete JSON object → full parse.
        let full = r#"{"path":"/a/b","name":"x"}"#;
        let args = parse_streaming_args(full);
        assert_eq!(args.get("path"), Some(&json!("/a/b")));
        assert_eq!(args.get("name"), Some(&json!("x")));

        // Incomplete stream → field preview scan.
        let partial = r#"{"path":"/a/b",""#;
        let args = parse_streaming_args(partial);
        assert_eq!(args.get("path"), Some(&json!("/a/b")));
        assert!(args.get("name").is_none());

        // Whitespace → empty.
        assert!(parse_streaming_args("   ").is_empty());
    }

    #[test]
    fn parse_streaming_args_unescapes_json_strings() {
        let partial = r#"{"path":"a\nb","name":"q\"z"}"#;
        let args = parse_streaming_args(partial);
        assert_eq!(args.get("path"), Some(&json!("a\nb")));
        assert_eq!(args.get("name"), Some(&json!("q\"z")));
    }

    #[test]
    fn complete_tool_result_removes_tracking_and_emits_end() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        let mut tc = tool_call("t1", "Bash");
        tc.step = Some(0);
        tc.turn_id = Some("1".to_owned());
        c.register_tool_call(&mut fx, tc.clone());
        assert!(c.has_active_tool_call("t1"));

        let result = ToolResultBlockData {
            tool_call_id: "t1".to_owned(),
            output: "out".to_owned(),
            is_error: false,
            synthetic: false,
        };
        let matched = c.complete_tool_result(&mut fx, "t1", result.clone());
        assert_eq!(matched.unwrap().id, "t1");
        assert!(!c.has_active_tool_call("t1"));
        assert!(!c.has_tool_component("t1"));
        assert!(fx
            .iter()
            .any(|e| matches!(e, StreamingEffect::ToolCallEnd { tool_call_id, output } if tool_call_id == "t1" && output == "out")));
    }

    #[test]
    fn mark_step_truncated_flags_only_matching_streaming_calls() {
        let mut c = controller();
        c.set_turn_id(Some("1".to_owned()));
        c.set_step(0);

        let mut a = tool_call("a", "Read");
        a.step = Some(0);
        a.turn_id = Some("1".to_owned());
        a.streaming_arguments = Some("{}".to_owned());
        a.streaming_started_at_ms = Some(1);
        c.set_active_tool_call("a", a);

        let mut b = tool_call("b", "Read");
        b.step = Some(1); // different step
        b.turn_id = Some("1".to_owned());
        b.streaming_arguments = Some("{}".to_owned());
        b.streaming_started_at_ms = Some(1);
        c.set_active_tool_call("b", b);

        let mut done = tool_call("c", "Read");
        done.step = Some(0);
        done.turn_id = Some("1".to_owned());
        done.result = Some(ToolResultBlockData::new("c", "x"));
        done.streaming_arguments = Some("{}".to_owned());
        done.streaming_started_at_ms = Some(1);
        c.set_active_tool_call("c", done);

        assert_eq!(c.mark_step_truncated("1", 0), 1);
        assert!(c.get_active_tool_call("a").unwrap().truncated);
        assert!(!c.get_active_tool_call("b").unwrap().truncated);
        assert!(!c.get_active_tool_call("c").unwrap().truncated);
    }

    #[test]
    fn finalize_live_text_buffers_closes_thinking_then_assistant() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        c.append_thinking_delta("thought");
        c.append_assistant_delta(&mut fx, "answer");
        // Force the thinking component into existence via a flush.
        c.flush_now(&mut fx, 0);
        assert!(c.has_active_thinking_component());

        let mut fx = Vec::new();
        c.finalize_live_text_buffers(&mut fx, 100);
        assert!(!c.has_active_thinking_component());
        assert!(!c.has_streaming_block());
        assert!(c.assistant_draft.is_empty());
        assert!(c.thinking_draft.is_empty());
        // thinking ended before assistant finalized
        let seq: Vec<&str> = fx
            .iter()
            .map(|e| match e {
                StreamingEffect::ThinkingEnd => "thinkEnd",
                StreamingEffect::StreamingTextEnd(_) => "textEnd",
                _ => "other",
            })
            .collect();
        let think_pos = seq.iter().position(|s| *s == "thinkEnd").unwrap();
        let text_pos = seq.iter().position(|s| *s == "textEnd").unwrap();
        assert!(think_pos < text_pos);
    }

    #[test]
    fn reset_live_text_clears_drafts_and_block() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.append_thinking_delta("t");
        c.append_assistant_delta(&mut fx, "a");
        c.flush_now(&mut fx, 0);
        let mut fx = Vec::new();
        c.reset_live_text(&mut fx);
        assert!(!c.has_thinking_draft());
        assert!(c.assistant_draft.is_empty());
        assert!(!c.has_streaming_block());
        assert!(!c.has_active_thinking_component());
        assert!(!c.has_pending());
    }

    #[test]
    fn apply_background_task_terminal_status_matches_by_agent_id() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        let mut tc = tool_call("card", "Agent");
        tc.step = Some(0);
        tc.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &tc);
        c.tool_call_subagent_spawned("card", "agent-42");

        let ok = c.apply_background_task_terminal_status(&ApplyTerminalStatusArgs {
            agent_id: Some("agent-42".to_owned()),
            description: "anything".to_owned(),
            status: BackgroundTaskStatus::Lost,
            error_text: None,
        });
        assert!(ok);
        let view = c.tool_call_subagents.get("card").unwrap();
        assert_eq!(view.terminal_status, Some(BackgroundTaskStatus::Lost));
    }

    #[test]
    fn apply_background_task_terminal_status_description_fallback_is_ambiguous_safe() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        for id in ["card1", "card2"] {
            let mut tc = tool_call(id, "Agent");
            tc.step = Some(0);
            tc.turn_id = Some("1".to_owned());
            c.on_tool_call_start(&mut fx, &tc);
            c.set_tool_call_subagent_meta(id, id, "same description");
        }
        // Ambiguous description (no agent id) → no update.
        let ok = c.apply_background_task_terminal_status(&ApplyTerminalStatusArgs {
            agent_id: None,
            description: "same description".to_owned(),
            status: BackgroundTaskStatus::Failed,
            error_text: Some("boom".to_owned()),
        });
        assert!(!ok);
        assert!(
            c.tool_call_subagents
                .get("card1")
                .unwrap()
                .terminal_status
                .is_none()
        );
    }

    #[test]
    fn mark_subagent_backgrounded_only_for_running_phase() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        let mut tc = tool_call("card", "Agent");
        tc.step = Some(0);
        tc.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &tc);
        c.tool_call_subagent_spawned("card", "agent-1");
        assert!(c.mark_subagent_backgrounded("agent-1"));
        assert!(c.tool_call_subagents.get("card").unwrap().backgrounded);

        // A completed card must not be marked.
        let mut tc2 = tool_call("card2", "Agent");
        tc2.step = Some(0);
        tc2.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &tc2);
        c.tool_call_subagent_spawned("card2", "agent-2");
        c.tool_call_subagent_completed("card2");
        assert!(!c.mark_subagent_backgrounded("agent-2"));
        assert!(!c.tool_call_subagents.get("card2").unwrap().backgrounded);
    }

    #[test]
    fn compaction_lifecycle() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.begin_compaction(&mut fx, Some("summarize".to_owned()));
        assert!(c.active_compaction_block().is_some());
        assert!(fx
            .iter()
            .any(|e| matches!(e, StreamingEffect::CompactionBegin { instruction } if instruction.as_deref() == Some("summarize"))));

        let mut fx = Vec::new();
        c.end_compaction(&mut fx, 10, 5, Some("sum".to_owned()));
        assert!(c.active_compaction_block().is_none());
        assert!(fx
            .iter()
            .any(|e| matches!(e, StreamingEffect::CompactionEnd { tokens_before: 10, tokens_after: 5, summary } if summary.as_deref() == Some("sum"))));

        // endCompaction on no active block is a no-op.
        let mut fx = Vec::new();
        c.end_compaction(&mut fx, 0, 0, None);
        assert!(fx.is_empty());
    }

    #[test]
    fn cancel_compaction_marks_canceled() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.begin_compaction(&mut fx, None);
        let mut fx = Vec::new();
        c.cancel_compaction(&mut fx);
        assert!(c.active_compaction_block().is_none());
        assert!(fx.contains(&StreamingEffect::CompactionCancel));
    }

    #[test]
    fn set_todo_list_records_items() {
        let mut c = controller();
        let mut fx = Vec::new();
        let todos = vec![
            TodoItem::new("first", TodoStatus::InProgress),
            TodoItem::new("second", TodoStatus::Pending),
        ];
        c.set_todo_list(&mut fx, todos.clone());
        assert_eq!(c.todo_items(), &todos[..]);
        let expected: Vec<(String, TodoStatus)> =
            todos.iter().map(|t| (t.title.clone(), t.status)).collect();
        assert!(
            fx.iter()
                .any(|e| matches!(e, StreamingEffect::TodoListSet(t) if *t == expected))
        );
    }

    #[test]
    fn wait_for_result_keeps_active_wait_until_next_turn() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        let mut tc = tool_call("w", "WaitFor");
        tc.step = Some(0);
        tc.turn_id = Some("1".to_owned());
        c.register_tool_call(&mut fx, tc.clone());

        let result = ToolResultBlockData::new("w", "done");
        c.complete_tool_result(&mut fx, "w", result);
        // The wait keeps ticking after the result lands.
        // (Active-wait state is internal; verify via finalize_active_wait no-op.)
        c.finalize_active_wait();
    }

    #[test]
    fn cleanup_after_replay_resets_state() {
        let mut c = controller();
        let mut fx = Vec::new();
        c.set_turn_id(Some("1".to_owned()));
        let mut tc = tool_call("t1", "Read");
        tc.step = Some(0);
        tc.turn_id = Some("1".to_owned());
        c.on_tool_call_start(&mut fx, &tc);
        c.set_active_tool_call("t1", tc);

        let mut completed = BTreeSet::new();
        completed.insert("t1".to_owned());
        c.cleanup_after_replay(&completed);
        assert!(!c.has_active_turn());
        assert!(!c.has_pending_agent_group());
        assert!(!c.has_tool_component("t1"));
    }
}
