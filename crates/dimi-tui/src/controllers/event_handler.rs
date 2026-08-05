//! Session event routing — port of `apps/dimi/src/tui/controllers/
//! session-event-handler.ts`.
//!
//! The `handle_event` routing table dispatches each event kind to a
//! `handle_xxx` method that applies pure state updates to the [`UiState`]
//! (drafts via [`StreamingUiController`], tool-call states, compaction state,
//! background tasks, transcript entries). SDK / component / async side effects
//! are annotated `// TODO(legacy): …`.
//!
//! Also carries the pure formatting helpers the TS scatters across
//! `utils/event-payload.ts`, `utils/background-task-status.ts`,
//! `utils/background-agent-status.ts`, `utils/hook-result-format.ts`, and
//! `utils/mcp-server-status.ts`.

use std::collections::{BTreeMap, BTreeSet};

use crate::chrome::{TodoItem, TodoStatus};
use crate::controllers::btw::BtwPanelController;
use crate::controllers::events::{
    AUTH_LOGIN_REQUIRED_CODE, AUTH_LOGIN_REQUIRED_STARTUP_NOTICE, CompactionCompletedData,
    CronFiredOrigin, ErrorPayload, Event, McpServerStatus, McpServerStatusSnapshot, PromptOrigin,
    TurnEndReason,
};
use crate::controllers::streaming::{
    ApplyTerminalStatusArgs, StreamingEffect, StreamingUiController,
};
use crate::controllers::subagent::SubAgentHandler;
use crate::controllers::{
    AppState, Args, BackgroundAgentMetadata, BackgroundAgentStatusData, BackgroundTaskInfo,
    BackgroundTaskKind, BackgroundTaskPhase, BackgroundTaskStatus, CronData, LivePaneMode,
    LivePaneState, MessageMode, PermissionMode, QueuedMessage, RenderMode, StreamingPhase,
    ToolCallBlockData, ToolResultBlockData, TranscriptEntry, TranscriptEntryKind,
    serialize_tool_result_output,
};
use crate::theme::ColorToken;

/// `SwarmModeEntry` — `state.swarmModeEntry` (only `'task'` is observed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmModeEntry {
    Task,
}

/// A surfaced UI message (`showError` / `showStatus` / `showNotice`).
#[derive(Debug, Clone, PartialEq)]
pub enum UiMessage {
    Error(String),
    Status {
        text: String,
        color: Option<ColorToken>,
    },
    Notice {
        title: String,
        detail: Option<String>,
    },
}

/// The UI state bag the event handler mutates. Holds the streaming
/// controller, transcript entries, app state, queue, background-task
/// bookkeeping, and the render-effect log.
#[derive(Debug, Clone)]
pub struct UiState {
    pub streaming: StreamingUiController,
    /// Render-hook effects emitted by the streaming controller during the most
    /// recent `handle_event` (component-tree mapping is legacy).
    pub effects: Vec<StreamingEffect>,
    pub app: AppState,
    pub live_pane: LivePaneState,
    pub transcript: Vec<TranscriptEntry>,
    pub queue: Vec<QueuedMessage>,
    pub messages: Vec<UiMessage>,
    /// Index of the live assistant entry in `transcript` (`_streamingBlock`).
    pub streaming_entry_index: Option<usize>,

    pub background_tasks: BTreeMap<String, BackgroundTaskInfo>,
    pub background_tasks_transcripted_terminal: BTreeSet<String>,
    /// `(bashTasks, agentTasks)` mirrored to the footer badge.
    pub background_badge: (u64, u64),

    pub rendered_skill_activation_ids: BTreeSet<String>,
    pub rendered_plugin_command_activation_ids: BTreeSet<String>,
    pub rendered_mcp_server_status_keys: BTreeMap<String, String>,
    pub mcp_servers: BTreeMap<String, McpServerStatusSnapshot>,
    pub mcp_pending_names: BTreeSet<String>,
    pub plugin_command_turns: BTreeMap<String, String>,
    pub plugin_mcp_tools_used_in_turn: BTreeSet<String>,
    pub swarm_mode_entry: Option<SwarmModeEntry>,

    pub subagent_info: BTreeMap<String, crate::controllers::SubagentInfo>,
    pub background_agent_metadata: BTreeMap<String, BackgroundAgentMetadata>,

    /// Live output appended to tool-call cards via `tool.progress`
    /// (`ToolCallComponent.appendProgress` / `appendLiveOutput`).
    pub tool_live_outputs: BTreeMap<String, Vec<String>>,

    pub defer_user_messages: bool,
    next_transcript_id: u64,
    /// Effects already consumed by `sync_streaming_transcript`.
    processed_effects: usize,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        UiState {
            streaming: StreamingUiController::new(),
            effects: Vec::new(),
            app: AppState::default(),
            live_pane: LivePaneState::default(),
            transcript: Vec::new(),
            queue: Vec::new(),
            messages: Vec::new(),
            streaming_entry_index: None,
            background_tasks: BTreeMap::new(),
            background_tasks_transcripted_terminal: BTreeSet::new(),
            background_badge: (0, 0),
            rendered_skill_activation_ids: BTreeSet::new(),
            rendered_plugin_command_activation_ids: BTreeSet::new(),
            rendered_mcp_server_status_keys: BTreeMap::new(),
            mcp_servers: BTreeMap::new(),
            mcp_pending_names: BTreeSet::new(),
            plugin_command_turns: BTreeMap::new(),
            plugin_mcp_tools_used_in_turn: BTreeSet::new(),
            swarm_mode_entry: None,
            subagent_info: BTreeMap::new(),
            background_agent_metadata: BTreeMap::new(),
            tool_live_outputs: BTreeMap::new(),
            defer_user_messages: false,
            next_transcript_id: 0,
            processed_effects: 0,
        }
    }

    /// `nextTranscriptId()` — `entry-N`.
    pub fn next_entry_id(&mut self) -> String {
        self.next_transcript_id += 1;
        format!("entry-{}", self.next_transcript_id)
    }

    pub fn shift_queued_message(&mut self) -> Option<QueuedMessage> {
        if self.queue.is_empty() {
            return None;
        }
        Some(self.queue.remove(0))
    }

    /// `showError` / `showStatus` / `showNotice` equivalent.
    pub fn push_message(&mut self, msg: UiMessage) {
        self.messages.push(msg);
    }

    pub fn show_error(&mut self, msg: impl Into<String>) {
        self.push_message(UiMessage::Error(msg.into()));
    }

    pub fn show_status(&mut self, msg: impl Into<String>, color: Option<ColorToken>) {
        self.push_message(UiMessage::Status {
            text: msg.into(),
            color,
        });
    }

    pub fn show_notice(&mut self, title: &str, detail: Option<String>) {
        self.push_message(UiMessage::Notice {
            title: title.to_owned(),
            detail,
        });
    }

    /// Map the streaming controller's live assistant block (and the
    /// `StreamingTextStart/Update/End` effects) into `transcript`. Each
    /// effect is processed exactly once (effects accumulate across events and
    /// replay records).
    pub fn sync_streaming_transcript(&mut self) {
        let effects_len = self.effects.len();
        let start = self.processed_effects.min(effects_len);
        let unprocessed: Vec<StreamingEffect> = self.effects[start..].to_vec();
        let mut idx = self.streaming_entry_index;
        for effect in &unprocessed {
            match effect {
                StreamingEffect::StreamingTextStart => {
                    if idx.is_none() {
                        let turn_id = self.streaming.get_turn_context().0.map(str::to_owned);
                        let mut entry = TranscriptEntry::new(
                            self.next_entry_id(),
                            TranscriptEntryKind::Assistant,
                            turn_id,
                            RenderMode::Markdown,
                            "",
                        );
                        entry.model_text = true;
                        idx = Some(self.transcript.len());
                        self.transcript.push(entry);
                    }
                }
                StreamingEffect::StreamingTextUpdate(text) => {
                    if let Some(i) = idx {
                        self.transcript[i].content = text.clone();
                    }
                }
                StreamingEffect::StreamingTextEnd(text) => {
                    if let Some(i) = idx {
                        self.transcript[i].content = text.clone();
                        idx = None;
                    }
                }
                _ => {}
            }
        }
        self.streaming_entry_index = idx;
        self.processed_effects = effects_len;
    }
}

/// The session event handler (port of `SessionEventHandler`).
#[derive(Debug)]
pub struct SessionEventHandler {
    pub state: UiState,
    pub subagent: SubAgentHandler,
    pub btw: BtwPanelController,
}

