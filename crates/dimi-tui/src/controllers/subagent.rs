//! Subagent event handling — port of `apps/dimi/src/tui/controllers/
//! subagent-event-handler.ts`.
//!
//! Holds the subagent lifecycle state machine (`subagent.spawned` →
//! `started`/`suspended` → `completed`/`failed`), the child-agent event
//! routing (`routeChildAgentEvent`), the background-agent metadata +
//! transcript bookkeeping, and the AgentSwarm progress state machine.
//!
//! Component operations (`ToolCallComponent.setSubagentMeta`,
//! `appendSubagentText`, `AgentSwarmProgressComponent`, …) are
//! `// TODO(legacy)`.

use std::collections::BTreeMap;

use regex::Regex;

use crate::controllers::btw::BtwPanelController;
use crate::controllers::event_handler::{
    BackgroundAgentExtras, UiState, format_background_agent_transcript,
};
use crate::controllers::events::{Event, MAIN_AGENT_ID};
use crate::controllers::streaming::ToolCallPreview;
use crate::controllers::{
    Args, BackgroundAgentMetadata, BackgroundTaskKind, BackgroundTaskPhase, BackgroundTaskStatus,
    RenderMode, ToolCallBlockData, TranscriptEntry, TranscriptEntryKind,
};

/// One cell of an AgentSwarm progress panel (a child subagent).
#[derive(Debug, Clone, PartialEq)]
pub struct SwarmSubagentState {
    pub swarm_index: Option<u32>,
    pub phase: SwarmSubagentPhase,
    pub text: String,
    pub tool_call_ids: Vec<String>,
}

/// `AgentSwarmProgressComponent` cell phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmSubagentPhase {
    Queued,
    Started,
    Suspended,
    Completed,
    Cancelled,
    Failed,
}

/// Pure AgentSwarm progress state (the component rendering is legacy).
#[derive(Debug, Clone, PartialEq)]
pub struct SwarmProgress {
    pub description: String,
    pub input_complete: bool,
    pub tool_call_ended: bool,
    pub active_cancelled: bool,
    pub failed: bool,
    pub model_display: Option<String>,
    pub streaming_arguments: Option<String>,
    pub args: Args,
    pub subagents: BTreeMap<String, SwarmSubagentState>,
}

impl SwarmProgress {
    pub fn new(description: String) -> Self {
        SwarmProgress {
            description,
            input_complete: false,
            tool_call_ended: false,
            active_cancelled: false,
            failed: false,
            model_display: None,
            streaming_arguments: None,
            args: Args::new(),
            subagents: BTreeMap::new(),
        }
    }

    /// `isRequestStreaming` — args are still streaming in.
    pub fn is_request_streaming(&self) -> bool {
        !self.input_complete
    }

    /// `isToolCallActive`.
    pub fn is_tool_call_active(&self) -> bool {
        !self.tool_call_ended
    }

    pub fn mark_input_complete(&mut self) {
        self.input_complete = true;
    }

    pub fn mark_tool_call_ended(&mut self) {
        self.tool_call_ended = true;
    }

    pub fn mark_active_cancelled(&mut self) {
        self.active_cancelled = true;
    }

    pub fn mark_swarm_failed(&mut self, output: &str) {
        self.failed = true;
        let _ = output;
    }

    /// `applyResult` — true when the result output parses as the structured
    /// swarm result (XML `<subagent … outcome=…>` tags, or the legacy
    /// `[agent N]` block form); false means the swarm failed to produce one.
    pub fn apply_result(&mut self, output: &str) -> bool {
        !parse_agent_swarm_result_statuses(output).is_empty()
    }

    /// `updateArgs`.
    pub fn update_args(&mut self, args: Args, streaming_arguments: Option<String>) {
        self.args = args;
        self.streaming_arguments = streaming_arguments;
    }

    /// `registerSubagent`.
    pub fn register_subagent(&mut self, agent_id: &str, swarm_index: Option<u32>) {
        self.subagents.insert(
            agent_id.to_owned(),
            SwarmSubagentState {
                swarm_index,
                phase: SwarmSubagentPhase::Queued,
                text: String::new(),
                tool_call_ids: Vec::new(),
            },
        );
    }

    pub fn mark_started(&mut self, agent_id: &str) {
        if let Some(cell) = self.subagents.get_mut(agent_id) {
            cell.phase = SwarmSubagentPhase::Started;
        }
    }

    pub fn mark_suspended(&mut self, agent_id: &str) {
        if let Some(cell) = self.subagents.get_mut(agent_id) {
            cell.phase = SwarmSubagentPhase::Suspended;
        }
    }

    pub fn mark_completed(&mut self, agent_id: &str, _result_summary: Option<&str>) {
        if let Some(cell) = self.subagents.get_mut(agent_id) {
            cell.phase = SwarmSubagentPhase::Completed;
        }
    }

    pub fn mark_cancelled(&mut self, agent_id: &str) {
        if let Some(cell) = self.subagents.get_mut(agent_id) {
            cell.phase = SwarmSubagentPhase::Cancelled;
        }
    }

    pub fn mark_failed(&mut self, agent_id: &str, _error: &str) {
        if let Some(cell) = self.subagents.get_mut(agent_id) {
            cell.phase = SwarmSubagentPhase::Failed;
        }
    }

    /// `appendModelDelta`.
    pub fn append_model_delta(&mut self, agent_id: &str, delta: &str) {
        if let Some(cell) = self.subagents.get_mut(agent_id) {
            cell.text.push_str(delta);
        }
    }

    /// `recordToolCall`.
    pub fn record_tool_call(&mut self, agent_id: &str, tool_call_id: &str) {
        if let Some(cell) = self.subagents.get_mut(agent_id) {
            cell.tool_call_ids.push(tool_call_id.to_owned());
        }
    }