impl Default for SessionEventHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionEventHandler {
    pub fn new() -> Self {
        SessionEventHandler {
            state: UiState::new(),
            subagent: SubAgentHandler::new(),
            btw: BtwPanelController::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Routing table
    // -----------------------------------------------------------------------

    /// `handleEvent` — the ~50-case dispatch. Returns nothing; the effects and
    /// state changes land in [`Self::state`].
    pub fn handle_event(&mut self, event: Event, now_ms: u64) {
        // Child-agent routing first (subagent lifecycle events are handled by
        // the subagent handler below, not routed as child events).
        let routed = {
            let Self {
                state,
                subagent,
                btw,
                ..
            } = self;
            subagent.route_child_agent_event(&event, btw, state)
        };
        if routed {
            return;
        }

        if let Some(turn_id) = event.turn_id() {
            self.state.streaming.set_turn_id(Some(turn_id.to_owned()));
        }

        match event {
            Event::TurnStarted { turn_id, origin } => {
                self.handle_turn_begin(&turn_id, origin, now_ms);
            }
            Event::TurnEnded {
                turn_id,
                reason,
                error,
                ..
            } => self.handle_turn_end(&turn_id, reason, error.as_ref(), now_ms),
            Event::TurnStepStarted { turn_id, step } => {
                self.handle_step_begin(&turn_id, step, now_ms);
            }
            Event::TurnStepInterrupted {
                turn_id,
                step,
                reason,
                message,
            } => self.handle_step_interrupted(&turn_id, step, &reason, message.as_deref(), now_ms),
            Event::TurnStepCompleted {
                turn_id,
                step,
                usage,
                provider_finish_reason,
                raw_finish_reason,
                finish_reason,
            } => self.handle_step_completed(
                &turn_id,
                step,
                usage,
                provider_finish_reason.as_deref(),
                raw_finish_reason.as_deref(),
                finish_reason.as_deref(),
                now_ms,
            ),
            Event::TurnStepRetrying => {}
            Event::ToolProgress {
                tool_call_id,
                update,
                ..
            } => {
                self.handle_tool_progress(&tool_call_id, update.kind, update.text);
            }
            Event::ShellOutput { command_id, update } => {
                // TODO(legacy): host.handleShellOutput({ commandId, update })
                let _ = (command_id, update);
            }
            Event::ShellStarted {
                command_id,
                task_id,
            } => {
                // TODO(legacy): host.handleShellStarted({ commandId, taskId })
                let _ = (command_id, task_id);
            }
            Event::AssistantDelta { delta, .. } => self.handle_assistant_delta(&delta, now_ms),
            Event::ThinkingDelta { delta, .. } => self.handle_thinking_delta(&delta, now_ms),
            Event::HookResult {
                turn_id,
                hook_event,
                content,
                blocked,
                ..
            } => self.handle_hook_result(&turn_id, &hook_event, &content, blocked, now_ms),
            Event::ToolCallStarted {
                tool_call_id,
                name,
                args,
                description,
                ..
            } => self.handle_tool_call(&tool_call_id, &name, args, description, now_ms),
            Event::ToolCallDelta {
                tool_call_id,
                name,
                arguments_part,
                ..
            } => self.handle_tool_call_delta(
                &tool_call_id,
                name.as_deref(),
                arguments_part.as_deref(),
                now_ms,
            ),
            Event::ToolResult {
                tool_call_id,
                output,
                is_error,
                synthetic,
                ..
            } => self.handle_tool_result(&tool_call_id, &output, is_error, synthetic, now_ms),
            Event::AgentStatusUpdated {
                context_usage,
                context_tokens,
                max_context_tokens,
                usage,
                plan_mode,
                swarm_mode,
                permission,
                model,
                thinking_effort,
                ..
            } => self.handle_status_update(
                context_usage,
                context_tokens,
                max_context_tokens,
                usage,
                plan_mode,
                swarm_mode,
                permission,
                model.as_deref(),
                thinking_effort.as_deref(),
            ),
            Event::SessionMetaUpdated { title, patch_title } => {
                self.handle_session_meta_changed(title.as_deref(), patch_title.as_deref());
            }
            Event::SkillActivated {
                activation_id,
                skill_name,
                skill_args,
                trigger,
            } => self.handle_skill_activated(
                &activation_id,
                &skill_name,
                skill_args.as_deref(),
                trigger,
            ),
            Event::PluginCommandActivated {
                activation_id,
                plugin_id,
                command_name,
                command_args,
            } => self.handle_plugin_command_activated(
                &activation_id,
                &plugin_id,
                &command_name,
                command_args.as_deref(),
            ),
            Event::Error(payload) => self.handle_session_error(&payload, now_ms),
            Event::Warning { message } => self.handle_session_warning(&message),
            Event::CompactionStarted { instruction } => {
                self.handle_compaction_begin(instruction.as_deref(), now_ms);
            }
            Event::CompactionCompleted { result } => {
                self.handle_compaction_end(&result, now_ms);
            }
            Event::CompactionBlocked => {}
            Event::CompactionCancelled => self.handle_compaction_cancel(now_ms),
            e @ (Event::SubagentSpawned { .. }
            | Event::SubagentStarted { .. }
            | Event::SubagentSuspended { .. }
            | Event::SubagentCompleted { .. }
            | Event::SubagentFailed { .. }) => {
                self.subagent.handle_lifecycle_event(&mut self.state, e);
            }
            Event::TaskStarted { info } => self.handle_background_task_event(info, true),
            Event::TaskTerminated { info } => self.handle_background_task_event(info, false),
            Event::CronFired { prompt, origin } => self.handle_cron_fired(&prompt, &origin),
            Event::McpServerStatus { server } => self.render_mcp_server_status(&server),
            Event::ToolListUpdated => {}
        }

        // Aggregate the live assistant streaming block into the transcript.
        self.sync_streaming_transcript();
    }

    /// `resetRuntimeState` — cleared between sessions.
    pub fn reset_runtime_state(&mut self) {
        self.state.background_tasks.clear();
        self.state.background_tasks_transcripted_terminal.clear();
        self.state.rendered_skill_activation_ids.clear();
        self.state.rendered_plugin_command_activation_ids.clear();
        self.state.rendered_mcp_server_status_keys.clear();
        self.state.mcp_servers.clear();
        self.state.mcp_pending_names.clear();
        self.state.plugin_command_turns.clear();
        self.state.plugin_mcp_tools_used_in_turn.clear();
        self.subagent.reset_runtime_state(&mut self.state);
    }

    /// `clearAgentSwarmProgress`.
    pub fn clear_agent_swarm_progress(&mut self) {
        self.subagent.clear_agent_swarm_progress();
    }

    // -----------------------------------------------------------------------
    // Private handlers — turn lifecycle
    // -----------------------------------------------------------------------

    fn handle_turn_begin(&mut self, turn_id: &str, origin: Option<PromptOrigin>, now_ms: u64) {
        // A new turn means a pending WaitFor wait ended: freeze its count-up
        // before resetToolUi tears down the tool-call bookkeeping.
        self.state.streaming.finalize_active_wait();
        if let Some(PromptOrigin::PluginCommand { plugin_id, .. }) = origin {
            self.state
                .plugin_command_turns
                .insert(turn_id.to_owned(), plugin_id);
        }
        self.subagent.clear_agent_swarm_progress();
        self.state.streaming.reset_tool_ui(&mut self.state.effects);
        self.state.streaming.set_step(0);
        self.state.live_pane = LivePaneState {
            mode: LivePaneMode::Waiting,
        };
        self.state.app.streaming_phase = StreamingPhase::Waiting;
        self.state.app.streaming_start_time = now_ms;
    }

    fn handle_turn_end(
        &mut self,
        turn_id: &str,
        reason: TurnEndReason,
        error: Option<&ErrorPayload>,
        now_ms: u64,
    ) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        if reason == TurnEndReason::Cancelled {
            self.subagent.mark_active_agent_swarms_cancelled();
        }
        if reason == TurnEndReason::Failed
            && error
                .map(|e| e.code.as_str())
                .is_some_and(|c| c == "provider.filtered")
        {
            self.state.show_status(
                "Turn stopped: provider safety policy blocked the response.",
                Some(ColorToken::Error),
            );
        }
        if reason == TurnEndReason::Blocked {
            self.state.show_status(
                "Turn stopped: prompt hook blocked the request.",
                Some(ColorToken::Error),
            );
        }
        // TODO(legacy): if all todos done → streamingUI.setTodoList([])
        self.state.streaming.reset_tool_ui(&mut self.state.effects);
        self.finalize_turn(now_ms);

        // Plugin usage is reported once the whole turn's output has ended; a
        // cancelled turn cut the output short, so skip the notice there.
        let report_plugin_usage = reason != TurnEndReason::Cancelled;
        let plugin_command_plugin_id = self.state.plugin_command_turns.remove(turn_id);
        if let Some(plugin_id) = plugin_command_plugin_id {
            if report_plugin_usage {
                // TODO(legacy): pluginUpdateNotifier.handlePluginCommandCompleted(pluginId)
                let _ = plugin_id;
            }
        }
        if report_plugin_usage {
            let used = std::mem::take(&mut self.state.plugin_mcp_tools_used_in_turn);
            // TODO(legacy): for each toolName → pluginUpdateNotifier.handleMcpToolCompleted(toolName)
            let _ = used;
        }
    }

    /// `streamingUI.finalizeTurn` — the pure turn-finalization policy.
    fn finalize_turn(&mut self, now_ms: u64) {
        if self.state.app.streaming_phase.is_idle() {
            return;
        }
        self.state.defer_user_messages = false;
        let completed_turn_key = self
            .state
            .streaming
            .get_turn_context()
            .0
            .map(str::to_owned)
            .unwrap_or_else(|| format!("local:{}", self.state.app.streaming_start_time));
        self.state
            .streaming
            .finalize_live_text_buffers(&mut self.state.effects, now_ms);
        // TODO(legacy): host.collapseTrailingToolCalls(); host.mergeCurrentTurnSteps()
        self.state.streaming.reset_tool_call_state();
        self.state.streaming.set_turn_id(None);

        let next = self.state.shift_queued_message();
        if let Some(next) = next {
            // The message is out of the queue but not yet sent: mark the
            // dispatch pending so nothing can start a turn ahead of it.
            self.state.app.queued_message_dispatch_pending = true;
            self.state.app.streaming_phase = StreamingPhase::Idle;
            self.state.live_pane = LivePaneState::default();
            // TODO(legacy): setTimeout(() => { dispatchPending = false; sendQueued(next) }, 0)
            let _ = next;
            return;
        }

        self.state.app.streaming_phase = StreamingPhase::Idle;
        self.state.live_pane = LivePaneState::default();
        // TODO(legacy): notifyTerminalOnce(state, `turn-complete:${completedTurnKey}`, …)
        let _ = completed_turn_key;
    }

    fn handle_step_begin(&mut self, _turn_id: &str, step: u32, now_ms: u64) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        self.state.streaming.set_step(step);
        self.state.streaming.reset_tool_ui(&mut self.state.effects);
        self.state
            .streaming
            .finalize_live_text_buffers(&mut self.state.effects, now_ms);
        self.state.live_pane = LivePaneState {
            mode: LivePaneMode::Waiting,
        };
        self.state.app.streaming_phase = StreamingPhase::Waiting;
        self.state.app.streaming_start_time = now_ms;
    }

    fn handle_step_interrupted(
        &mut self,
        _turn_id: &str,
        _step: u32,
        reason: &str,
        message: Option<&str>,
        now_ms: u64,
    ) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        self.state.streaming.reset_tool_ui(&mut self.state.effects);
        self.state
            .streaming
            .finalize_live_text_buffers(&mut self.state.effects, now_ms);
        if reason == "error" {
            return;
        }
        if reason == "aborted" || reason.is_empty() {
            self.subagent.mark_active_agent_swarms_cancelled();
            match message {
                None | Some("") => {
                    self.state
                        .show_status("Interrupted by user", Some(ColorToken::Error));
                }
                Some(msg) => {
                    self.state.show_error(msg);
                }
            }
            return;
        }
        self.state.show_error(if reason == "max_steps" {
            "reached per-turn step limit (max_steps)".to_owned()
        } else {
            format!("step interrupted ({reason})")
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_step_completed(
        &mut self,
        turn_id: &str,
        step: u32,
        usage: Option<serde_json::Value>,
        provider_finish_reason: Option<&str>,
        raw_finish_reason: Option<&str>,
        finish_reason: Option<&str>,
        now_ms: u64,
    ) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        // TODO(legacy): maybeShowDebugTiming (DIMI_CODE_DEBUG env-gated)

        // Keep the latest per-step usage for the footer CH% badge.
        if let Some(usage) = usage {
            self.state.app.latest_prompt_usage = Some(usage);
        }

        if provider_finish_reason == Some("filtered") {
            self.state.show_notice(
                "Provider safety policy blocked the response.",
                Some(format!(
                    "The model output was filtered ({}).",
                    raw_finish_reason.unwrap_or("content_filter")
                )),
            );
            return;
        }

        if finish_reason != Some("max_tokens") {
            return;
        }

        let truncated_count = self.state.streaming.mark_step_truncated(turn_id, step);
        let title = if truncated_count > 0 {
            "Model hit max_tokens — tool call was truncated before it could run."
        } else {
            "Model hit max_tokens — no tool call was emitted."
        };
        self.state.show_notice(title, None);
    }

    fn handle_cron_fired(&mut self, prompt: &str, origin: &CronFiredOrigin) {
        self.state.streaming.flush_now(&mut self.state.effects, 0);
        let turn_id = self.state.streaming.get_turn_context().0.map(str::to_owned);
        let mut entry = TranscriptEntry::new(
            self.state.next_entry_id(),
            TranscriptEntryKind::Cron,
            turn_id,
            RenderMode::Plain,
            prompt,
        );
        entry.cron_data = Some(CronData {
            job_id: Some(origin.job_id.clone()),
            cron: Some(origin.cron.clone()),
            recurring: Some(origin.recurring),
            coalesced_count: Some(origin.coalesced_count),
            stale: Some(origin.stale),
            missed_count: None,
        });
        self.state.transcript.push(entry);
    }

    // -----------------------------------------------------------------------
    // Private handlers — model deltas
    // -----------------------------------------------------------------------

    fn handle_thinking_delta(&mut self, delta: &str, now_ms: u64) {
        // Encrypted / redacted reasoning streams whitespace-only thinking;
        // keep the waiting moon up until real text shows.
        if delta.trim().is_empty() && !self.state.streaming.has_thinking_draft() {
            return;
        }
        self.state.streaming.append_thinking_delta(delta);
        self.state.live_pane.mode = LivePaneMode::Idle;
        if self.state.app.streaming_phase != StreamingPhase::Thinking {
            self.state.app.streaming_phase = StreamingPhase::Thinking;
            self.state.app.streaming_start_time = now_ms;
        }
        // TODO(legacy): host arms a timer for the returned delay.
        let _ = self.state.streaming.schedule_flush(now_ms);
    }

    fn handle_assistant_delta(&mut self, delta: &str, now_ms: u64) {
        if self.state.streaming.has_thinking_draft() {
            self.state
                .streaming
                .flush_thinking_to_transcript(&mut self.state.effects, now_ms);
        }
        self.state
            .streaming
            .append_assistant_delta(&mut self.state.effects, delta);
        self.state.live_pane = LivePaneState {
            mode: LivePaneMode::Idle,
        };
        if self.state.app.streaming_phase != StreamingPhase::Composing {
            self.state.app.streaming_phase = StreamingPhase::Composing;
            self.state.app.streaming_start_time = now_ms;
        }
        let _ = self.state.streaming.schedule_flush(now_ms);
    }

    fn handle_hook_result(
        &mut self,
        _turn_id: &str,
        hook_event: &str,
        content: &str,
        blocked: bool,
        now_ms: u64,
    ) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        if self.state.streaming.has_thinking_draft() {
            self.state
                .streaming
                .flush_thinking_to_transcript(&mut self.state.effects, now_ms);
        }
        self.state
            .streaming
            .finalize_assistant_stream(&mut self.state.effects, now_ms);
        let turn_id = self.state.streaming.get_turn_context().0.map(str::to_owned);
        let entry_id = self.state.next_entry_id();
        self.state.transcript.push(TranscriptEntry::new(
            entry_id,
            TranscriptEntryKind::Assistant,
            turn_id,
            RenderMode::Markdown,
            format_hook_result_markdown(hook_event, content, blocked),
        ));
        self.state.live_pane = LivePaneState {
            mode: LivePaneMode::Idle,
        };
    }

    // -----------------------------------------------------------------------
    // Private handlers — tool calls
    // -----------------------------------------------------------------------