    /// `setModelDisplay`.
    pub fn set_model_display(&mut self, display: String) {
        self.model_display = Some(display);
    }
}

/// The subagent event handler (port of `SubAgentEventHandler`).
///
/// Cross-cutting state that replay/session bookkeeping also touches
/// (`subagentInfo`, `backgroundAgentMetadata`) lives on [`UiState`]; the
/// handler owns only the AgentSwarm progress map.
#[derive(Debug, Default)]
pub struct SubAgentHandler {
    agent_swarm_progress: BTreeMap<String, SwarmProgress>,
}

impl SubAgentHandler {
    pub fn new() -> Self {
        SubAgentHandler::default()
    }

    pub fn has_agent_swarm_progress(&self, tool_call_id: &str) -> bool {
        self.agent_swarm_progress.contains_key(tool_call_id)
    }

    pub fn has_active_agent_swarm_tool_call(&self) -> bool {
        self.agent_swarm_progress
            .values()
            .any(|p| p.is_tool_call_active())
    }

    pub fn swarm_progress(&self, tool_call_id: &str) -> Option<&SwarmProgress> {
        self.agent_swarm_progress.get(tool_call_id)
    }

    /// `resetRuntimeState`.
    pub fn reset_runtime_state(&mut self, state: &mut UiState) {
        state.subagent_info.clear();
        state.background_agent_metadata.clear();
        self.clear_agent_swarm_progress();
    }

    /// `clearAgentSwarmProgress`.
    pub fn clear_agent_swarm_progress(&mut self) {
        // TODO(legacy): dispose each AgentSwarmProgressComponent
        self.agent_swarm_progress.clear();
    }

    /// `routeChildAgentEvent` — route a non-lifecycle event to the matching
    /// child-agent card / swarm panel / btw panel. Returns true when consumed.
    pub fn route_child_agent_event(
        &mut self,
        event: &Event,
        btw: &mut BtwPanelController,
        state: &mut UiState,
    ) -> bool {
        if is_subagent_lifecycle_event(event) {
            return false;
        }
        let Some(child_agent_id) = event.agent_id() else {
            return false;
        };
        if child_agent_id == MAIN_AGENT_ID {
            return false;
        }
        if btw.route_event(event) {
            return true;
        }
        let Some(info) = state.subagent_info.get(child_agent_id) else {
            // No remembered subagent → the event is not ours to render.
            return true;
        };
        let parent_tool_call_id = info.parent_tool_call_id.clone();
        if parent_tool_call_id.is_empty() {
            return true;
        }
        if let Some(progress) = self.agent_swarm_progress.get_mut(&parent_tool_call_id) {
            apply_subagent_event_to_swarm_progress(progress, event, child_agent_id);
            return true;
        }
        let streaming = &mut state.streaming;
        if !streaming.has_tool_component(&parent_tool_call_id) {
            return true;
        }
        // TODO(legacy): toolCall.setSubagentMeta(childAgentId, info.name)
        streaming.set_tool_call_subagent_meta(&parent_tool_call_id, child_agent_id, &info.name);
        // TODO(legacy): per-event-type component updates —
        //   hook.result → appendSubagentText(plain, 'text')
        //   assistant.delta → appendSubagentText(delta, 'text')
        //   thinking.delta → appendSubagentText(delta, 'thinking')
        //   tool.call.started → appendSubToolCall
        //   tool.call.delta → appendSubToolCallDelta
        //   tool.progress → appendSubToolLiveOutput
        //   tool.result → finishSubToolCall
        //   agent.status.updated → updateSubagentMetrics
        true
    }