    fn handle_tool_call(
        &mut self,
        tool_call_id: &str,
        name: &str,
        args: Args,
        description: Option<String>,
        now_ms: u64,
    ) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        let swarm_args = args.clone();
        let (turn_id, step) = self.state.streaming.get_turn_context();
        let tool_call = ToolCallBlockData {
            id: tool_call_id.to_owned(),
            name: name.to_owned(),
            args,
            description,
            streaming_arguments: None,
            streaming_started_at_ms: None,
            result: None,
            step: Some(step),
            turn_id: turn_id.map(str::to_owned),
            truncated: false,
        };
        self.state
            .streaming
            .register_tool_call(&mut self.state.effects, tool_call);
        if name == "AgentSwarm" {
            self.subagent.handle_agent_swarm_tool_call_started(
                &mut self.state,
                tool_call_id,
                &swarm_args,
                now_ms,
            );
        }
        self.state.live_pane = LivePaneState {
            mode: LivePaneMode::Tool,
        };
    }

    fn handle_tool_call_delta(
        &mut self,
        tool_call_id: &str,
        name: Option<&str>,
        arguments_part: Option<&str>,
        now_ms: u64,
    ) {
        if tool_call_id.is_empty() {
            return;
        }
        self.state
            .streaming
            .accumulate_tool_call_delta(tool_call_id, name, arguments_part, now_ms);
        let preview = self
            .state
            .streaming
            .get_streaming_tool_call_preview(tool_call_id);
        if let Some(preview) = &preview {
            if preview.name == "AgentSwarm" || self.subagent.has_agent_swarm_progress(tool_call_id)
            {
                self.subagent.handle_agent_swarm_tool_call_delta(
                    &mut self.state,
                    tool_call_id,
                    preview,
                    now_ms,
                );
            }
        }
        self.state.live_pane = LivePaneState {
            mode: LivePaneMode::Tool,
        };
        if self.state.app.streaming_phase != StreamingPhase::Composing {
            self.state.app.streaming_phase = StreamingPhase::Composing;
            self.state.app.streaming_start_time = now_ms;
        }
        let _ = self.state.streaming.schedule_flush(now_ms);
    }

    fn handle_tool_progress(
        &mut self,
        tool_call_id: &str,
        kind: crate::controllers::events::ToolProgressKind,
        text: Option<String>,
    ) {
        let Some(text) = text else { return };
        if text.is_empty() {
            return;
        }
        if !self.state.streaming.has_tool_component(tool_call_id) {
            return;
        }
        // TODO(legacy): tc.appendProgress(text) / tc.appendLiveOutput(text)
        let entry = self
            .state
            .tool_live_outputs
            .entry(tool_call_id.to_owned())
            .or_default();
        if kind == crate::controllers::events::ToolProgressKind::Status {
            entry.push(format!("[status] {text}"));
        } else {
            entry.push(text);
        }
    }

    fn handle_tool_result(
        &mut self,
        tool_call_id: &str,
        output: &serde_json::Value,
        is_error: bool,
        synthetic: bool,
        now_ms: u64,
    ) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        let result = ToolResultBlockData {
            tool_call_id: tool_call_id.to_owned(),
            output: serialize_tool_result_output(output),
            is_error,
            synthetic,
        };
        let result_output = result.output.clone();
        let matched_call = self.state.streaming.complete_tool_result(
            &mut self.state.effects,
            tool_call_id,
            result,
        );
        if let Some(matched) = &matched_call {
            if is_plugin_mcp_tool_name(&matched.name) {
                // Buffer plugin MCP usage for the turn; the update notice fires
                // once the whole turn's output has ended.
                self.state
                    .plugin_mcp_tools_used_in_turn
                    .insert(matched.name.clone());
            }
        }
        self.subagent.handle_agent_swarm_tool_result(
            &mut self.state,
            tool_call_id,
            &result_output,
            is_error,
        );
        if let Some(matched) = &matched_call {
            if matched.name == "TodoList" && !is_error {
                if let Some(raw_todos) = matched
                    .args
                    .get("todos")
                    .and_then(serde_json::Value::as_array)
                {
                    let items: Vec<TodoItem> = raw_todos
                        .iter()
                        .filter_map(|t| {
                            let title = t.get("title").and_then(serde_json::Value::as_str)?;
                            let status = t.get("status").and_then(serde_json::Value::as_str)?;
                            if !is_todo_item_shape(title, status) {
                                return None;
                            }
                            let status = todo_status_from_str(status)?;
                            Some(TodoItem::new(title, status))
                        })
                        .collect();
                    if !items.is_empty() || raw_todos.is_empty() {
                        self.state
                            .streaming
                            .set_todo_list(&mut self.state.effects, items);
                    }
                }
            }
        }
        self.state.live_pane = LivePaneState {
            mode: LivePaneMode::Waiting,
        };
    }

    // -----------------------------------------------------------------------
    // Private handlers — status / session meta
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn handle_status_update(
        &mut self,
        context_usage: Option<f64>,
        context_tokens: Option<u64>,
        max_context_tokens: Option<u64>,
        usage: Option<serde_json::Value>,
        plan_mode: Option<bool>,
        swarm_mode: Option<bool>,
        permission: Option<PermissionMode>,
        model: Option<&str>,
        thinking_effort: Option<&str>,
    ) {
        if let Some(v) = context_usage {
            self.state.app.context_usage = Some(v);
        }
        if let Some(v) = context_tokens {
            self.state.app.context_tokens = Some(v);
        }
        if let Some(v) = max_context_tokens {
            self.state.app.max_context_tokens = Some(v);
        }
        if let Some(v) = usage {
            self.state.app.session_usage = Some(v);
        }
        if let Some(v) = plan_mode {
            self.state.app.plan_mode = v;
        }
        // Snapshot the pre-patch swarm mode BEFORE `app.swarm_mode` is set to
        // `false` below — TS computes `shouldRenderSwarmEnded` from
        // `appState.swarmMode` before applying the patch.
        let should_render_swarm_ended = swarm_mode == Some(false)
            && self.state.app.swarm_mode
            && self.state.swarm_mode_entry == Some(SwarmModeEntry::Task);
        if let Some(v) = swarm_mode {
            self.state.app.swarm_mode = v;
        }
        if let Some(v) = permission {
            self.state.app.permission_mode = v;
        }
        if let Some(v) = model {
            self.state.app.model = v.to_owned();
        }
        if let Some(v) = thinking_effort {
            self.state.app.thinking_effort = v.to_owned();
        }
        if swarm_mode == Some(false) {
            self.state.swarm_mode_entry = None;
            if should_render_swarm_ended {
                // `renderSwarmModeMarker('ended')` — a "Swarm ended" marker.
                let turn_id = self.state.streaming.get_turn_context().0.map(str::to_owned);
                let entry = TranscriptEntry::new(
                    self.state.next_entry_id(),
                    TranscriptEntryKind::Status,
                    turn_id,
                    RenderMode::Plain,
                    "Swarm ended",
                );
                self.state.transcript.push(entry);
            }
        }
    }

    fn handle_session_meta_changed(&mut self, title: Option<&str>, patch_title: Option<&str>) {
        let title = title.or(patch_title);
        if let Some(title) = title {
            self.state.app.session_title = Some(title.to_owned());
            // TODO(legacy): host.updateTerminalTitle()
        }
    }

    fn handle_session_error(&mut self, payload: &ErrorPayload, now_ms: u64) {
        self.state
            .streaming
            .flush_now(&mut self.state.effects, now_ms);
        self.state.streaming.reset_tool_ui(&mut self.state.effects);
        self.state
            .streaming
            .finalize_live_text_buffers(&mut self.state.effects, now_ms);
        if payload.code == AUTH_LOGIN_REQUIRED_CODE {
            self.state.show_error(AUTH_LOGIN_REQUIRED_STARTUP_NOTICE);
            return;
        }
        self.state.show_error(format_error_payload(payload));
        if !self.state.app.session_id.is_empty() {
            // TODO(legacy): showStatus(errorReportHintLine())
        }
    }

    fn handle_session_warning(&mut self, message: &str) {
        self.state
            .show_status(format!("Warning: {message}"), Some(ColorToken::Warning));
    }

    fn handle_skill_activated(
        &mut self,
        activation_id: &str,
        skill_name: &str,
        skill_args: Option<&str>,
        trigger: crate::controllers::SkillActivationTrigger,
    ) {
        if self
            .state
            .rendered_skill_activation_ids
            .contains(activation_id)
        {
            return;
        }
        self.state
            .rendered_skill_activation_ids
            .insert(activation_id.to_owned());
        let mut entry = TranscriptEntry::new(
            self.state.next_entry_id(),
            TranscriptEntryKind::SkillActivation,
            None,
            RenderMode::Plain,
            format!("Activated skill: {skill_name}"),
        );
        entry.skill_activation_id = Some(activation_id.to_owned());
        entry.skill_name = Some(skill_name.to_owned());
        entry.skill_args = skill_args.map(str::to_owned);
        entry.skill_trigger = Some(trigger);
        self.state.transcript.push(entry);
    }

    fn handle_plugin_command_activated(
        &mut self,
        activation_id: &str,
        plugin_id: &str,
        command_name: &str,
        command_args: Option<&str>,
    ) {
        if self
            .state
            .rendered_plugin_command_activation_ids
            .contains(activation_id)
        {
            return;
        }
        self.state
            .rendered_plugin_command_activation_ids
            .insert(activation_id.to_owned());
        let mut entry = TranscriptEntry::new(
            self.state.next_entry_id(),
            TranscriptEntryKind::PluginCommand,
            None,
            RenderMode::Plain,
            format!("/{plugin_id}:{command_name}"),
        );
        entry.plugin_command_data = Some(crate::controllers::PluginCommandProjection {
            activation_id: activation_id.to_owned(),
            plugin_id: plugin_id.to_owned(),
            command_name: command_name.to_owned(),
            command_args: command_args.map(str::to_owned),
        });
        self.state.transcript.push(entry);
    }

    // -----------------------------------------------------------------------
    // Private handlers — compaction
    // -----------------------------------------------------------------------

    fn handle_compaction_begin(&mut self, instruction: Option<&str>, now_ms: u64) {
        self.state
            .streaming
            .finalize_live_text_buffers(&mut self.state.effects, now_ms);
        self.state.app.is_compacting = true;
        self.state.app.streaming_phase = StreamingPhase::Waiting;
        self.state.app.streaming_start_time = now_ms;
        self.state
            .streaming
            .begin_compaction(&mut self.state.effects, instruction.map(str::to_owned));
    }

    fn handle_compaction_end(&mut self, result: &CompactionCompletedData, now_ms: u64) {
        self.state.streaming.end_compaction(
            &mut self.state.effects,
            result.tokens_before,
            result.tokens_after,
            result.summary.clone(),
        );
        let _ = now_ms;
        self.finish_compaction();
    }

    fn handle_compaction_cancel(&mut self, now_ms: u64) {
        self.state
            .streaming
            .cancel_compaction(&mut self.state.effects);
        let _ = now_ms;
        self.finish_compaction();
    }

    /// `finishCompaction`.
    fn finish_compaction(&mut self) {
        let has_active_turn = self.state.streaming.has_active_turn();
        if !has_active_turn {
            let next = self.state.shift_queued_message();
            if next.is_some() {
                self.state.app.queued_message_dispatch_pending = true;
            }
            self.state.app.is_compacting = false;
            self.state.app.streaming_phase = StreamingPhase::Idle;
            self.state.live_pane = LivePaneState::default();
            if next.is_some() {
                // TODO(legacy): setTimeout(() => { dispatchPending = false; sendQueued(next) }, 0)
            }
        } else {
            self.state.app.is_compacting = false;
        }
    }

    // -----------------------------------------------------------------------
    // Private handlers — background tasks
    // -----------------------------------------------------------------------

    fn handle_background_task_event(&mut self, info: BackgroundTaskInfo, started: bool) {
        let previous = self
            .state
            .background_tasks
            .insert(info.task_id.clone(), info.clone());
        // TODO(legacy): tasksBrowser refreshOutputViewer if viewer matches taskId

        let is_terminal = info.status.is_terminal();

        if started {
            if info.kind == BackgroundTaskKind::Agent {
                // A foreground subagent detached via Ctrl+B: flip its card to
                // `◐ backgrounded`.
                if let Some(agent_id) = &info.agent_id {
                    self.state.streaming.mark_subagent_backgrounded(agent_id);
                }
                self.sync_background_task_badge();
                // TODO(legacy): tasksBrowserController.repaint()
                return;
            }
            // Default-async tools emit task.started for nearly every call;
            // only process/question (and tool failures via terminated) belong
            // in the transcript.
            if should_show_background_task_transcript(&info) {
                self.append_background_task_entry(&info);
            }
            self.sync_background_task_badge();
            // TODO(legacy): tasksBrowserController.repaint()
            return;
        }

        if is_terminal {
            if info.kind == BackgroundTaskKind::Agent {
                // The Agent tool's spawn-success ToolResult is not an error, so
                // push the actual terminal status onto the card.
                self.state.streaming.apply_background_task_terminal_status(
                    &ApplyTerminalStatusArgs {
                        agent_id: info.agent_id.clone(),
                        description: info.description.clone().unwrap_or_default(),
                        status: info.status,
                        error_text: None,
                    },
                );
            }
            if !self
                .state
                .background_tasks_transcripted_terminal
                .contains(&info.task_id)
            {
                if should_show_background_task_transcript(&info) {
                    self.append_background_task_entry(&info);
                }
                self.state
                    .background_tasks_transcripted_terminal
                    .insert(info.task_id.clone());
            }
            self.sync_background_task_badge();
            // TODO(legacy): tasksBrowserController.repaint()
            return;
        }

        if previous.as_ref().map(|p| p.status) != Some(info.status) {
            self.sync_background_task_badge();
        }
        // TODO(legacy): tasksBrowserController.repaint()
    }

    fn append_background_task_entry(&mut self, info: &BackgroundTaskInfo) {
        let status = format_background_task_transcript(info);
        let turn_id = self.state.streaming.get_turn_context().0.map(str::to_owned);
        let mut entry = TranscriptEntry::new(
            self.state.next_entry_id(),
            TranscriptEntryKind::Status,
            turn_id,
            RenderMode::Plain,
            status.headline.clone(),
        );
        entry.detail = status.detail.clone();
        entry.background_agent_status = Some(status);
        self.state.transcript.push(entry);
    }

    /// `syncBackgroundTaskBadge` — pure counts; the footer write is legacy.
    fn sync_background_task_badge(&mut self) {
        let mut bash_tasks = 0u64;
        let mut agent_tasks = 0u64;
        for info in self.state.background_tasks.values() {
            if info.status.is_terminal() {
                continue;
            }
            if info.kind == BackgroundTaskKind::Agent {
                agent_tasks += 1;
            } else {
                bash_tasks += 1;
            }
        }
        self.state.background_badge = (bash_tasks, agent_tasks);
        // TODO(legacy): state.footer.setBackgroundCounts({ bashTasks, agentTasks })
    }

    // -----------------------------------------------------------------------
    // Private handlers — mcp
    // -----------------------------------------------------------------------

    fn render_mcp_server_status(&mut self, server: &McpServerStatusSnapshot) {
        let key = mcp_server_status_key(server);
        if self.state.rendered_mcp_server_status_keys.get(&server.name) == Some(&key) {
            return;
        }
        self.state
            .rendered_mcp_server_status_keys
            .insert(server.name.clone(), key);
        self.state
            .mcp_servers
            .insert(server.name.clone(), server.clone());
        let summary = format_mcp_startup_status_summary(self.state.mcp_servers.values());
        self.state.app.mcp_servers_summary = if summary.is_empty() {
            None
        } else {
            Some(summary)
        };

        match server.status {
            McpServerStatus::Connected => {
                let tool_str = if server.tool_count == 1 {
                    "1 tool".to_owned()
                } else {
                    format!("{} tools", server.tool_count)
                };
                let message = format!(
                    "MCP server \"{}\" connected · {tool_str} ({})",
                    server.name, server.transport
                );
                self.finalize_mcp_server_status_row(
                    &server.name,
                    message,
                    Some(ColorToken::Success),
                );
            }
            McpServerStatus::Failed => {
                let message = match &server.error {
                    Some(error) => format!("MCP server \"{}\" failed: {error}", server.name),
                    None => format!("MCP server \"{}\" failed", server.name),
                };
                self.finalize_mcp_server_status_row(&server.name, message, Some(ColorToken::Error));
            }
            McpServerStatus::NeedsAuth => {
                let message = format!(
                    "MCP server \"{}\" needs OAuth — run /mcp-config login {}",
                    server.name, server.name
                );
                self.finalize_mcp_server_status_row(
                    &server.name,
                    message,
                    Some(ColorToken::Warning),
                );
            }
            McpServerStatus::Disabled => {
                let message = format!("MCP server \"{}\" disabled", server.name);
                self.finalize_mcp_server_status_row(
                    &server.name,
                    message,
                    Some(ColorToken::TextMuted),
                );
            }
            McpServerStatus::Pending => {
                // TODO(legacy): showMcpServerStatusSpinner(name)
                self.state.mcp_pending_names.insert(server.name.clone());
            }
        }
    }

    /// `finalizeMcpServerStatusRow` — replaces a pending spinner when present,
    /// else surfaces a status line.
    fn finalize_mcp_server_status_row(
        &mut self,
        name: &str,
        message: String,
        color: Option<ColorToken>,
    ) {
        // TODO(legacy): replace spinner component in transcriptContainer
        self.state.mcp_pending_names.remove(name);
        self.state.show_status(&message, color);
    }

    // -----------------------------------------------------------------------
    // Transcript aggregation
    // -----------------------------------------------------------------------

    /// Aggregate the live assistant streaming block into the transcript
    /// (delegates to [`UiState::sync_streaming_transcript`]).
    fn sync_streaming_transcript(&mut self) {
        self.state.sync_streaming_transcript();
    }
}