    /// `handleLifecycleEvent` — dispatch `subagent.*` events.
    pub fn handle_lifecycle_event(&mut self, state: &mut UiState, event: Event) {
        match event {
            Event::SubagentSpawned {
                subagent_id,
                parent_tool_call_id,
                subagent_name,
                run_in_background,
                swarm_index,
                description,
            } => self.handle_subagent_spawned(
                state,
                &subagent_id,
                &parent_tool_call_id,
                &subagent_name,
                run_in_background,
                swarm_index,
                description.as_deref(),
            ),
            Event::SubagentStarted { subagent_id } => {
                self.handle_subagent_started(state, &subagent_id);
            }
            Event::SubagentSuspended {
                subagent_id,
                reason,
            } => {
                self.handle_subagent_suspended(state, &subagent_id, reason.as_deref());
            }
            Event::SubagentCompleted {
                subagent_id,
                result_summary,
                ..
            } => self.handle_subagent_completed(state, &subagent_id, result_summary.as_deref()),
            Event::SubagentFailed { subagent_id, error } => {
                self.handle_subagent_failed(state, &subagent_id, &error);
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // AgentSwarm progress
    // -----------------------------------------------------------------------

    /// `handleAgentSwarmToolCallStarted`.
    pub fn handle_agent_swarm_tool_call_started(
        &mut self,
        state: &mut UiState,
        tool_call_id: &str,
        args: &Args,
        now_ms: u64,
    ) {
        let progress =
            self.ensure_agent_swarm_progress(state, tool_call_id, args.clone(), None, now_ms);
        progress.mark_input_complete();
    }

    /// `handleAgentSwarmToolCallDelta`.
    pub fn handle_agent_swarm_tool_call_delta(
        &mut self,
        state: &mut UiState,
        tool_call_id: &str,
        preview: &ToolCallPreview,
        now_ms: u64,
    ) {
        self.ensure_agent_swarm_progress(
            state,
            tool_call_id,
            preview.args.clone(),
            Some(preview.arguments_text.clone()),
            now_ms,
        );
    }

    /// `handleAgentSwarmToolResult`.
    pub fn handle_agent_swarm_tool_result(
        &mut self,
        _state: &mut UiState,
        tool_call_id: &str,
        output: &str,
        is_error: bool,
    ) {
        let should_remove = {
            let Some(progress) = self.agent_swarm_progress.get_mut(tool_call_id) else {
                return;
            };
            if is_error && is_user_cancelled_subagent_error(output) {
                if progress.is_request_streaming() {
                    true
                } else {
                    progress.mark_tool_call_ended();
                    progress.mark_active_cancelled();
                    false
                }
            } else if is_error {
                progress.mark_tool_call_ended();
                if !progress.apply_result(output) {
                    progress.mark_swarm_failed(output);
                }
                false
            } else {
                progress.mark_tool_call_ended();
                progress.apply_result(output);
                false
            }
        };
        if should_remove {
            // TODO(legacy): dispose progress + remove from transcriptContainer
            self.agent_swarm_progress.remove(tool_call_id);
        }
    }

    /// `markActiveAgentSwarmsCancelled`.
    pub fn mark_active_agent_swarms_cancelled(&mut self) {
        let mut updated = false;
        let mut to_remove: Vec<String> = Vec::new();
        for (tool_call_id, progress) in self.agent_swarm_progress.iter_mut() {
            if progress.is_request_streaming() {
                to_remove.push(tool_call_id.clone());
            } else {
                progress.mark_active_cancelled();
            }
            updated = true;
        }
        for id in to_remove {
            // TODO(legacy): dispose progress + remove from transcriptContainer
            self.agent_swarm_progress.remove(&id);
        }
        let _ = updated;
    }

    fn ensure_agent_swarm_progress(
        &mut self,
        state: &mut UiState,
        tool_call_id: &str,
        args: Args,
        streaming_arguments: Option<String>,
        now_ms: u64,
    ) -> &mut SwarmProgress {
        if self.agent_swarm_progress.contains_key(tool_call_id) {
            let progress = self
                .agent_swarm_progress
                .get_mut(tool_call_id)
                .expect("checked");
            progress.update_args(args, streaming_arguments);
            return progress;
        }
        let description = agent_swarm_description_from_args(&args);
        let mut progress = SwarmProgress::new(description);
        progress.update_args(args, streaming_arguments);
        self.agent_swarm_progress
            .insert(tool_call_id.to_owned(), progress);
        // TODO(legacy): streamingUI.finalizeLiveTextBuffers('tool');
        //   addChild(progress); host.updateActivityPane(); requestRender()
        state
            .streaming
            .finalize_live_text_buffers(&mut state.effects, now_ms);
        self.agent_swarm_progress
            .get_mut(tool_call_id)
            .expect("just inserted")
    }

    fn update_agent_swarm_progress(
        &mut self,
        parent_tool_call_id: &str,
        update: impl FnOnce(&mut SwarmProgress),
    ) -> bool {
        let Some(progress) = self.agent_swarm_progress.get_mut(parent_tool_call_id) else {
            return false;
        };
        update(progress);
        true
    }

    // -----------------------------------------------------------------------
    // Lifecycle handlers
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn handle_subagent_spawned(
        &mut self,
        state: &mut UiState,
        subagent_id: &str,
        parent_tool_call_id: &str,
        subagent_name: &str,
        run_in_background: bool,
        swarm_index: Option<u32>,
        description: Option<&str>,
    ) {
        state.subagent_info.insert(
            subagent_id.to_owned(),
            crate::controllers::SubagentInfo {
                parent_tool_call_id: parent_tool_call_id.to_owned(),
                name: subagent_name.to_owned(),
                run_in_background,
                swarm_index,
            },
        );

        if run_in_background {
            let meta = self.build_background_agent_metadata(
                state,
                subagent_id,
                parent_tool_call_id,
                subagent_name,
                description,
            );
            state
                .background_agent_metadata
                .insert(subagent_id.to_owned(), meta.clone());
            self.append_background_agent_entry(state, BackgroundTaskPhase::Started, &meta, None);
            // TODO(legacy): deps.syncBackgroundAgentBadge()
            return;
        }

        self.handle_foreground_subagent_spawned(
            state,
            subagent_id,
            parent_tool_call_id,
            subagent_name,
            swarm_index,
            description,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_foreground_subagent_spawned(
        &mut self,
        state: &mut UiState,
        subagent_id: &str,
        parent_tool_call_id: &str,
        subagent_name: &str,
        swarm_index: Option<u32>,
        description: Option<&str>,
    ) {
        if self.update_agent_swarm_progress(parent_tool_call_id, |progress| {
            progress.register_subagent(subagent_id, swarm_index);
        }) {
            return;
        }

        if !get_or_activate_tool_component(state, parent_tool_call_id) {
            // Standalone Agent card for a subagent that spawned without a
            // preceding Agent tool call (`createStandaloneSubagentToolCall`).
            let fallback = description
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Run {subagent_name} agent"));
            let (turn_id, step) = state.streaming.get_turn_context();
            let mut args = Args::new();
            args.insert(
                "description".to_owned(),
                serde_json::Value::String(fallback.clone()),
            );
            args.insert(
                "subagent_type".to_owned(),
                serde_json::Value::String(subagent_name.to_owned()),
            );
            let tool_call = ToolCallBlockData {
                id: parent_tool_call_id.to_owned(),
                name: "Agent".to_owned(),
                args,
                description: Some(fallback),
                streaming_arguments: None,
                streaming_started_at_ms: None,
                result: None,
                step: Some(step),
                turn_id: turn_id.map(str::to_owned),
                truncated: false,
            };
            state
                .streaming
                .on_tool_call_start(&mut state.effects, &tool_call);
            if !state.streaming.has_tool_component(parent_tool_call_id) {
                return;
            }
        }
        state
            .streaming
            .tool_call_subagent_spawned(parent_tool_call_id, subagent_id);
        // TODO(legacy): tc.onSubagentSpawned({ agentId, agentName, runInBackground })
    }

    fn handle_subagent_started(&mut self, state: &mut UiState, subagent_id: &str) {
        let Some(info) = state.subagent_info.get(subagent_id).cloned() else {
            return;
        };
        if info.run_in_background {
            return;
        }
        if self.update_agent_swarm_progress(&info.parent_tool_call_id, |progress| {
            progress.mark_started(subagent_id);
        }) {
            return;
        }
        if get_or_activate_tool_component(state, &info.parent_tool_call_id) {
            state
                .streaming
                .tool_call_subagent_started(&info.parent_tool_call_id);
            // TODO(legacy): tc.onSubagentStarted({ agentId, agentName, runInBackground })
        }
    }

    fn handle_subagent_suspended(
        &mut self,
        state: &mut UiState,
        subagent_id: &str,
        _reason: Option<&str>,
    ) {
        let Some(info) = state.subagent_info.get(subagent_id).cloned() else {
            return;
        };
        if info.run_in_background {
            return;
        }
        self.update_agent_swarm_progress(&info.parent_tool_call_id, |progress| {
            progress.mark_suspended(subagent_id);
        });
        // TODO(legacy): tc.onSubagentSuspended({ agentId, reason, swarmIndex })
    }

    fn handle_subagent_completed(
        &mut self,
        state: &mut UiState,
        subagent_id: &str,
        result_summary: Option<&str>,
    ) {
        let background_meta = state.background_agent_metadata.remove(subagent_id);
        if let Some(meta) = background_meta {
            let task_id = find_agent_task_id(subagent_id, &meta, &state.background_tasks);
            // TODO(legacy): deps.syncBackgroundAgentBadge()
            if let Some(task_id) = &task_id {
                if state
                    .background_tasks_transcripted_terminal
                    .contains(task_id)
                {
                    return;
                }
                state
                    .background_tasks_transcripted_terminal
                    .insert(task_id.clone());
            }
            let extras = result_summary.map(|summary| BackgroundAgentExtras {
                result_summary: Some(summary.to_owned()),
                error: None,
            });
            self.append_background_agent_entry(
                state,
                BackgroundTaskPhase::Completed,
                &meta,
                extras.as_ref(),
            );
            return;
        }

        let Some(info) = state.subagent_info.get(subagent_id).cloned() else {
            return;
        };
        if info.run_in_background {
            return;
        }
        if self.update_agent_swarm_progress(&info.parent_tool_call_id, |progress| {
            progress.mark_completed(subagent_id, result_summary);
        }) {
            state
                .streaming
                .remove_tool_component_if_inactive(&info.parent_tool_call_id);
            return;
        }
        if state
            .streaming
            .has_tool_component(&info.parent_tool_call_id)
        {
            state
                .streaming
                .tool_call_subagent_completed(&info.parent_tool_call_id);
            // TODO(legacy): tc.onSubagentCompleted({ contextTokens, usage, resultSummary })
        }
        state
            .streaming
            .remove_tool_component_if_inactive(&info.parent_tool_call_id);
    }

    fn handle_subagent_failed(&mut self, state: &mut UiState, subagent_id: &str, error: &str) {
        let background_meta = state.background_agent_metadata.remove(subagent_id);
        if let Some(meta) = background_meta {
            let task_id = find_agent_task_id(subagent_id, &meta, &state.background_tasks);
            let task = task_id
                .as_ref()
                .and_then(|id| state.background_tasks.get(id))
                .cloned();
            // TODO(legacy): deps.syncBackgroundAgentBadge()
            if let Some(task) = &task {
                if task.kind == BackgroundTaskKind::Agent
                    && task.status == BackgroundTaskStatus::TimedOut
                {
                    return;
                }
            }
            state.streaming.apply_background_task_terminal_status(
                &crate::controllers::streaming::ApplyTerminalStatusArgs {
                    agent_id: Some(subagent_id.to_owned()),
                    description: meta.description.clone().unwrap_or_default(),
                    status: BackgroundTaskStatus::Failed,
                    error_text: Some(error.to_owned()),
                },
            );
            if let Some(task_id) = &task_id {
                if state
                    .background_tasks_transcripted_terminal
                    .contains(task_id)
                {
                    return;
                }
                state
                    .background_tasks_transcripted_terminal
                    .insert(task_id.clone());
            }
            let extras = BackgroundAgentExtras {
                result_summary: None,
                error: Some(error.to_owned()),
            };
            self.append_background_agent_entry(
                state,
                BackgroundTaskPhase::Failed,
                &meta,
                Some(&extras),
            );
            return;
        }

        let Some(info) = state.subagent_info.get(subagent_id).cloned() else {
            return;
        };
        if info.run_in_background {
            return;
        }
        if self.update_agent_swarm_progress(&info.parent_tool_call_id, |progress| {
            mark_agent_swarm_failed_or_cancelled(progress, subagent_id, error);
        }) {
            state
                .streaming
                .remove_tool_component_if_inactive(&info.parent_tool_call_id);
            return;
        }
        if state
            .streaming
            .has_tool_component(&info.parent_tool_call_id)
        {
            state
                .streaming
                .tool_call_subagent_failed(&info.parent_tool_call_id);
            // TODO(legacy): tc.onSubagentFailed({ error })
        }
        state
            .streaming
            .remove_tool_component_if_inactive(&info.parent_tool_call_id);
    }

    fn build_background_agent_metadata(
        &self,
        state: &UiState,
        subagent_id: &str,
        parent_tool_call_id: &str,
        subagent_name: &str,
        description: Option<&str>,
    ) -> BackgroundAgentMetadata {
        let parent = state.streaming.get_active_tool_call(parent_tool_call_id);
        let description = parent
            .and_then(|tc| tc.args.get("description"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .or_else(|| description.map(str::to_owned));
        BackgroundAgentMetadata {
            agent_id: subagent_id.to_owned(),
            parent_tool_call_id: parent_tool_call_id.to_owned(),
            agent_name: Some(subagent_name.to_owned()),
            description,
        }
    }

    fn append_background_agent_entry(
        &self,
        state: &mut UiState,
        phase: BackgroundTaskPhase,
        meta: &BackgroundAgentMetadata,
        extras: Option<&BackgroundAgentExtras>,
    ) {
        let status = format_background_agent_transcript(phase, meta, extras);
        let turn_id = state.streaming.get_turn_context().0.map(str::to_owned);
        let mut entry = TranscriptEntry::new(
            state.next_entry_id(),
            TranscriptEntryKind::Status,
            turn_id,
            RenderMode::Plain,
            status.headline.clone(),
        );
        entry.detail = status.detail.clone();
        entry.background_agent_status = Some(status);
        state.transcript.push(entry);
    }
}

/// `getOrActivateToolComponent` — returns true when the card exists.
fn get_or_activate_tool_component(state: &mut UiState, parent_tool_call_id: &str) -> bool {
    if state.streaming.has_tool_component(parent_tool_call_id) {
        return true;
    }
    let Some(tool_call) = state
        .streaming
        .get_active_tool_call(parent_tool_call_id)
        .cloned()
    else {
        return false;
    };
    state
        .streaming
        .on_tool_call_start(&mut state.effects, &tool_call);
    state.streaming.has_tool_component(parent_tool_call_id)
}

/// `applySubagentEventToSwarmProgress`.
fn apply_subagent_event_to_swarm_progress(
    progress: &mut SwarmProgress,
    event: &Event,
    subagent_id: &str,
) {
    match event {
        Event::AssistantDelta { delta, .. } | Event::ThinkingDelta { delta, .. } => {
            progress.append_model_delta(subagent_id, delta);
        }
        Event::ToolCallStarted { tool_call_id, .. } => {
            progress.record_tool_call(subagent_id, tool_call_id);
        }
        Event::AgentStatusUpdated {
            model: Some(model), ..
        } => {
            // TODO(legacy): modelDisplayName(model, availableModels[model])
            progress.set_model_display(model.clone());
        }
        _ => {}
    }
}

/// `markAgentSwarmFailedOrCancelled`.
fn mark_agent_swarm_failed_or_cancelled(
    progress: &mut SwarmProgress,
    subagent_id: &str,
    error: &str,
) {
    if is_user_cancelled_subagent_error(error) {
        progress.mark_cancelled(subagent_id);
    } else {
        progress.mark_failed(subagent_id, error);
    }
}

/// `isSubagentLifecycleEvent`.
fn is_subagent_lifecycle_event(event: &Event) -> bool {
    matches!(
        event,
        Event::SubagentSpawned { .. }
            | Event::SubagentStarted { .. }
            | Event::SubagentSuspended { .. }
            | Event::SubagentCompleted { .. }
            | Event::SubagentFailed { .. }
    )
}

/// `isUserCancelledSubagentError`.
fn is_user_cancelled_subagent_error(error: &str) -> bool {
    matches!(
        error.trim(),
        "Aborted by the user" | "The user manually interrupted this subagent batch."
    )
}

/// `agentSwarmDescriptionFromArgs`.
pub fn agent_swarm_description_from_args(args: &Args) -> String {
    args.get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default()
}

/// `AgentSwarmResultStatus['status']` — the per-member outcome parsed from a
/// swarm tool result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmResultStatus {
    Completed,
    Failed,
    Cancelled,
}

/// One parsed swarm result row (`AgentSwarmResultStatus`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmResultEntry {
    pub index: u32,
    pub status: SwarmResultStatus,
}

/// `parseAgentSwarmResultStatuses` — try the XML `<subagent>` form first,
/// then fall back to the legacy `[agent N]` block form (port of the TS
/// helper in `agent-swarm-progress.ts`).
pub fn parse_agent_swarm_result_statuses(output: &str) -> Vec<SwarmResultEntry> {
    let xml = parse_agent_swarm_xml_result_statuses(output);
    if !xml.is_empty() {
        return xml;
    }
    parse_agent_swarm_legacy_result_statuses(output)
}

/// `forEachSubagentTag` + `parseAgentSwarmXmlResultStatuses` — collect every
/// `<subagent … outcome="completed|failed|aborted|cancelled">` row. The body
/// between the open and close tags is ignored here (it is rendered by the
/// legacy component); the decision is which statuses are present.
fn parse_agent_swarm_xml_result_statuses(output: &str) -> Vec<SwarmResultEntry> {
    let tag_re = Regex::new(r#"<subagent\b([^>]*)>"#).expect("valid subagent tag regex");
    let mut entries = Vec::new();
    let mut search_from = 0usize;
    let mut tag_index = 0u32;
    while let Some(cap) = tag_re.captures(&output[search_from..]) {
        let whole = cap.get(0).expect("whole match");
        let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let open_end = search_from + whole.end();
        let Some(close_rel) = output[open_end..].find("</subagent>") else {
            break;
        };
        let close_end = open_end + close_rel + "</subagent>".len();
        tag_index += 1;
        let status = xml_attribute(attrs, "outcome").and_then(|o| swarm_status_from_outcome(&o));
        if let Some(status) = status {
            let explicit_index = xml_attribute(attrs, "index")
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|i| *i > 0);
            entries.push(SwarmResultEntry {
                index: explicit_index.unwrap_or(tag_index),
                status,
            });
        }
        search_from = close_end;
    }
    entries
}

/// `xmlAttribute` — read a quoted attribute from a tag's attribute string.
fn xml_attribute(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"\b{name}="([^"]*)""#);
    let re = Regex::new(&pattern).ok()?;
    re.captures(attrs)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

/// `forEachAgentBlock` + `parseAgentSwarmLegacyResultStatuses` — the
/// pre-XML `[agent N]\nstatus: …` block form.
fn parse_agent_swarm_legacy_result_statuses(output: &str) -> Vec<SwarmResultEntry> {
    let block_re = Regex::new(r"(?m)^\[agent (\d+)\]$").expect("valid agent-block regex");
    let status_re = Regex::new(r"(?m)^status: (completed|failed|aborted|cancelled)$")
        .expect("valid status regex");
    let blocks: Vec<(usize, u32)> = block_re
        .captures_iter(output)
        .filter_map(|cap| {
            let start = cap.get(0)?.start();
            let index = cap.get(1)?.as_str().parse::<u32>().ok()?;
            Some((start, index))
        })
        .collect();
    let mut entries = Vec::new();
    for (i, (block_start, index)) in blocks.iter().enumerate() {
        let block_end = blocks
            .get(i + 1)
            .map(|(start, _)| *start)
            .unwrap_or(output.len());
        let block = &output[*block_start..block_end];
        if let Some(status_cap) = status_re.captures(block) {
            let outcome = status_cap.get(1).map(|m| m.as_str()).unwrap_or("");
            if let Some(status) = swarm_status_from_outcome(outcome) {
                entries.push(SwarmResultEntry {
                    index: *index,
                    status,
                });
            }
        }
    }
    entries
}

/// Map the XML/legacy `outcome`/`status` value to a [`SwarmResultStatus`];
/// `aborted`/`cancelled` both collapse to `cancelled` (matching TS).
fn swarm_status_from_outcome(outcome: &str) -> Option<SwarmResultStatus> {
    match outcome {
        "completed" => Some(SwarmResultStatus::Completed),
        "failed" => Some(SwarmResultStatus::Failed),
        "aborted" | "cancelled" => Some(SwarmResultStatus::Cancelled),
        _ => None,
    }
}

/// `findAgentTaskId` — agent-id match first, description fallback (ambiguous
/// descriptions resolve to `None`).
pub fn find_agent_task_id(
    subagent_id: &str,
    meta: &BackgroundAgentMetadata,
    background_tasks: &BTreeMap<String, crate::controllers::BackgroundTaskInfo>,
) -> Option<String> {
    for info in background_tasks.values() {
        if info.kind != BackgroundTaskKind::Agent {
            continue;
        }
        if info.agent_id.as_deref() == Some(subagent_id) {
            return Some(info.task_id.clone());
        }
    }
    let description = meta.description.as_deref().or(meta.agent_name.as_deref());
    let description = description?;
    let mut matched: Option<String> = None;
    for info in background_tasks.values() {
        if info.kind != BackgroundTaskKind::Agent {
            continue;
        }
        if info.description.as_deref() != Some(description) {
            continue;
        }
        if matched.is_some() {
            return None;
        }
        matched = Some(info.task_id.clone());
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::event_handler::SessionEventHandler;
    use crate::controllers::events::TurnEndReason;
    use serde_json::json;

    fn handler() -> SessionEventHandler {
        SessionEventHandler::new()
    }

    #[test]
    fn route_child_agent_event_attaches_delta_to_parent_card() {
        let mut h = handler();
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "child".to_owned(),
                parent_tool_call_id: "card".to_owned(),
                subagent_name: "coder".to_owned(),
                run_in_background: false,
                swarm_index: None,
                description: None,
            },
            1000,
        );
        assert!(h.state.streaming.has_tool_component("card"));

        // A child assistant delta is consumed (routed) — not re-dispatched.
        h.handle_event(
            Event::AssistantDelta {
                agent_id: Some("child".to_owned()),
                delta: "hi".to_owned(),
            },
            1100,
        );
        // The card records the subagent meta.
        let view = h.state.streaming.tool_call_subagent_view("card");
        assert_eq!(view.unwrap().agent_id.as_deref(), Some("child"));
        // The main streaming draft was NOT touched.
        assert!(!h.state.streaming.has_streaming_block());
    }

    #[test]
    fn main_agent_events_are_not_routed() {
        let mut h = handler();
        h.handle_event(
            Event::AssistantDelta {
                agent_id: Some("main".to_owned()),
                delta: "hi".to_owned(),
            },
            0,
        );
        assert!(h.state.streaming.has_streaming_block());
    }

    #[test]
    fn subagent_lifecycle_background_started_completed() {
        let mut h = handler();
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "bg-agent".to_owned(),
                parent_tool_call_id: "p".to_owned(),
                subagent_name: "worker".to_owned(),
                run_in_background: true,
                swarm_index: None,
                description: Some("background task".to_owned()),
            },
            1000,
        );
        assert!(h.state.background_agent_metadata.contains_key("bg-agent"));
        let statuses: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::Status)
            .collect();
        assert_eq!(statuses.len(), 1);
        assert!(
            statuses[0]
                .content
                .contains("worker agent started in background")
        );
        assert_eq!(
            statuses[0].background_agent_status.as_ref().unwrap().phase,
            BackgroundTaskPhase::Started
        );

        h.handle_event(
            Event::SubagentCompleted {
                subagent_id: "bg-agent".to_owned(),
                result_summary: Some("did work".to_owned()),
                context_tokens: None,
                usage: None,
            },
            2000,
        );
        assert!(!h.state.background_agent_metadata.contains_key("bg-agent"));
        let statuses: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::Status)
            .collect();
        assert_eq!(statuses.len(), 2);
        assert!(
            statuses[1]
                .content
                .contains("worker agent completed in background")
        );
    }

    #[test]
    fn foreground_subagent_started_marks_card_running() {
        let mut h = handler();
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "child".to_owned(),
                parent_tool_call_id: "card".to_owned(),
                subagent_name: "coder".to_owned(),
                run_in_background: false,
                swarm_index: None,
                description: None,
            },
            1000,
        );
        h.handle_event(
            Event::SubagentStarted {
                subagent_id: "child".to_owned(),
            },
            1100,
        );
        let view = h.state.streaming.tool_call_subagent_view("card");
        assert_eq!(
            view.unwrap().phase,
            crate::controllers::streaming::SubagentCardPhase::Running
        );
    }

    #[test]
    fn subagent_failed_surfaces_error_on_background_card() {
        let mut h = handler();
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "bg-agent".to_owned(),
                parent_tool_call_id: "p".to_owned(),
                subagent_name: "worker".to_owned(),
                run_in_background: true,
                swarm_index: None,
                description: Some("d".to_owned()),
            },
            1000,
        );
        h.handle_event(
            Event::SubagentFailed {
                subagent_id: "bg-agent".to_owned(),
                error: "crashed".to_owned(),
            },
            2000,
        );
        let statuses: Vec<&TranscriptEntry> = h
            .state
            .transcript
            .iter()
            .filter(|e| e.kind == TranscriptEntryKind::Status)
            .collect();
        assert_eq!(statuses.len(), 2);
        assert!(
            statuses[1]
                .content
                .contains("worker agent failed in background")
        );
        assert!(
            statuses[1]
                .detail
                .as_deref()
                .unwrap_or("")
                .contains("crashed")
        );
    }

    #[test]
    fn find_agent_task_id_prefers_agent_id() {
        let mut tasks = BTreeMap::new();
        let mut info = crate::controllers::BackgroundTaskInfo::new(
            "t1",
            BackgroundTaskKind::Agent,
            BackgroundTaskStatus::Running,
        );
        info.agent_id = Some("a1".to_owned());
        info.description = Some("shared".to_owned());
        tasks.insert("t1".to_owned(), info);
        let mut other = crate::controllers::BackgroundTaskInfo::new(
            "t2",
            BackgroundTaskKind::Agent,
            BackgroundTaskStatus::Running,
        );
        other.description = Some("shared".to_owned());
        tasks.insert("t2".to_owned(), other);

        let meta = BackgroundAgentMetadata {
            agent_id: "a1".to_owned(),
            parent_tool_call_id: "p".to_owned(),
            agent_name: None,
            description: Some("shared".to_owned()),
        };
        // agent-id match wins over the ambiguous description.
        assert_eq!(
            find_agent_task_id("a1", &meta, &tasks).as_deref(),
            Some("t1")
        );
        // Description-only fallback with two matches → ambiguous (None).
        let meta2 = BackgroundAgentMetadata {
            agent_id: "a2".to_owned(),
            parent_tool_call_id: "p".to_owned(),
            agent_name: None,
            description: Some("shared".to_owned()),
        };
        assert_eq!(find_agent_task_id("a2", &meta2, &tasks), None);
    }

    #[test]
    fn swarm_progress_state_machine() {
        let mut h = handler();
        // The AgentSwarm tool call starts first — that creates the panel.
        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "swarm".to_owned(),
                name: "AgentSwarm".to_owned(),
                args: crate::controllers::args_json(json!({"description": "batch"})),
                description: Some("batch".to_owned()),
            },
            1000,
        );
        assert!(h.subagent.has_agent_swarm_progress("swarm"));
        let progress = h.subagent.swarm_progress("swarm").unwrap();
        assert!(progress.input_complete);
        assert!(progress.is_tool_call_active());

        // A spawn alone does not create the panel; with the panel live, the
        // spawn registers the subagent cell into it.
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "s1".to_owned(),
                parent_tool_call_id: "swarm".to_owned(),
                subagent_name: "coder".to_owned(),
                run_in_background: false,
                swarm_index: Some(0),
                description: Some("batch".to_owned()),
            },
            1050,
        );
        assert!(
            h.subagent
                .swarm_progress("swarm")
                .unwrap()
                .subagents
                .contains_key("s1")
        );

        // Child deltas accumulate onto the swarm cell.
        h.handle_event(
            Event::AssistantDelta {
                agent_id: Some("s1".to_owned()),
                delta: "hello".to_owned(),
            },
            1200,
        );
        let progress = h.subagent.swarm_progress("swarm").unwrap();
        assert_eq!(progress.subagents.get("s1").unwrap().text, "hello");

        // Tool result ends the swarm tool call. A structured swarm XML result
        // parses (TS `applyResult`) — the non-error path does not mark the
        // swarm failed regardless, but the output shape is the real one.
        h.handle_event(
            Event::ToolResult {
                agent_id: None,
                tool_call_id: "swarm".to_owned(),
                output: json!(
                    "<agent_swarm_result><subagent index=\"1\" outcome=\"completed\">done</subagent></agent_swarm_result>"
                ),
                is_error: false,
                synthetic: false,
            },
            1300,
        );
        let progress = h.subagent.swarm_progress("swarm").unwrap();
        assert!(!progress.is_tool_call_active());
        assert!(!progress.failed);
    }

    #[test]
    fn swarm_structured_xml_error_result_is_parsed_not_failed() {
        let mut h = handler();
        // The AgentSwarm tool call starts first — that creates the panel.
        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "swarm".to_owned(),
                name: "AgentSwarm".to_owned(),
                args: crate::controllers::args_json(json!({"description": "batch"})),
                description: Some("batch".to_owned()),
            },
            1000,
        );
        assert!(h.subagent.has_agent_swarm_progress("swarm"));

        // An ERROR result that still carries a structured swarm XML result
        // parses (per TS `applyResult`): the swarm is NOT marked failed — the
        // per-member statuses in the XML carry the failure, not the swarm.
        h.handle_event(
            Event::ToolResult {
                agent_id: None,
                tool_call_id: "swarm".to_owned(),
                output: json!(
                    "<agent_swarm_result><subagent index=\"1\" outcome=\"completed\">done</subagent><subagent index=\"2\" outcome=\"failed\">boom</subagent></agent_swarm_result>"
                ),
                is_error: true,
                synthetic: false,
            },
            1300,
        );
        let progress = h.subagent.swarm_progress("swarm").unwrap();
        assert!(!progress.is_tool_call_active());
        assert!(!progress.failed);
    }

    #[test]
    fn swarm_unparseable_error_result_marks_swarm_failed() {
        let mut h = handler();
        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "swarm".to_owned(),
                name: "AgentSwarm".to_owned(),
                args: crate::controllers::args_json(json!({"description": "batch"})),
                description: Some("batch".to_owned()),
            },
            1000,
        );
        // An error result with no parseable swarm statuses → swarm failed.
        h.handle_event(
            Event::ToolResult {
                agent_id: None,
                tool_call_id: "swarm".to_owned(),
                output: json!("crashed without a structured result"),
                is_error: true,
                synthetic: false,
            },
            1300,
        );
        let progress = h.subagent.swarm_progress("swarm").unwrap();
        assert!(!progress.is_tool_call_active());
        assert!(progress.failed);
    }

    #[test]
    fn child_tool_progress_routes_to_child_card() {
        let mut h = handler();
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "child".to_owned(),
                parent_tool_call_id: "card".to_owned(),
                subagent_name: "coder".to_owned(),
                run_in_background: false,
                swarm_index: None,
                description: None,
            },
            1000,
        );
        // A child-agent tool.progress carries an agent id, so the subagent
        // routing consumes it (TS: appendSubToolLiveOutput to the child card)
        // instead of the main handler treating it as a main-agent output.
        h.handle_event(
            Event::ToolProgress {
                agent_id: Some("child".to_owned()),
                tool_call_id: "sub-tool".to_owned(),
                update: crate::controllers::events::ToolProgressUpdate {
                    kind: crate::controllers::events::ToolProgressKind::Stdout,
                    text: Some("subagent output".to_owned()),
                },
            },
            1100,
        );
        // Consumed by the child routing → never reached the main live-output
        // map.
        assert!(h.state.tool_live_outputs.is_empty());
    }

    #[test]
    fn main_tool_progress_without_agent_id_reaches_main_handler() {
        let mut h = handler();
        // A main-agent tool component exists; a no-agent-id tool.progress is
        // NOT routed to a child card and lands in the main live-output map.
        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                name: "Bash".to_owned(),
                args: Args::new(),
                description: None,
            },
            1000,
        );
        h.handle_event(
            Event::ToolProgress {
                agent_id: None,
                tool_call_id: "t1".to_owned(),
                update: crate::controllers::events::ToolProgressUpdate {
                    kind: crate::controllers::events::ToolProgressKind::Status,
                    text: Some("running".to_owned()),
                },
            },
            1100,
        );
        assert_eq!(h.state.tool_live_outputs.get("t1").unwrap().len(), 1);
    }

    #[test]
    fn turn_ended_with_agent_id_completes_btw_panel() {
        let mut h = handler();
        h.btw.open("child", "hello");
        assert!(h.btw.active().unwrap().running);
        // The child's turn.ended carries its agent id, so the subagent
        // routing delivers it to the BTW panel (TS btw-panel routeEvent uses
        // event.agentId), which transitions running → done.
        h.handle_event(
            Event::TurnEnded {
                agent_id: Some("child".to_owned()),
                turn_id: "1".to_owned(),
                reason: TurnEndReason::Completed,
                error: None,
            },
            1000,
        );
        let panel = h.btw.active().unwrap();
        assert!(panel.done);
        assert!(!panel.running);
    }

    #[test]
    fn turn_ended_with_agent_id_fails_btw_panel() {
        let mut h = handler();
        h.btw.open("child", "hello");
        h.handle_event(
            Event::TurnEnded {
                agent_id: Some("child".to_owned()),
                turn_id: "1".to_owned(),
                reason: TurnEndReason::Failed,
                error: Some(crate::controllers::events::ErrorPayload {
                    code: "boom".to_owned(),
                    message: "it broke".to_owned(),
                    details: None,
                }),
            },
            1000,
        );
        let panel = h.btw.active().unwrap();
        assert!(panel.failed_message.is_some());
        assert!(!panel.running);
    }

    #[test]
    fn main_turn_ended_not_routed_to_btw() {
        let mut h = handler();
        h.btw.open("child", "hello");
        // The MAIN agent's turn end (agent id "main") is not a child event:
        // routing must NOT deliver it to the BTW panel (it falls through to
        // the main turn handler).
        h.handle_event(
            Event::TurnEnded {
                agent_id: Some(MAIN_AGENT_ID.to_owned()),
                turn_id: "1".to_owned(),
                reason: TurnEndReason::Completed,
                error: None,
            },
            1000,
        );
        let panel = h.btw.active().unwrap();
        assert!(!panel.done);
        assert!(panel.running);
    }

    #[test]
    fn swarm_cancelled_when_turn_cancelled() {
        let mut h = handler();
        h.handle_event(
            Event::SubagentSpawned {
                subagent_id: "s1".to_owned(),
                parent_tool_call_id: "swarm".to_owned(),
                subagent_name: "coder".to_owned(),
                run_in_background: false,
                swarm_index: None,
                description: None,
            },
            1000,
        );
        h.handle_event(
            Event::ToolCallStarted {
                agent_id: None,
                tool_call_id: "swarm".to_owned(),
                name: "AgentSwarm".to_owned(),
                args: Args::new(),
                description: None,
            },
            1100,
        );
        h.handle_event(
            Event::TurnEnded {
                agent_id: None,
                turn_id: "1".to_owned(),
                reason: TurnEndReason::Cancelled,
                error: None,
            },
            1200,
        );
        let progress = h.subagent.swarm_progress("swarm").unwrap();
        assert!(progress.active_cancelled);
        assert!(progress.is_tool_call_active());
    }

    #[test]
    fn is_user_cancelled_subagent_error_matches() {
        assert!(is_user_cancelled_subagent_error("Aborted by the user"));
        assert!(is_user_cancelled_subagent_error(
            "The user manually interrupted this subagent batch."
        ));
        assert!(!is_user_cancelled_subagent_error("crashed"));
    }
}