// ---------------------------------------------------------------------------
// Pure formatting helpers
// ---------------------------------------------------------------------------

/// `isTodoItemShape` (`event-payload.ts`).
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

/// `formatErrorPayload` (`event-payload.ts`) — `[code] message`, with the
/// provider-filtered special case.
pub fn format_error_payload(error: &ErrorPayload) -> String {
    match format_provider_filtered_message(error.details.as_ref()) {
        Some(filtered) => format!("[{}] {filtered}", error.code),
        None => format!("[{}] {}", error.code, error.message),
    }
}

fn format_provider_filtered_message(details: Option<&Args>) -> Option<String> {
    let finish_reason = string_detail(details, "finishReason");
    let raw_finish_reason = string_detail(details, "rawFinishReason");
    if finish_reason.as_deref() != Some("filtered")
        && raw_finish_reason.as_deref() != Some("content_filter")
    {
        return None;
    }
    let normalized = finish_reason.unwrap_or_else(|| "filtered".to_owned());
    let raw = match raw_finish_reason {
        Some(raw) => format!(", rawFinishReason={raw}"),
        None => String::new(),
    };
    Some(format!(
        "Provider filtered the response before visible output (finishReason={normalized}{raw})."
    ))
}

fn string_detail(details: Option<&Args>, key: &str) -> Option<String> {
    details
        .and_then(|d| d.get(key))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// `formatHookResultMarkdown` (`hook-result-format.ts`).
pub fn format_hook_result_markdown(hook_event: &str, content: &str, blocked: bool) -> String {
    format!(
        "*{}*\n\n{}",
        format_hook_result_title(hook_event, blocked),
        format_hook_result_body(content)
    )
}

/// `formatHookResultPlain` (`hook-result-format.ts`).
pub fn format_hook_result_plain(hook_event: &str, content: &str, blocked: bool) -> String {
    format!(
        "{}\n\n{}",
        format_hook_result_title(hook_event, blocked),
        format_hook_result_body(content)
    )
}

fn format_hook_result_title(hook_event: &str, blocked: bool) -> String {
    if blocked {
        format!("{hook_event} hook blocked")
    } else {
        format!("{hook_event} hook")
    }
}

fn format_hook_result_body(content: &str) -> String {
    let content = content.trim();
    if content.is_empty() {
        "(empty)".to_owned()
    } else {
        content.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Background task formatting (`background-task-status.ts` /
// `background-agent-status.ts`)
// ---------------------------------------------------------------------------

/// `shouldShowBackgroundTaskTranscript`.
pub fn should_show_background_task_transcript(info: &BackgroundTaskInfo) -> bool {
    if info.kind == BackgroundTaskKind::Agent {
        return false;
    }
    // Foreground lifecycle must not claim "in background".
    if info.detached == Some(false) {
        return false;
    }
    if info.kind == BackgroundTaskKind::Tool {
        return matches!(
            info.status,
            BackgroundTaskStatus::Failed
                | BackgroundTaskStatus::TimedOut
                | BackgroundTaskStatus::Killed
                | BackgroundTaskStatus::Lost
        );
    }
    true
}

/// `formatBackgroundTaskTranscript`.
pub fn format_background_task_transcript(info: &BackgroundTaskInfo) -> BackgroundAgentStatusData {
    BackgroundAgentStatusData {
        phase: phase_from_status(info.status),
        headline: headline_for(info),
        detail: detail_for(info),
    }
}

fn phase_from_status(status: BackgroundTaskStatus) -> BackgroundTaskPhase {
    match status {
        BackgroundTaskStatus::Running => BackgroundTaskPhase::Started,
        BackgroundTaskStatus::Completed => BackgroundTaskPhase::Completed,
        BackgroundTaskStatus::Failed
        | BackgroundTaskStatus::TimedOut
        | BackgroundTaskStatus::Killed
        | BackgroundTaskStatus::Lost => BackgroundTaskPhase::Failed,
    }
}

fn subject_for(info: &BackgroundTaskInfo) -> &'static str {
    match info.kind {
        BackgroundTaskKind::Agent => "agent task",
        BackgroundTaskKind::Question => "question task",
        BackgroundTaskKind::Tool => "tool task",
        BackgroundTaskKind::Process => "bash task",
    }
}

fn headline_for(info: &BackgroundTaskInfo) -> String {
    let subject = subject_for(info);
    match info.status {
        BackgroundTaskStatus::Running => format!("{subject} started in background"),
        BackgroundTaskStatus::Completed => format!("{subject} completed in background"),
        BackgroundTaskStatus::Failed => format!("{subject} failed in background"),
        BackgroundTaskStatus::TimedOut => format!("{subject} timed out"),
        BackgroundTaskStatus::Killed => format!("{subject} stopped"),
        BackgroundTaskStatus::Lost => format!("{subject} lost"),
    }
}

fn normalize_background_field(value: &str) -> Option<String> {
    let collapsed: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    const MAX: usize = 240;
    if collapsed.chars().count() <= MAX {
        return Some(collapsed);
    }
    let mut out: String = collapsed.chars().take(MAX - 3).collect();
    out.push_str("...");
    Some(out)
}

fn detail_for(info: &BackgroundTaskInfo) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(desc) = info
        .description
        .as_deref()
        .and_then(normalize_background_field)
    {
        parts.push(desc);
    }
    if matches!(
        info.status,
        BackgroundTaskStatus::Completed | BackgroundTaskStatus::Failed
    ) && info.kind == BackgroundTaskKind::Process
    {
        if let Some(code) = info.exit_code {
            parts.push(format!("exit {code}"));
        }
    }
    if info.status == BackgroundTaskStatus::Killed {
        parts.push(
            info.stop_reason
                .as_deref()
                .and_then(normalize_background_field)
                .map(|reason| format!("stopped — {reason}"))
                .unwrap_or_else(|| "stopped".to_owned()),
        );
    }
    if info.status == BackgroundTaskStatus::Failed {
        if let Some(reason) = info
            .stop_reason
            .as_deref()
            .and_then(normalize_background_field)
        {
            parts.push(reason);
        }
    }
    if info.status == BackgroundTaskStatus::TimedOut {
        parts.push("timed out".to_owned());
    }
    if info.status == BackgroundTaskStatus::Lost {
        parts.push("session restarted before completion".to_owned());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

/// Extras for [`format_background_agent_transcript`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgentExtras {
    pub result_summary: Option<String>,
    pub error: Option<String>,
}

/// `formatBackgroundAgentTranscript` (`background-agent-status.ts`).
pub fn format_background_agent_transcript(
    phase: BackgroundTaskPhase,
    meta: &BackgroundAgentMetadata,
    extras: Option<&BackgroundAgentExtras>,
) -> BackgroundAgentStatusData {
    let normalized_name = meta
        .agent_name
        .as_deref()
        .and_then(normalize_background_field);
    let subject = match normalized_name {
        Some(name) => format!("{name} agent"),
        None => "agent".to_owned(),
    };
    let headline = match phase {
        BackgroundTaskPhase::Started => format!("{subject} started in background"),
        BackgroundTaskPhase::Completed => format!("{subject} completed in background"),
        BackgroundTaskPhase::Failed => format!("{subject} failed in background"),
    };
    let tail = if phase == BackgroundTaskPhase::Failed {
        extras
            .and_then(|e| e.error.as_deref())
            .and_then(normalize_background_field)
    } else {
        None
    };
    let mut detail_parts = Vec::new();
    if let Some(desc) = meta
        .description
        .as_deref()
        .and_then(normalize_background_field)
    {
        detail_parts.push(desc);
    }
    if let Some(tail) = tail {
        detail_parts.push(tail);
    }
    BackgroundAgentStatusData {
        phase,
        headline,
        detail: if detail_parts.is_empty() {
            None
        } else {
            Some(detail_parts.join(" · "))
        },
    }
}

// ---------------------------------------------------------------------------
// MCP server status helpers (`mcp-server-status.ts`)
// ---------------------------------------------------------------------------

/// `MCP_STARTUP_STATUS_ROW_LIMIT`.
pub const MCP_STARTUP_STATUS_ROW_LIMIT: usize = 4;

fn mcp_startup_status_priority(status: McpServerStatus) -> u8 {
    match status {
        McpServerStatus::Failed => 0,
        McpServerStatus::NeedsAuth => 1,
        McpServerStatus::Pending => 2,
        McpServerStatus::Connected => 3,
        McpServerStatus::Disabled => 4,
    }
}

/// `selectMcpStartupStatusRows`.
pub fn select_mcp_startup_status_rows(
    servers: &[McpServerStatusSnapshot],
) -> Vec<McpServerStatusSnapshot> {
    let mut rows: Vec<&McpServerStatusSnapshot> = servers
        .iter()
        .filter(|s| s.status != McpServerStatus::Disabled)
        .collect();
    rows.sort_by_key(|s| mcp_startup_status_priority(s.status));
    rows.into_iter()
        .take(MCP_STARTUP_STATUS_ROW_LIMIT)
        .cloned()
        .collect()
}

/// `formatMcpStartupStatusSummary`.
pub fn format_mcp_startup_status_summary<'a, I>(servers: I) -> String
where
    I: IntoIterator<Item = &'a McpServerStatusSnapshot>,
{
    let mut failed = 0u64;
    let mut needs_auth = 0u64;
    let mut connecting = 0u64;
    let mut connected = 0u64;
    let mut disabled = 0u64;
    for server in servers {
        match server.status {
            McpServerStatus::Failed => failed += 1,
            McpServerStatus::NeedsAuth => needs_auth += 1,
            McpServerStatus::Pending => connecting += 1,
            McpServerStatus::Connected => connected += 1,
            McpServerStatus::Disabled => disabled += 1,
        }
    }
    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if needs_auth > 0 {
        parts.push(format!("{needs_auth} need auth"));
    }
    if connecting > 0 {
        parts.push(format!("{connecting} connecting"));
    }
    if connected > 0 {
        parts.push(format!("{connected} connected"));
    }
    if disabled > 0 {
        parts.push(format!("{disabled} disabled"));
    }
    parts.join(", ")
}

/// `mcpServerStatusKey` — `JSON.stringify([status, transport, toolCount, error])`.
pub fn mcp_server_status_key(server: &McpServerStatusSnapshot) -> String {
    serde_json::json!([
        match server.status {
            McpServerStatus::Connected => "connected",
            McpServerStatus::Failed => "failed",
            McpServerStatus::NeedsAuth => "needs-auth",
            McpServerStatus::Disabled => "disabled",
            McpServerStatus::Pending => "pending",
        },
        server.transport,
        server.tool_count,
        server.error,
    ])
    .to_string()
}

/// Cheap name check for plugin-provided MCP tools (`mcp__plugin-…`).
pub fn is_plugin_mcp_tool_name(tool_name: &str) -> bool {
    crate::controllers::plugin_update::is_plugin_mcp_tool_name(tool_name)
}

/// Message-mode helper for a queued message.
pub fn message_mode_of(item: &QueuedMessage) -> MessageMode {
    item.mode
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::events::{
        CompactionCompletedData, CronFiredOrigin, ErrorPayload, PromptOrigin, ToolProgressKind,
        ToolProgressUpdate, TurnEndReason,
    };
    use crate::controllers::{BackgroundTaskKind, SkillActivationTrigger};
    use serde_json::json;

    fn handler() -> SessionEventHandler {
        SessionEventHandler::new()
    }

    fn turn_started(turn_id: &str) -> Event {
        Event::TurnStarted {
            turn_id: turn_id.to_owned(),
            origin: Some(PromptOrigin::User),
        }
    }

    #[test]
    fn routing_turn_started_opens_waiting_phase() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        assert_eq!(h.state.app.streaming_phase, StreamingPhase::Waiting);
        assert_eq!(h.state.live_pane.mode, LivePaneMode::Waiting);
        assert!(h.state.streaming.has_active_turn());
        assert_eq!(h.state.streaming.get_turn_context().0, Some("1"));
    }

    #[test]
    fn routing_turn_ended_finalizes_and_returns_to_idle() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.state
            .streaming
            .append_assistant_delta(&mut h.state.effects, "hi");
        h.handle_event(
            Event::TurnEnded {
                agent_id: None,
                turn_id: "1".to_owned(),
                reason: TurnEndReason::Completed,
                error: None,
            },
            2000,
        );
        assert_eq!(h.state.app.streaming_phase, StreamingPhase::Idle);
        assert!(!h.state.streaming.has_active_turn());
        // The assistant draft was finalized into the transcript.
        let assistant: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::Assistant)
            .collect();
        assert_eq!(assistant.len(), 1);
        assert_eq!(assistant[0].content, "hi");
        assert!(assistant[0].model_text);
    }

    #[test]
    fn routing_thinking_delta_sets_thinking_phase() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::ThinkingDelta {
                agent_id: None,
                delta: "let me think".to_owned(),
            },
            1000,
        );
        assert_eq!(h.state.app.streaming_phase, StreamingPhase::Thinking);
        assert!(h.state.streaming.has_thinking_draft());
    }

    #[test]
    fn routing_thinking_whitespace_only_is_ignored_before_any_text() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::ThinkingDelta {
                agent_id: None,
                delta: " ".to_owned(),
            },
            1000,
        );
        // No draft yet → phase stays waiting (moon up), no thinking state.
        assert_eq!(h.state.app.streaming_phase, StreamingPhase::Waiting);
        assert!(!h.state.streaming.has_thinking_draft());
    }

    #[test]
    fn routing_assistant_delta_flushes_thinking_first() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::ThinkingDelta {
                agent_id: None,
                delta: "thought".to_owned(),
            },
            1000,
        );
        // Simulate the flush timer firing.
        h.state.streaming.flush_now(&mut h.state.effects, 1100);
        assert!(h.state.streaming.has_active_thinking_component());

        h.handle_event(
            Event::AssistantDelta {
                agent_id: None,
                delta: "answer".to_owned(),
            },
            1200,
        );
        // Thinking was flushed to transcript and ended before assistant text.
        let thinking_ended = h
            .state
            .effects
            .iter()
            .any(|e| matches!(e, StreamingEffect::ThinkingEnd));
        assert!(thinking_ended);
        assert_eq!(h.state.app.streaming_phase, StreamingPhase::Composing);
    }

    #[test]
    fn routing_tool_call_starts_component_and_tool_pane() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                name: "Bash".to_owned(),
                args: crate::controllers::args_json(json!({"command": "ls"})),
                description: None,
            },
            1000,
        );
        assert_eq!(h.state.live_pane.mode, LivePaneMode::Tool);
        assert!(h.state.streaming.has_active_tool_call("t1"));
        assert!(h.state.streaming.has_tool_component("t1"));
        assert_eq!(
            h.state.streaming.get_active_tool_call("t1").unwrap().name,
            "Bash"
        );
    }

    #[test]
    fn routing_tool_result_completes_call_and_todo_list() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                name: "TodoList".to_owned(),
                args: crate::controllers::args_json(json!({"todos": [
                    {"title": "a", "status": "in_progress"},
                    {"title": "b", "status": "done"},
                    {"title": "", "status": "pending"},
                    {"title": "c", "status": "weird"}
                ]})),
                description: None,
            },
            1000,
        );
        h.handle_event(
            Event::ToolResult {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                output: json!("ok"),
                is_error: false,
                synthetic: false,
            },
            1100,
        );
        assert!(!h.state.streaming.has_active_tool_call("t1"));
        assert_eq!(h.state.live_pane.mode, LivePaneMode::Waiting);
        let todos = h.state.streaming.todo_items();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].title, "a");
        assert_eq!(todos[1].title, "b");
    }

    #[test]
    fn routing_step_completed_max_tokens_truncates_tool_calls() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.state.streaming.set_step(0);
        let mut tc = ToolCallBlockData::new("t1", "Read");
        tc.step = Some(0);
        tc.turn_id = Some("1".to_owned());
        tc.streaming_arguments = Some("{}".to_owned());
        tc.streaming_started_at_ms = Some(1);
        h.state.streaming.set_active_tool_call("t1", tc);

        h.handle_event(
            Event::TurnStepCompleted {
                turn_id: "1".to_owned(),
                step: 0,
                usage: Some(json!({"total_tokens": 5})),
                provider_finish_reason: None,
                raw_finish_reason: None,
                finish_reason: Some("max_tokens".to_owned()),
            },
            1200,
        );
        assert!(
            h.state
                .streaming
                .get_active_tool_call("t1")
                .unwrap()
                .truncated
        );
        assert_eq!(
            h.state.app.latest_prompt_usage,
            Some(json!({"total_tokens": 5}))
        );
        let notice = h
            .state
            .messages
            .iter()
            .find(|m| matches!(m, UiMessage::Notice { .. }));
        assert!(notice.is_some());
    }

    #[test]
    fn routing_step_completed_filtered_shows_notice_and_returns() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::TurnStepCompleted {
                turn_id: "1".to_owned(),
                step: 0,
                usage: None,
                provider_finish_reason: Some("filtered".to_owned()),
                raw_finish_reason: Some("content_filter".to_owned()),
                finish_reason: Some("max_tokens".to_owned()),
            },
            1200,
        );
        let notice = h.state.messages.iter().find_map(|m| match m {
            UiMessage::Notice { title, .. } => Some(title.clone()),
            _ => None,
        });
        assert_eq!(
            notice.as_deref(),
            Some("Provider safety policy blocked the response.")
        );
    }

    #[test]
    fn routing_error_resets_live_buffers_and_formats_payload() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::ThinkingDelta {
                agent_id: None,
                delta: "t".to_owned(),
            },
            1000,
        );
        h.handle_event(
            Event::Error(ErrorPayload {
                code: "network".to_owned(),
                message: "connection lost".to_owned(),
                details: None,
            }),
            1500,
        );
        assert!(!h.state.streaming.has_thinking_draft());
        let err = h.state.messages.iter().find_map(|m| match m {
            UiMessage::Error(e) => Some(e.clone()),
            _ => None,
        });
        assert_eq!(err.as_deref(), Some("[network] connection lost"));
    }

    #[test]
    fn routing_auth_login_required_uses_startup_notice() {
        let mut h = handler();
        h.handle_event(
            Event::Error(ErrorPayload {
                code: AUTH_LOGIN_REQUIRED_CODE.to_owned(),
                message: "unused".to_owned(),
                details: None,
            }),
            0,
        );
        let err = h.state.messages.iter().find_map(|m| match m {
            UiMessage::Error(e) => Some(e.clone()),
            _ => None,
        });
        assert_eq!(err.as_deref(), Some(AUTH_LOGIN_REQUIRED_STARTUP_NOTICE));
    }

    #[test]
    fn routing_error_formatting_provider_filtered() {
        let mut details = Args::new();
        details.insert("finishReason".to_owned(), json!("filtered"));
        let payload = ErrorPayload {
            code: "provider.filtered".to_owned(),
            message: "generic".to_owned(),
            details: Some(details),
        };
        let formatted = format_error_payload(&payload);
        assert!(formatted.contains("Provider filtered the response"));
    }

    #[test]
    fn routing_skill_activated_is_deduplicated() {
        let mut h = handler();
        let event = Event::SkillActivated {
            activation_id: "a1".to_owned(),
            skill_name: "my-skill".to_owned(),
            skill_args: Some("x".to_owned()),
            trigger: SkillActivationTrigger::UserSlash,
        };
        h.handle_event(event.clone(), 0);
        h.handle_event(event, 0);
        let skills: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::SkillActivation)
            .collect();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].content, "Activated skill: my-skill");
    }

    #[test]
    fn routing_compaction_begin_end() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::CompactionStarted {
                instruction: Some("summarize".to_owned()),
            },
            1000,
        );
        assert!(h.state.app.is_compacting);
        assert!(h.state.streaming.active_compaction_block().is_some());

        h.handle_event(
            Event::CompactionCompleted {
                result: CompactionCompletedData {
                    tokens_before: 10,
                    tokens_after: 5,
                    summary: Some("sum".to_owned()),
                },
            },
            1100,
        );
        assert!(!h.state.app.is_compacting);
        assert!(h.state.streaming.active_compaction_block().is_none());
    }

    #[test]
    fn routing_cron_fired_appends_cron_entry() {
        let mut h = handler();
        h.handle_event(
            Event::CronFired {
                prompt: "check status".to_owned(),
                origin: CronFiredOrigin {
                    job_id: "j1".to_owned(),
                    cron: "*/5 * * * *".to_owned(),
                    recurring: true,
                    coalesced_count: 1,
                    stale: false,
                },
            },
            0,
        );
        let cron: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::Cron)
            .collect();
        assert_eq!(cron.len(), 1);
        assert_eq!(cron[0].content, "check status");
        assert_eq!(
            cron[0].cron_data.as_ref().unwrap().job_id.as_deref(),
            Some("j1")
        );
    }

    #[test]
    fn routing_background_task_started_terminated() {
        let mut h = handler();
        let info = BackgroundTaskInfo::new(
            "task1",
            BackgroundTaskKind::Process,
            BackgroundTaskStatus::Running,
        );
        h.handle_event(Event::TaskStarted { info }, 0);
        assert_eq!(h.state.background_badge, (1, 0));
        assert_eq!(h.state.background_tasks.len(), 1);

        // Process task failed → transcript card + badge update.
        let mut failed = BackgroundTaskInfo::new(
            "task1",
            BackgroundTaskKind::Process,
            BackgroundTaskStatus::Failed,
        );
        failed.exit_code = Some(1);
        h.handle_event(Event::TaskTerminated { info: failed }, 0);
        assert_eq!(h.state.background_badge, (0, 0));
        let statuses: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::Status)
            .collect();
        assert_eq!(statuses.len(), 2);
        // First card: task.started (process lifecycle). Second: task.terminated.
        assert!(statuses[1].content.contains("failed in background"));
        // Terminal transcripted only once.
        h.handle_event(
            Event::TaskTerminated {
                info: BackgroundTaskInfo::new(
                    "task1",
                    BackgroundTaskKind::Process,
                    BackgroundTaskStatus::Failed,
                ),
            },
            0,
        );
        assert_eq!(
            h.state
                .transcript
                .iter()
                .filter(|e| e.kind == TranscriptEntryKind::Status)
                .count(),
            2
        );
    }

    #[test]
    fn routing_agent_background_task_updates_card_status() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        // Spawn a foreground Agent card via subagent machinery.
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "agent-1".to_owned(),
                parent_tool_call_id: "card".to_owned(),
                subagent_name: "coder".to_owned(),
                run_in_background: false,
                swarm_index: None,
                description: Some("fix bug".to_owned()),
            },
            1000,
        );
        assert!(h.state.streaming.has_tool_component("card"));

        // A background task terminates the same agent as lost.
        let mut info =
            BackgroundTaskInfo::new("bg1", BackgroundTaskKind::Agent, BackgroundTaskStatus::Lost);
        info.agent_id = Some("agent-1".to_owned());
        info.description = Some("fix bug".to_owned());
        h.handle_event(Event::TaskTerminated { info }, 1100);
        let view = h.state.streaming.tool_call_subagent_view("card");
        assert!(view.is_some());
        assert_eq!(
            view.unwrap().terminal_status,
            Some(BackgroundTaskStatus::Lost)
        );
    }

    #[test]
    fn routing_mcp_server_status_connects() {
        let mut h = handler();
        let server = McpServerStatusSnapshot {
            name: "filesystem".to_owned(),
            status: McpServerStatus::Connected,
            tool_count: 3,
            transport: "stdio".to_owned(),
            error: None,
        };
        h.handle_event(Event::McpServerStatus { server }, 0);
        let status = h.state.messages.iter().find_map(|m| match m {
            UiMessage::Status { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(
            status.as_deref(),
            Some("MCP server \"filesystem\" connected · 3 tools (stdio)")
        );
        assert!(h.state.app.mcp_servers_summary.is_some());
    }

    #[test]
    fn routing_session_meta_sets_title() {
        let mut h = handler();
        h.handle_event(
            Event::SessionMetaUpdated {
                title: Some("My session".to_owned()),
                patch_title: None,
            },
            0,
        );
        assert_eq!(h.state.app.session_title.as_deref(), Some("My session"));
    }

    #[test]
    fn routing_swarm_ended_marker_uses_pre_patch_swarm_mode() {
        let mut h = handler();
        // A swarm task is active: swarm mode on + the task-mode entry.
        h.state.app.swarm_mode = true;
        h.state.swarm_mode_entry = Some(SwarmModeEntry::Task);
        h.handle_event(
            Event::AgentStatusUpdated {
                agent_id: None,
                context_usage: None,
                context_tokens: None,
                max_context_tokens: None,
                usage: None,
                plan_mode: None,
                swarm_mode: Some(false),
                permission: None,
                model: None,
                thinking_effort: None,
            },
            0,
        );
        assert!(!h.state.app.swarm_mode);
        assert!(h.state.swarm_mode_entry.is_none());
        // The "Swarm ended" marker is rendered — the decision reads the
        // PRE-patch swarm mode (TS snapshots `appState.swarmMode` before
        // applying the patch), not the value just set to false.
        assert!(
            h.state
                .transcript
                .iter()
                .any(|e| e.content == "Swarm ended"),
            "swarm-ended marker should be emitted when a swarm task ends"
        );
    }

    #[test]
    fn routing_agent_status_update_patches_app_state() {
        let mut h = handler();
        h.handle_event(
            Event::AgentStatusUpdated {
                agent_id: None,
                context_usage: Some(0.42),
                context_tokens: Some(4200),
                max_context_tokens: Some(10000),
                usage: Some(json!({"total_tokens": 1})),
                plan_mode: Some(true),
                swarm_mode: Some(false),
                permission: Some(PermissionMode::Yolo),
                model: Some("claude".to_owned()),
                thinking_effort: Some("high".to_owned()),
            },
            0,
        );
        assert_eq!(h.state.app.context_usage, Some(0.42));
        assert_eq!(h.state.app.context_tokens, Some(4200));
        assert_eq!(h.state.app.max_context_tokens, Some(10000));
        assert!(h.state.app.plan_mode);
        assert_eq!(h.state.app.permission_mode, PermissionMode::Yolo);
        assert_eq!(h.state.app.model, "claude");
        assert_eq!(h.state.app.thinking_effort, "high");
    }

    #[test]
    fn routing_hook_result_appends_assistant_markdown_entry() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::HookResult {
                agent_id: None,
                turn_id: "1".to_owned(),
                hook_event: "pre_tool_use".to_owned(),
                content: "  hello  ".to_owned(),
                blocked: false,
            },
            1000,
        );
        let assistant: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::Assistant)
            .collect();
        assert_eq!(assistant.len(), 1);
        assert_eq!(assistant[0].content, "*pre_tool_use hook*\n\nhello");
        assert!(!assistant[0].model_text);
    }

    #[test]
    fn routing_tool_progress_only_when_component_exists() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::ToolProgress {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                update: ToolProgressUpdate {
                    kind: ToolProgressKind::Stdout,
                    text: Some("out".to_owned()),
                },
            },
            0,
        );
        // No component yet → ignored.
        assert!(h.state.tool_live_outputs.is_empty());

        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                name: "Bash".to_owned(),
                args: Args::new(),
                description: None,
            },
            0,
        );
        h.handle_event(
            Event::ToolProgress {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                update: ToolProgressUpdate {
                    kind: ToolProgressKind::Status,
                    text: Some("running".to_owned()),
                },
            },
            0,
        );
        assert_eq!(h.state.tool_live_outputs.get("t1").unwrap().len(), 1);
    }

    #[test]
    fn routing_turn_step_interrupted_by_user() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.handle_event(
            Event::TurnStepInterrupted {
                turn_id: "1".to_owned(),
                step: 0,
                reason: "aborted".to_owned(),
                message: None,
            },
            1500,
        );
        let err = h.state.messages.iter().find_map(|m| match m {
            UiMessage::Status { text, color } => Some((text.clone(), *color)),
            _ => None,
        });
        assert_eq!(
            err,
            Some(("Interrupted by user".to_owned(), Some(ColorToken::Error)))
        );
    }

    #[test]
    fn routing_queued_message_dispatched_on_turn_end() {
        let mut h = handler();
        h.handle_event(turn_started("1"), 1000);
        h.state.queue.push(QueuedMessage::prompt("next please"));
        h.state.app.streaming_phase = StreamingPhase::Composing;
        h.handle_event(
            Event::TurnEnded {
                agent_id: None,
                turn_id: "1".to_owned(),
                reason: TurnEndReason::Completed,
                error: None,
            },
            2000,
        );
        // The queued message was popped; dispatch is pending until the send.
        assert!(h.state.queue.is_empty());
        assert!(h.state.app.queued_message_dispatch_pending);
        assert_eq!(h.state.app.streaming_phase, StreamingPhase::Idle);
    }

    #[test]
    fn format_error_payload_provider_filtered_finish_reason() {
        let mut details = Args::new();
        details.insert("rawFinishReason".to_owned(), json!("content_filter"));
        let payload = ErrorPayload {
            code: "provider.filtered".to_owned(),
            message: "generic".to_owned(),
            details: Some(details),
        };
        assert!(format_error_payload(&payload).contains("finishReason=filtered"));
    }

    #[test]
    fn background_task_transcript_helpers() {
        let mut info = BackgroundTaskInfo::new(
            "t",
            BackgroundTaskKind::Process,
            BackgroundTaskStatus::Running,
        );
        info.detached = Some(false);
        assert!(!should_show_background_task_transcript(&info));

        let mut tool = BackgroundTaskInfo::new(
            "t",
            BackgroundTaskKind::Tool,
            BackgroundTaskStatus::Completed,
        );
        tool.detached = Some(true);
        assert!(!should_show_background_task_transcript(&tool));

        let mut tool_failed =
            BackgroundTaskInfo::new("t", BackgroundTaskKind::Tool, BackgroundTaskStatus::Killed);
        tool_failed.detached = Some(true);
        assert!(should_show_background_task_transcript(&tool_failed));

        let s = format_background_task_transcript(&tool_failed);
        assert!(s.headline.contains("stopped"));
        assert_eq!(s.phase, BackgroundTaskPhase::Failed);
    }

    #[test]
    fn mcp_status_helpers() {
        let servers = vec![
            McpServerStatusSnapshot {
                name: "a".to_owned(),
                status: McpServerStatus::Pending,
                tool_count: 0,
                transport: "sse".to_owned(),
                error: None,
            },
            McpServerStatusSnapshot {
                name: "b".to_owned(),
                status: McpServerStatus::Failed,
                tool_count: 0,
                transport: "stdio".to_owned(),
                error: None,
            },
        ];
        let rows = select_mcp_startup_status_rows(&servers);
        assert_eq!(rows[0].name, "b"); // failed sorts before pending
        let summary = format_mcp_startup_status_summary(&servers);
        assert_eq!(summary, "1 failed, 1 connecting");
        let key = mcp_server_status_key(&servers[0]);
        assert!(key.contains("pending"));
    }

    #[test]
    fn is_plugin_mcp_tool_name_matches_prefix() {
        assert!(is_plugin_mcp_tool_name("mcp__plugin-foo__read"));
        assert!(!is_plugin_mcp_tool_name("mcp__read"));
        assert!(!is_plugin_mcp_tool_name("Bash"));
    }
}
