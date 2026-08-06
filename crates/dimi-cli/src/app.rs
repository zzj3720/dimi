//! `DimiApp` — the dimi-cli coordinator (port of the minimal `DimiTUI`
//! surface from `apps/dimi/src/tui/dimi-tui.ts`).
//!
//! Owns the three shared components the TUI mounts (transcript / footer /
//! editor), the session state machine, the echo backend stub, and the event
//! loop (raw terminal + Kitty negotiation + input routing + resize). Engine
//! wiring is a later slice: the data plane is the wire.jsonl cold rebuild and
//! interactive input is echoed back as status rows, so the full UI can be
//! exercised headless via [`DimiApp::render_lines`].

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use dimi_engine::engine::Engine;
use dimi_engine::events::EngineEvent;
use dimi_engine::llm::{LlmClient, OpenAiCompatibleClient};
use dimi_engine::permission::{PermissionMode, PolicyConfig};
use dimi_engine::tool::{BashTool, ToolRegistry};
use dimi_engine::types::{
    EngineTool, EngineTurnInput, LlmMessage, LlmToolCall, LlmToolCallFunction, ProviderConfig,
};
use dimi_tui::chrome::{QueuePaneOptions, WelcomeState};
use dimi_tui::commands::{
    DispatchAction, SlashCommand, builtin_slash_commands, dispatch_input,
    find_builtin_slash_command,
};
use dimi_tui::component::{Component, SharedComponent};
use dimi_tui::components::messages::tool_renderers::ToolResultData;
use dimi_tui::controllers::btw::BtwPanelController;
use dimi_tui::controllers::events::{ErrorPayload, Event, TurnEndReason};
use dimi_tui::controllers::{BackgroundTaskInfo, BackgroundTaskKind, BackgroundTaskStatus};
use dimi_tui::custom_editor::{CustomEditor, CustomEditorCallbacks, InputMode};
use dimi_tui::dialogs::session_picker::{
    SessionPickerAction, SessionPickerComponent, SessionPickerOptions, SessionRow, SessionScope,
};
use dimi_tui::editor::EditorOptions;
use dimi_tui::footer::{FooterComponent, FooterState};
use dimi_tui::keys::matches_key;
use dimi_tui::process_terminal::ProcessTerminal;
use dimi_tui::session::{
    BusyInputIntent, BusyInputMode, SessionBackend, SessionError, SessionManager,
    resolve_busy_input_intent,
};
use dimi_tui::terminal::Terminal;
use dimi_tui::theme::{ColorToken, DARK_COLORS, LIGHT_COLORS, invalidate_components, set_palette};
use dimi_tui::tui::Tui;
use dimi_tui::wire_transcript::{TranscriptEntry, TranscriptEntryKind};

use crate::activity::{ActivityComponent, ActivityMode};
use crate::btw_panel::BtwPanelComponent;
use crate::config::Config;
use crate::panels::{EditorSlotComponent, QueuePanelHost};
use crate::tasks_panel::{TaskInfo, TaskListComponent};
use crate::transcript::{
    TranscriptContainer, assistant_entry, status_entry, tool_call_entry, user_entry,
};

/// Child indices of the mounted TUI tree (transcript=0, activity=1, btw=2,
/// queue=3, tasks=4, footer=5, editor-slot=6).
const EDITOR_CHILD_INDEX: usize = 6;

/// The agent id `/btw` panels attach to (a dimi-cli-local subagent handle).
/// Engine events carrying this attribution are routed to the BTW panel.
pub const BTW_AGENT_ID: &str = "btw";

/// Status shown whenever a normal prompt would reach the engine but the
/// provider is not configured — a clear nudge to fill in `config.toml`.
pub const ENGINE_NOT_CONFIGURED_MSG: &str =
    "未配置 provider：请在 config.toml 设置 model / base_url / api_key 后重试";

/// The Bash tool definition advertised to the LLM (matches `BashTool`'s
/// schema: `command` required, `cwd`/`timeout`/`description` optional).
fn bash_tool_def() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": "Bash",
            "description": "Run a shell command in the working directory and return its output. Use for file system, git, build and any terminal work.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to run" },
                    "cwd": { "type": "string", "description": "Working directory (optional, defaults to the session cwd)" },
                    "timeout": { "type": "integer", "description": "Timeout in seconds (default 60, max 300)" },
                    "description": { "type": "string", "description": "A short note on what the command does (optional)" }
                },
                "required": ["command"],
                "additionalProperties": false
            }
        }
    })
}

/// The engine tool list handed to the LLM each turn (slice 6a: Bash only).
fn engine_tools() -> Vec<EngineTool> {
    vec![EngineTool {
        name: "Bash".to_string(),
        description: "Run a shell command in the working directory and return its output."
            .to_string(),
        args_schema: bash_tool_def()["function"]["parameters"].clone(),
    }]
}

/// A plain user LLM message (used to merge a prompt that is not already in the
/// transcript — the `/btw` side prompt — and for steer injection).
fn user_llm_message(content: &str) -> LlmMessage {
    LlmMessage {
        role: "user".to_string(),
        content: serde_json::Value::String(content.to_owned()),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        tools: None,
        reasoning: None,
        origin: None,
    }
}

/// Assemble the OpenAI-shaped conversation from the transcript entries.
/// User/assistant text maps to `user`/`assistant` messages; tool exchanges
/// map back to `assistant(tool_calls)` + `tool(result)` messages so a later
/// turn's context stays well-formed (consecutive user messages would 400).
/// Thinking/status entries are display-only and skipped.
fn transcript_messages(entries: &[TranscriptEntry]) -> Vec<LlmMessage> {
    let mut out = Vec::new();
    for entry in entries {
        match entry.kind {
            TranscriptEntryKind::User => out.push(LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(entry.content.clone()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            }),
            TranscriptEntryKind::Assistant => out.push(LlmMessage {
                role: "assistant".to_string(),
                content: serde_json::Value::String(entry.content.clone()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            }),
            TranscriptEntryKind::ToolCall => {
                // The tool-call card maps back to an assistant message with
                // `tool_calls` (OpenAI wire shape: args serialized to JSON).
                if let Some(call) = &entry.tool_call {
                    let args_json = serde_json::Value::Object(call.args.clone());
                    out.push(LlmMessage {
                        role: "assistant".to_string(),
                        content: serde_json::Value::String(String::new()),
                        name: None,
                        tool_call_id: None,
                        tool_calls: Some(vec![LlmToolCall {
                            id: call.id.clone(),
                            call_type: Some("function".to_string()),
                            function: LlmToolCallFunction {
                                name: call.name.clone(),
                                arguments: serde_json::to_string(&args_json)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            },
                        }]),
                        tools: None,
                        reasoning: None,
                        origin: None,
                    });
                }
                // The attached result (when it landed) maps to a `tool`
                // message completing the exchange.
                if let Some(result) = &entry.tool_result {
                    out.push(LlmMessage {
                        role: "tool".to_string(),
                        content: serde_json::Value::String(result.output.clone()),
                        name: None,
                        tool_call_id: Some(result.tool_call_id.clone()),
                        tool_calls: None,
                        tools: None,
                        reasoning: None,
                        origin: None,
                    });
                }
            }
            TranscriptEntryKind::Thinking | TranscriptEntryKind::Status => {}
        }
    }
    out
}

/// The engine backend behind [`SessionManager`] (slice 6a: real engine
/// dialogue). Each dispatched prompt spawns a real `Engine::run_turn` on the
/// app's tokio runtime; the turn's [`EngineEvent`]s stream back through an
/// mpsc channel that [`DimiApp::pump_engine_turns`] drains into the
/// transcript, so the synchronous event loop never blocks on the engine.
pub struct EngineBackend {
    handle: tokio::runtime::Handle,
    llm: std::sync::Arc<dyn LlmClient>,
    tools: std::sync::Arc<ToolRegistry>,
    transcript: Rc<RefCell<TranscriptContainer>>,
    /// Engine events from in-flight turns, drained by the app (non-blocking).
    rx: tokio::sync::mpsc::UnboundedReceiver<EngineEvent>,
    /// Sender cloned into each spawned turn task.
    tx: tokio::sync::mpsc::UnboundedSender<EngineEvent>,
    /// The current turn's task handle (used by `wait_for_turn` for
    /// deterministic tests / replay).
    current_turn: Option<tokio::task::JoinHandle<()>>,
    /// Set when a drained batch contained `turn.ended`: the app resets the
    /// session's streaming flag so the next idle flush can dispatch.
    turn_idle: bool,
    /// Persistent `tool_call_id → (turn_id, transcript entry index)` map: the
    /// P1 cross-batch fix — a `tool.result` arriving in a later pump batch is
    /// attached to the tool-call card it was created in, instead of being
    /// dropped for living outside the current batch. Entries are removed when
    /// their turn ends.
    pending_tool_cards: HashMap<String, (i64, usize)>,
    /// Turn ids whose events belong to the BTW side panel (P3/P7): they are
    /// skipped by the main-transcript projection and never set `turn_idle`
    /// (a btw turn finishing must not clear a concurrent user turn's
    /// streaming). Registered by [`EngineBackend::start_btw_turn`], removed
    /// when the turn ends.
    btw_turn_ids: HashSet<i64>,
    /// The current main (non-btw) turn's steering channel (P6): while a turn
    /// is streaming, `handle_submit` in steer mode pushes user messages here
    /// and the engine drains them into the next step's request. `None` while
    /// idle (or only a btw turn is running) — steering then falls back to
    /// queueing.
    steer: Option<std::sync::Arc<std::sync::Mutex<Vec<LlmMessage>>>>,
    /// Provider config (`None` = not configured → clear error status).
    provider: Option<ProviderConfig>,
    policy: PolicyConfig,
    /// Working directory handed to tool execution.
    work_dir: Option<String>,
    /// Background tasks tracked from `task.started` / `task.settled`
    /// (backgrounded bash / subagents — populated when the engine emits them).
    tasks: Vec<BackgroundTaskInfo>,
    next_turn_id: i64,
    next_session_id: i64,
}

impl EngineBackend {
    pub fn new(
        handle: tokio::runtime::Handle,
        llm: std::sync::Arc<dyn LlmClient>,
        tools: std::sync::Arc<ToolRegistry>,
        transcript: Rc<RefCell<TranscriptContainer>>,
        provider: Option<ProviderConfig>,
        work_dir: Option<String>,
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        EngineBackend {
            handle,
            llm,
            tools,
            transcript,
            rx,
            tx,
            current_turn: None,
            turn_idle: false,
            pending_tool_cards: HashMap::new(),
            btw_turn_ids: HashSet::new(),
            steer: None,
            provider,
            policy: PolicyConfig {
                mode: PermissionMode::Auto,
                rules: vec![],
                session_approved_patterns: vec![],
            },
            work_dir,
            tasks: Vec::new(),
            next_turn_id: 1,
            next_session_id: 1,
        }
    }

    /// Spawn one main engine turn for `text` (used by both prompt and `!`
    /// bash lines — slice 6a routes bash through the model). The text is
    /// expected to already be the trailing user entry (the app pushes it
    /// before flushing); a steering channel is created so a busy Enter can
    /// inject mid-turn (P6). Returns the spawned turn's id, or `None` when
    /// the provider is not configured (a clear status was already pushed to
    /// the transcript).
    pub fn start_turn(&mut self, text: &str) -> Option<i64> {
        let transcript = self.transcript.borrow();
        let mut messages = transcript_messages(transcript.entries());
        // Defensive: if the caller did not push `text` into the transcript
        // (all current callers do), merge it so the prompt always reaches the
        // LLM rather than silently dropping (P2 sibling of the `/btw` fix).
        let already_trailing = matches!(
            messages.last(),
            Some(LlmMessage {
                role,
                content: serde_json::Value::String(s),
                ..
            }) if role == "user" && s == text
        );
        if !already_trailing {
            messages.push(user_llm_message(text));
        }
        drop(transcript);
        self.spawn_turn(messages, false)
    }

    /// Spawn a `/btw` side-agent turn. The prompt is merged into the request
    /// as a user message — it is owned by the BTW panel, not the main
    /// transcript, so it cannot be assembled from the transcript entries
    /// (P2). The turn is registered as a btw turn: its events update the
    /// panel only and its end never clears a concurrent main turn's streaming
    /// (P3/P7).
    pub fn start_btw_turn(&mut self, prompt: &str) -> Option<i64> {
        let transcript = self.transcript.borrow();
        let mut messages = transcript_messages(transcript.entries());
        messages.push(user_llm_message(prompt));
        drop(transcript);
        self.spawn_turn(messages, true)
    }

    /// Inject a steering message into the running main turn (P6). Returns
    /// `false` when there is no live turn to steer into (idle, or only a btw
    /// turn is running) — the caller then falls back to queueing.
    pub fn inject_steer(&mut self, text: &str) -> bool {
        let Some(steer) = &self.steer else {
            return false;
        };
        steer.lock().unwrap().push(user_llm_message(text));
        true
    }

    /// Build the turn input and spawn it on the app runtime. `is_btw`
    /// registers the turn as panel-owned (skipped by the main transcript
    /// projection, its end ignored for streaming) and does not claim the
    /// steering channel.
    fn spawn_turn(&mut self, messages: Vec<LlmMessage>, is_btw: bool) -> Option<i64> {
        let Some(provider) = self.provider.clone() else {
            self.transcript.borrow_mut().push(status_entry(
                ENGINE_NOT_CONFIGURED_MSG,
                Some(ColorToken::Error),
            ));
            return None;
        };

        let turn_id = self.next_turn_id;
        self.next_turn_id += 1;
        let input = EngineTurnInput {
            turn_id,
            messages,
            tools: engine_tools(),
            active_tools: None,
            provider,
            max_steps_per_turn: None,
            max_retries_per_step: None,
            cwd: self.work_dir.clone(),
            shell: None,
            context_window: None,
            max_context_tokens: None,
            reserved_context_size: None,
            compaction_trigger_ratio: None,
            next_agent_id: None,
            kill_grace_ms: None,
            subagent_model: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
            completion_review: None,
            origin: dimi_wire::model::TurnOrigin::User { payload: None },
            uses_worker_rejection_guidance: false,
        };

        let steer = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        if is_btw {
            self.btw_turn_ids.insert(turn_id);
        } else {
            self.steer = Some(steer.clone());
        }

        let engine = Engine::default();
        let llm = self.llm.clone();
        let tools = self.tools.clone();
        let policy = self.policy.clone();
        let tx = self.tx.clone();
        let handle = self.handle.spawn(async move {
            let mut on_event = move |event: EngineEvent| {
                let _ = tx.send(event);
            };
            let _ = engine
                .run_turn_with_steer(
                    &input,
                    steer,
                    llm.as_ref(),
                    tools.as_ref(),
                    &policy,
                    &mut on_event,
                )
                .await;
        });
        self.current_turn = Some(handle);
        Some(turn_id)
    }

    /// The background tasks tracked so far (from `task.started` /
    /// `task.settled`).
    pub fn tasks(&self) -> &[BackgroundTaskInfo] {
        &self.tasks
    }

    /// Abort the in-flight turn (if any) — used when closing the BTW panel
    /// (Esc) so the side-agent's output stops streaming instead of leaking
    /// into the transcript. No-op when idle.
    pub fn cancel_current_turn(&mut self) {
        if let Some(handle) = self.current_turn.take() {
            handle.abort();
        }
        self.turn_idle = false;
    }

    /// Point the next turn at a different model (`/model <name>`). The engine
    /// snapshots the provider per turn, so the change takes effect from the
    /// next prompt without touching the in-flight one. No-op while the
    /// provider is not configured (the unconfigured status path stays).
    pub fn set_model(&mut self, model: &str) {
        if let Some(provider) = &mut self.provider {
            provider.model = model.to_string();
        }
    }

    /// Point the next turn at a different thinking effort (`/effort`). The
    /// engine snapshots the provider per turn, so the change takes effect
    /// from the next prompt. No-op while the provider is not configured.
    pub fn set_thinking_effort(&mut self, effort: &str) {
        if let Some(provider) = &mut self.provider {
            provider.thinking_effort = Some(effort.to_string());
        }
    }

    /// Change the permission mode used for the next turn's tool-call policy
    /// (`/permission` / `/yolo` / `/auto`).
    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.policy.mode = mode;
    }

    /// Drain every buffered engine event (non-blocking) into transcript
    /// entries, and return the drained events so the app can route them to
    /// panel controllers (e.g. the BTW panel). Empty when nothing changed.
    fn drain_events(&mut self) -> Vec<EngineEvent> {
        let mut batch: Vec<EngineEvent> = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            batch.push(event);
        }
        if batch.is_empty() {
            return batch;
        }
        self.apply_engine_events(&batch);
        batch
    }

    /// Map a batch of engine events to transcript entries (assistant text +
    /// tool calls/results + turn-end status). Entries are pushed to the
    /// transcript immediately so tool-call cards get a stable index the
    /// persistent `pending_tool_cards` map can refer back to across batches
    /// (P1). BTW-turn events are skipped here — they belong to the panel
    /// (P3), and their turn end never sets `turn_idle` (P7).
    fn apply_engine_events(&mut self, events: &[EngineEvent]) {
        let mut pending_assistant = String::new();
        let mut main_turn_ended = false;
        for event in events {
            let btw = event_turn_id(event).is_some_and(|id| self.btw_turn_ids.contains(&id));
            if btw {
                // P3/P7: btw-turn events update the panel only. Still clean up
                // its pending tool cards when the turn ends.
                if let EngineEvent::TurnEnded { turn_id, .. } = event {
                    self.pending_tool_cards.retain(|_, (t, _)| *t != *turn_id);
                }
                continue;
            }
            match event {
                EngineEvent::AssistantDelta { delta, .. } => pending_assistant.push_str(delta),
                EngineEvent::ToolCallStarted {
                    turn_id,
                    tool_call_id,
                    name,
                    args,
                    ..
                } => {
                    self.flush_assistant_to_transcript(&mut pending_assistant);
                    let idx = {
                        let mut transcript = self.transcript.borrow_mut();
                        let idx = transcript.entries().len();
                        transcript.push(tool_call_entry(tool_call_id, name, args.as_ref()));
                        idx
                    };
                    // Record the card's location so a result arriving in a
                    // later batch can still attach (P1).
                    self.pending_tool_cards
                        .insert(tool_call_id.clone(), (*turn_id, idx));
                }
                EngineEvent::ToolResult {
                    tool_call_id,
                    output,
                    is_error,
                    ..
                } => {
                    self.attach_tool_result(
                        tool_call_id,
                        ToolResultData {
                            tool_call_id: tool_call_id.clone(),
                            output: output.clone(),
                            is_error: is_error.unwrap_or(false),
                        },
                    );
                }
                EngineEvent::TurnEnded {
                    turn_id,
                    reason,
                    error,
                    ..
                } => {
                    self.flush_assistant_to_transcript(&mut pending_assistant);
                    if reason == "completed" {
                        self.transcript
                            .borrow_mut()
                            .push(status_entry("✓ turn 完成", None));
                    } else {
                        let message = error
                            .as_ref()
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("engine turn failed");
                        self.transcript.borrow_mut().push(status_entry(
                            &format!("✗ turn {reason}: {message}"),
                            Some(ColorToken::Error),
                        ));
                    }
                    // This turn's tool exchanges are fully resolved — forget
                    // their card indices (the map is per-turn, not per-batch).
                    self.pending_tool_cards.retain(|_, (t, _)| *t != *turn_id);
                    // The session's busy flag must drop so the next idle flush
                    // can dispatch new input (TS `streamingPhase` idle parity).
                    main_turn_ended = true;
                }
                EngineEvent::TaskStarted {
                    task_id,
                    kind,
                    description,
                    ..
                } => {
                    let task_kind = if kind == "agent" {
                        BackgroundTaskKind::Agent
                    } else {
                        BackgroundTaskKind::Process
                    };
                    self.tasks.retain(|t| t.task_id != *task_id);
                    self.tasks.push(BackgroundTaskInfo {
                        task_id: task_id.clone(),
                        kind: task_kind,
                        status: BackgroundTaskStatus::Running,
                        agent_id: None,
                        description: Some(description.clone()),
                        exit_code: None,
                        stop_reason: None,
                        detached: None,
                    });
                }
                EngineEvent::TaskSettled {
                    task_id,
                    status,
                    error,
                    exit_code,
                    ..
                } => {
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.task_id == *task_id) {
                        task.status = match status.as_str() {
                            "completed" => BackgroundTaskStatus::Completed,
                            "timed_out" => BackgroundTaskStatus::TimedOut,
                            _ => BackgroundTaskStatus::Failed,
                        };
                        task.exit_code = exit_code.map(|c| c as i32);
                        task.stop_reason = error.clone();
                    }
                }
                _ => {}
            }
        }
        self.flush_assistant_to_transcript(&mut pending_assistant);
        if main_turn_ended {
            self.turn_idle = true;
            // The main turn is done → its steering channel is dead; a later
            // busy submit must not inject into it.
            self.steer = None;
        }
    }

    /// Attach a tool result to its tool-call card, resolving the card across
    /// the whole transcript — the persistent `pending_tool_cards` map first
    /// (works when the card lives in an earlier batch, P1), falling back to a
    /// scan when the recorded index is stale (undo/clear truncated the
    /// transcript).
    fn attach_tool_result(&mut self, tool_call_id: &str, result: ToolResultData) {
        if let Some((_, idx)) = self.pending_tool_cards.get(tool_call_id).copied() {
            let mut transcript = self.transcript.borrow_mut();
            if let Some(entry) = transcript.entries_mut().get_mut(idx)
                && entry.kind == TranscriptEntryKind::ToolCall
                && entry
                    .tool_call
                    .as_ref()
                    .is_some_and(|c| c.id == tool_call_id)
            {
                entry.tool_result = Some(result);
                return;
            }
        }
        // Index stale (transcript truncated/cleared) → scan for the card.
        let mut transcript = self.transcript.borrow_mut();
        if let Some(entry) = transcript
            .entries_mut()
            .iter_mut()
            .rev()
            .find(|e| e.tool_call.as_ref().is_some_and(|c| c.id == tool_call_id))
        {
            entry.tool_result = Some(result);
        }
    }

    /// Flush the accumulated assistant text into the transcript as one entry
    /// (empty → no-op).
    fn flush_assistant_to_transcript(&self, pending: &mut String) {
        if pending.is_empty() {
            return;
        }
        self.transcript.borrow_mut().push(assistant_entry(pending));
        pending.clear();
    }
}

/// The `turn_id` carried by an engine event, if any (task/agent events have
/// none).
fn event_turn_id(event: &EngineEvent) -> Option<i64> {
    match event {
        EngineEvent::TurnStarted { turn_id, .. }
        | EngineEvent::TurnEnded { turn_id, .. }
        | EngineEvent::TurnStepStarted { turn_id, .. }
        | EngineEvent::TurnStepCompleted { turn_id, .. }
        | EngineEvent::TurnStepInterrupted { turn_id, .. }
        | EngineEvent::TurnStepRetrying { turn_id, .. }
        | EngineEvent::AssistantDelta { turn_id, .. }
        | EngineEvent::ThinkingDelta { turn_id, .. }
        | EngineEvent::ToolCallDelta { turn_id, .. }
        | EngineEvent::ToolCallStarted { turn_id, .. }
        | EngineEvent::ToolProgress { turn_id, .. }
        | EngineEvent::ToolResult { turn_id, .. }
        | EngineEvent::ContextCompacted { turn_id, .. }
        | EngineEvent::CompletionReviewInjected { turn_id, .. } => Some(*turn_id),
        EngineEvent::TaskStarted { .. }
        | EngineEvent::TaskSettled { .. }
        | EngineEvent::TaskOutput { .. } => None,
    }
}

fn task_status_label(status: BackgroundTaskStatus) -> String {
    match status {
        BackgroundTaskStatus::Running => "running",
        BackgroundTaskStatus::Completed => "completed",
        BackgroundTaskStatus::Failed => "failed",
        BackgroundTaskStatus::TimedOut => "timed out",
        BackgroundTaskStatus::Killed => "killed",
        BackgroundTaskStatus::Lost => "lost",
    }
    .to_string()
}

/// Wall-clock now in milliseconds (the same unit as `Date.now()`), used for
/// the `/sessions` picker's relative timestamps.
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Clipboard copy strategy for `/copy` — injectable so tests never touch a
/// real clipboard tool.
type Clipboard = Box<dyn Fn(&str) -> Result<(), String>>;

/// The production clipboard: `pbcopy` on macOS, `xclip -selection clipboard`
/// on Linux, an error elsewhere (the TS clipboard helper's native path).
fn default_clipboard() -> Clipboard {
    Box::new(copy_via_clipboard_tool)
}

/// Spawn the platform clipboard tool and pipe `text` into its stdin.
fn copy_via_clipboard_tool(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let (cmd, args): (&str, &[&str]) = ("pbcopy", &[]);
    #[cfg(target_os = "linux")]
    let (cmd, args): (&str, &[&str]) = ("xclip", &["-selection", "clipboard"]);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err("剪贴板工具不可用（需要 pbcopy 或 xclip）".to_string());

    use std::io::Write;
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 {cmd} 失败：{e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "无法写入剪贴板子进程".to_string())?
        .write_all(text.as_bytes())
        .map_err(|e| format!("写入剪贴板失败：{e}"))?;
    let status = child.wait().map_err(|e| format!("等待 {cmd} 失败：{e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} 退出码：{status}"))
    }
}

/// Render the transcript as a Markdown document (port of the TS
/// `buildExportMarkdown` as a flat, human-readable export — the TS version
/// carries structured history from the session context).
fn transcript_to_markdown(
    entries: &[TranscriptEntry],
    session_id: Option<&str>,
    work_dir: Option<&str>,
) -> String {
    let mut md = String::from("# Dimi 会话导出\n\n");
    if let Some(id) = session_id {
        md.push_str(&format!("- Session: `{id}`\n"));
    }
    if let Some(dir) = work_dir {
        md.push_str(&format!("- Workdir: `{dir}`\n"));
    }
    md.push('\n');
    for entry in entries {
        match entry.kind {
            TranscriptEntryKind::User => {
                md.push_str("## User\n\n");
                md.push_str(entry.content.trim());
                md.push_str("\n\n");
            }
            TranscriptEntryKind::Assistant => {
                md.push_str("## Assistant\n\n");
                md.push_str(entry.content.trim());
                md.push_str("\n\n");
            }
            TranscriptEntryKind::Thinking => {
                md.push_str("## Thinking\n\n");
                md.push_str(entry.content.trim());
                md.push_str("\n\n");
            }
            TranscriptEntryKind::Status => {
                md.push_str(&format!("> {}\n\n", entry.content.trim()));
            }
            TranscriptEntryKind::ToolCall => {
                let name = entry
                    .tool_call
                    .as_ref()
                    .map(|c| c.name.as_str())
                    .unwrap_or("tool");
                let args = entry
                    .tool_call
                    .as_ref()
                    .and_then(|c| serde_json::to_string(&c.args).ok())
                    .unwrap_or_default();
                md.push_str(&format!("### Tool: {name}\n\n```json\n{args}\n```\n\n"));
            }
        }
    }
    md
}

/// Write the exported Markdown to `path`, creating the parent directory.
fn write_markdown_export(path: &Path, md: &str) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建目录 {}：{e}", parent.display()))?;
    }
    std::fs::write(path, md).map_err(|e| format!("写入 {} 失败：{e}", path.display()))
}

/// Map a permission-mode string to the engine's [`PermissionMode`].
fn parse_permission_mode(mode: &str) -> PermissionMode {
    match mode {
        "yolo" => PermissionMode::Yolo,
        "auto" => PermissionMode::Auto,
        _ => PermissionMode::Manual,
    }
}

/// Parse `/undo`'s optional count (`[1-9][0-9]*`), defaulting to 1 (port of
/// `parseUndoCount`).
fn parse_undo_count(args: &str) -> Option<usize> {
    let value = args.trim();
    if value.is_empty() {
        return Some(1);
    }
    if value.chars().all(|c| c.is_ascii_digit()) {
        let n: usize = value.parse().ok()?;
        if n > 0 { Some(n) } else { None }
    } else {
        None
    }
}

impl SessionBackend for EngineBackend {
    fn create_session(&mut self) -> Result<String, SessionError> {
        let id = format!("session-{}", self.next_session_id);
        self.next_session_id += 1;
        Ok(id)
    }
    fn resume_session(&mut self, session_id: &str) -> Result<String, SessionError> {
        Ok(session_id.to_owned())
    }
    fn switch_session(&mut self, session_id: &str) -> Result<String, SessionError> {
        Ok(session_id.to_owned())
    }
    fn send_bash_line(&mut self, text: &str) -> bool {
        // Slice 6a simplification: `!` shell lines also run through the
        // engine turn (real dimi-exec dispatch is a later slice).
        self.start_turn(text).is_some()
    }
    fn send_prompt(&mut self, text: &str) -> bool {
        self.start_turn(text).is_some()
    }
}

/// Shares the [`ProcessTerminal`] between the `Tui` (as its `Terminal`) and
/// the event loop (which drives stdin through the buffer).
struct SharedTerminal {
    inner: Rc<RefCell<ProcessTerminal>>,
}

impl Terminal for SharedTerminal {
    fn start(&mut self, on_input: &mut dyn FnMut(&str), on_resize: &mut dyn FnMut()) {
        self.inner.borrow_mut().start(on_input, on_resize);
    }
    fn stop(&mut self) {
        self.inner.borrow_mut().stop();
    }
    fn write(&mut self, data: &str) {
        self.inner.borrow_mut().write(data);
    }
    fn columns(&self) -> usize {
        self.inner.borrow().columns()
    }
    fn rows(&self) -> usize {
        self.inner.borrow().rows()
    }
    fn hide_cursor(&mut self) {
        self.inner.borrow_mut().hide_cursor();
    }
    fn show_cursor(&mut self) {
        self.inner.borrow_mut().show_cursor();
    }
    fn kitty_protocol_active(&self) -> bool {
        self.inner.borrow().kitty_protocol_active()
    }
    fn move_by(&mut self, lines: isize) {
        self.inner.borrow_mut().move_by(lines);
    }
    fn clear_line(&mut self) {
        self.inner.borrow_mut().clear_line();
    }
    fn clear_from_cursor(&mut self) {
        self.inner.borrow_mut().clear_from_cursor();
    }
    fn clear_screen(&mut self) {
        self.inner.borrow_mut().clear_screen();
    }
    fn set_title(&mut self, title: &str) {
        self.inner.borrow_mut().set_title(title);
    }
    fn set_progress(&mut self, active: bool) {
        self.inner.borrow_mut().set_progress(active);
    }
    fn drain_input(&mut self, max_ms: u64, idle_ms: u64) {
        self.inner.borrow_mut().drain_input(max_ms, idle_ms);
    }
}

/// The app's slash-command list: the full builtin registry plus the slice-6
/// `/wire` replay command (a dev affordance, not part of the TS registry).
fn app_slash_commands() -> &'static [SlashCommand] {
    static APP_SLASH_COMMANDS: std::sync::OnceLock<Vec<SlashCommand>> = std::sync::OnceLock::new();
    APP_SLASH_COMMANDS.get_or_init(|| {
        let mut commands = builtin_slash_commands().to_vec();
        commands.push(SlashCommand::new(
            "wire",
            "Load a wire.jsonl transcript (replay)",
        ));
        commands
    })
}

/// The app coordinator.
pub struct DimiApp {
    pub transcript: Rc<RefCell<TranscriptContainer>>,
    /// The streaming activity pane (moon spinner + working tip) below the
    /// transcript.
    pub activity: Rc<RefCell<ActivityComponent>>,
    /// The BTW panel rendering component (0 lines while closed).
    pub btw_panel: Rc<RefCell<BtwPanelComponent>>,
    /// The BTW panel state machine (`route_event` feeds it; the rendering
    /// component mirrors `active()`).
    pub btw_controller: Rc<RefCell<BtwPanelController>>,
    /// The queue pane host (renders only while the send queue has messages).
    pub queue_pane: Rc<RefCell<QueuePanelHost>>,
    /// The tasks panel (renders only while `/tasks` is open).
    pub tasks_panel: Rc<RefCell<TaskListComponent>>,
    /// The session picker, when mounted (replaces the editor while open).
    pub session_picker: Rc<RefCell<Option<SessionPickerComponent>>>,
    /// Turn id of the current `/btw` turn: engine events carrying this id are
    /// routed to the BTW panel.
    btw_turn_id: Option<i64>,
    /// Sessions the app has created/resumed, for the `/sessions` picker.
    known_sessions: Vec<SessionRow>,
    pub footer: Rc<RefCell<FooterComponent>>,
    pub editor: Rc<RefCell<CustomEditor>>,
    pub session: SessionManager,
    backend: EngineBackend,
    /// The tokio runtime that runs engine turns in the background (the
    /// synchronous event loop never blocks on it — results arrive via the
    /// backend's mpsc channel). Read by [`DimiApp::wait_for_turn`] (the
    /// deterministic test/replay path).
    #[allow(dead_code)]
    runtime: tokio::runtime::Runtime,
    pub config: Config,
    /// Enter-submit texts produced by the editor's `on_submit`, drained by
    /// the event loop after each stdin chunk (decouples the callback from the
    /// borrow of the editor/transcript).
    submit_inbox: Rc<RefCell<Vec<String>>>,
    /// Set by Esc / Ctrl-C / Ctrl-D to break the event loop cleanly.
    exit_flag: Rc<Cell<bool>>,
    /// Clipboard copy strategy for `/copy` (injectable in tests).
    clipboard: Clipboard,
}

impl DimiApp {
    pub fn new(config: Config) -> Self {
        // The production client is the OpenAI-compatible HTTP transport; when
        // the provider is not configured the backend surfaces a clear status
        // before ever calling it (the client is inert in that case).
        let llm: std::sync::Arc<dyn LlmClient> = match config.provider_config() {
            Some(p) => std::sync::Arc::new(OpenAiCompatibleClient {
                base_url: p.base_url.clone(),
                api_key: p.api_key.clone(),
                model: p.model.clone(),
            }),
            None => std::sync::Arc::new(OpenAiCompatibleClient {
                base_url: String::new(),
                api_key: String::new(),
                model: config.model.clone().unwrap_or_default(),
            }),
        };
        Self::with_llm(config, llm)
    }

    /// Build the app with a caller-supplied LLM client — tests inject a
    /// `ScriptedLlmClient` for deterministic, network-free turns.
    pub fn with_llm(config: Config, llm: std::sync::Arc<dyn LlmClient>) -> Self {
        let transcript = Rc::new(RefCell::new(TranscriptContainer::new()));
        let footer = Rc::new(RefCell::new(FooterComponent::new(FooterState::new())));
        let submit_inbox: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let exit_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));

        let inbox = submit_inbox.clone();
        let flag_escape = exit_flag.clone();
        let flag_ctrl_c = exit_flag.clone();
        let flag_ctrl_d = exit_flag.clone();
        let editor = Rc::new(RefCell::new(CustomEditor::new(
            EditorOptions { padding_x: 4 },
            CustomEditorCallbacks {
                on_escape: Some(Box::new(move || {
                    flag_escape.set(true);
                })),
                on_ctrl_c: Some(Box::new(move || {
                    flag_ctrl_c.set(true);
                })),
                on_ctrl_d: Some(Box::new(move || {
                    flag_ctrl_d.set(true);
                })),
                ..Default::default()
            },
        )));
        {
            let mut ed = editor.borrow_mut();
            let inner = ed.inner_mut();
            inner.on_submit = Some(Box::new(move |text: &str| {
                inbox.borrow_mut().push(text.to_owned());
            }));
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build the engine tokio runtime");
        let handle = runtime.handle().clone();

        let mut tools = ToolRegistry::new();
        tools.register_with_def("Bash", Box::new(BashTool::default()), Some(bash_tool_def()));

        let provider = config.provider_config();
        let mut backend = EngineBackend::new(
            handle,
            llm,
            std::sync::Arc::new(tools),
            transcript.clone(),
            provider,
            config.work_dir.clone(),
        );
        let mut session = SessionManager::new(BusyInputMode::default());
        // The TS app creates a session during startup; do the same so queued
        // messages flush immediately instead of waiting on a session gate.
        let _ = session.create_session(&mut backend);

        let mut app = DimiApp {
            transcript,
            activity: Rc::new(RefCell::new(ActivityComponent::new())),
            btw_panel: Rc::new(RefCell::new(BtwPanelComponent::new())),
            btw_controller: Rc::new(RefCell::new(BtwPanelController::new())),
            queue_pane: Rc::new(RefCell::new(QueuePanelHost::new())),
            tasks_panel: Rc::new(RefCell::new(TaskListComponent::new())),
            session_picker: Rc::new(RefCell::new(None)),
            btw_turn_id: None,
            known_sessions: Vec::new(),
            footer,
            editor,
            session,
            backend,
            runtime,
            config,
            submit_inbox,
            exit_flag,
            clipboard: default_clipboard(),
        };
        app.apply_config();
        // Record the startup session so `/sessions` has at least one row.
        app.record_current_session();
        app.update_queue_pane();
        app
    }

    /// Apply the loaded config to the chrome (welcome + footer) and cold-build
    /// the transcript from `config.wire` if set.
    fn apply_config(&mut self) {
        // Apply the configured theme at startup so `/theme light` survives a
        // restart (the palette is process-global; invalidate re-colors all
        // mounted chrome on the next render).
        match self.config.theme.as_deref() {
            Some("light") => {
                set_palette(LIGHT_COLORS);
                self.invalidate_chrome();
            }
            Some("dark") | None => {
                set_palette(DARK_COLORS);
                self.invalidate_chrome();
            }
            Some(other) => {
                eprintln!("dimi-cli: unknown theme {other:?} (expected dark|light); using dark");
                set_palette(DARK_COLORS);
                self.invalidate_chrome();
            }
        }

        let mut welcome = WelcomeState::new();
        if let Some(model) = &self.config.model {
            welcome.model = model.clone();
        }
        if let Some(work_dir) = &self.config.work_dir {
            welcome.work_dir = work_dir.clone();
        }
        self.transcript
            .borrow_mut()
            .set_welcome_state(Some(welcome));

        self.refresh_footer();

        if let Some(wire) = &self.config.wire {
            let _ = self.transcript.borrow_mut().load_wire(Path::new(wire));
        }
    }

    /// Render the chrome below the transcript (activity, btw, queue, tasks,
    /// footer, editor-slot) in mount order. Returns the concatenated lines.
    fn render_chrome(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        lines.extend(self.activity.borrow_mut().render(width));
        lines.extend(self.btw_panel.borrow_mut().render(width));
        lines.extend(self.queue_pane.borrow_mut().render(width));
        lines.extend(self.tasks_panel.borrow_mut().render(width));
        lines.extend(self.footer.borrow_mut().render(width));
        if self.session_picker.borrow().is_some() {
            lines.extend(
                self.session_picker
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .render(width),
            );
        } else {
            lines.extend(self.editor.borrow_mut().render(width));
        }
        lines
    }

    /// Compute the transcript's visible-line budget from the current terminal
    /// size: `rows` minus the height of every chrome section below it
    /// (activity / btw / queue / tasks / footer / editor-slot). The chrome is
    /// rendered first (at `width`) to learn its heights; the TUI mounts the
    /// chrome after the transcript, so the container must cap itself to this
    /// budget.
    fn apply_layout(&mut self, width: usize, rows: usize) {
        let chrome_height = self.render_chrome(width).len();
        let max_lines = rows.saturating_sub(chrome_height);
        self.transcript.borrow_mut().set_max_lines(max_lines);
    }

    /// Pure frame render for headless tests: transcript (capped to the rows
    /// left after the chrome below it), then the chrome (activity / btw /
    /// queue / tasks / footer / editor-slot). Returns exactly `rows` lines
    /// when the transcript fills the budget it was given.
    ///
    /// `#[allow(dead_code)]`: this is the headless-test render surface of a
    /// binary crate — the live app renders through `Tui::do_render` instead,
    /// so `cargo build` (without test cfg) would otherwise flag it unused.
    #[allow(dead_code)]
    pub fn render_lines(&mut self, width: usize, rows: usize) -> Vec<String> {
        let chrome = self.render_chrome(width);
        let max_lines = rows.saturating_sub(chrome.len());
        self.transcript.borrow_mut().set_max_lines(max_lines);
        let transcript_lines = self.transcript.borrow_mut().render(width);
        let mut out = transcript_lines;
        out.extend(chrome);
        out
    }

    /// Drain buffered engine events into the transcript (non-blocking; called
    /// every event-loop iteration and after submits). Returns whether anything
    /// changed so the caller can skip redundant re-renders.
    pub fn pump_engine_turns(&mut self) -> bool {
        let mut changed = false;
        let events = self.backend.drain_events();
        if !events.is_empty() {
            changed = true;
            self.route_btw_events(&events);
            self.sync_activity_mode();
        }
        if self.backend.turn_idle {
            self.backend.turn_idle = false;
            self.session.set_streaming(false);
            changed = true;
            self.sync_activity_mode();
            // P4: the turn ended → dispatch anything queued while it streamed
            // (TS drains on turn.ended; the Rust port never did, leaving the
            // queue stranded until the next manual submit).
            self.session.flush_queued_messages(&mut self.backend);
            self.update_queue_pane();
            self.sync_activity_mode();
        }
        changed
    }

    /// Block until the current turn's spawned task completes, then drain its
    /// events into the transcript. Test / replay affordance — the live event
    /// loop uses the non-blocking [`DimiApp::pump_engine_turns`] instead.
    #[allow(dead_code)]
    pub fn wait_for_turn(&mut self) {
        if let Some(mut handle) = self.backend.current_turn.take() {
            let _ = self.runtime.block_on(&mut handle);
            self.pump_engine_turns();
        }
    }

    /// Start the terminal + TUI and run the event loop (see
    /// `crates/dimi-tui/examples/shell.rs` for the wiring this is based on).
    /// Blocks until EOF, Esc, Ctrl-C, or Ctrl-D.
    pub fn run(&mut self) {
        // SIGINT/SIGTERM must restore the terminal before exiting (raw mode
        // would otherwise be left on the shell). Register atomic flags the
        // event loop polls — mirroring the TS signal handling.
        let sigint = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sigterm = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _ = signal_hook::flag::register(libc::SIGINT, sigint.clone());
        let _ = signal_hook::flag::register(libc::SIGTERM, sigterm.clone());

        let term = Rc::new(RefCell::new(ProcessTerminal::new()));
        let mut tui = Tui::new(Box::new(SharedTerminal {
            inner: term.clone(),
        }));
        tui.set_clear_on_shrink(true);
        tui.add_child(Box::new(SharedComponent::new(self.transcript.clone())));
        tui.add_child(Box::new(SharedComponent::new(self.activity.clone())));
        tui.add_child(Box::new(SharedComponent::new(self.btw_panel.clone())));
        tui.add_child(Box::new(SharedComponent::new(self.queue_pane.clone())));
        tui.add_child(Box::new(SharedComponent::new(self.tasks_panel.clone())));
        tui.add_child(Box::new(SharedComponent::new(self.footer.clone())));
        // The editor slot renders the session picker while it is open and the
        // editor otherwise (TS `mountEditorReplacement` / `restoreEditor`).
        tui.add_child(Box::new(EditorSlotComponent::new(
            self.editor.clone(),
            self.session_picker.clone(),
        )));

        // Initial layout so the first frame caps the transcript correctly.
        let width = term.borrow().columns();
        let rows = term.borrow().rows();
        self.apply_layout(width, rows);

        tui.set_focus(Some(EDITOR_CHILD_INDEX));
        // `SharedComponent` does not forward `Focusable`, so drive the
        // editor's focus flag directly — needed for CURSOR_MARKER emission.
        self.editor.borrow_mut().inner_mut().set_focused(true);
        tui.start();

        // A dedicated reader thread feeds raw stdin bytes into a channel; the
        // main loop polls it with a short timeout so it can pump engine turn
        // events and re-render while a turn streams (the TUI loop must never
        // block on stdin while the assistant is typing).
        let (stdin_tx, stdin_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 4096];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => {
                        let _ = stdin_tx.send(None);
                        break;
                    }
                    Ok(n) => {
                        if stdin_tx.send(Some(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        let _ = stdin_tx.send(None);
                        break;
                    }
                }
            }
        });

        loop {
            let progress = self.pump_engine_turns();
            // Advance the activity spinner (returns true while the pane is
            // visible — the streaming turn's spinner animates each tick).
            let spinner_changed = self.activity.borrow_mut().advance();
            if self.exit_flag.get()
                || sigint.load(std::sync::atomic::Ordering::Relaxed)
                || sigterm.load(std::sync::atomic::Ordering::Relaxed)
            {
                break;
            }
            if term.borrow_mut().take_resize_pending() {
                let width = term.borrow().columns();
                let rows = term.borrow().rows();
                self.apply_layout(width, rows);
                tui.request_render();
            }
            match stdin_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(Some(bytes)) => {
                    let chunk = String::from_utf8_lossy(&bytes).into_owned();
                    // Route complete sequences (after Kitty negotiation +
                    // buffer splitting) into the TUI input handler. Panel
                    // navigation keys are routed at the app layer because the
                    // TUI only feeds the focused (editor) child.
                    let mut on_input = |data: &str| {
                        let picker_open = self.session_picker.borrow().is_some();
                        let tasks_open = self.tasks_panel.borrow().is_open();
                        let btw_open = self.btw_panel.borrow().is_open();
                        // Esc closes an open modal panel first.
                        if matches_key(data, "escape") && (tasks_open || btw_open) {
                            if tasks_open {
                                self.tasks_panel.borrow_mut().set_open(false);
                            }
                            if btw_open {
                                self.btw_controller.borrow_mut().close_or_cancel();
                                self.btw_panel.borrow_mut().close();
                                // Cancel the side-agent turn so its output
                                // stops streaming (it would otherwise leak
                                // into the transcript as a stray turn).
                                self.backend.cancel_current_turn();
                                self.backend.btw_turn_ids.clear();
                                self.btw_turn_id = None;
                            }
                            return;
                        }
                        if picker_open {
                            // The picker replaces the editor: all input goes to
                            // the focused editor slot (which delegates to it).
                            tui.handle_input(data);
                            return;
                        }
                        // When the editor is empty, Up/Down scroll the
                        // transcript (mirrors the TS TUI: empty-editor arrows
                        // navigate the history window).
                        let editor_empty = self.editor.borrow().inner().get_text().is_empty();
                        if editor_empty && (matches_key(data, "up") || matches_key(data, "down")) {
                            self.transcript.borrow_mut().handle_input(data);
                        } else {
                            tui.handle_input(data);
                        }
                    };
                    term.borrow_mut().process_stdin_chunk(&chunk, &mut on_input);
                    // Flush an incomplete escape tail (e.g. a lone Esc before
                    // the next byte arrives) so Esc works on non-Kitty
                    // terminals; the stdin buffer would otherwise hold it
                    // until the next read.
                    term.borrow_mut().flush_stdin(&mut on_input);
                    // Poll the session picker for a select/cancel action (the
                    // editor slot has consumed the key; the host applies the
                    // outcome outside the closure's borrows).
                    let picker_action = if self.session_picker.borrow().is_some() {
                        self.session_picker
                            .borrow_mut()
                            .as_mut()
                            .and_then(|p| p.take_action())
                    } else {
                        None
                    };
                    if let Some(action) = picker_action {
                        self.handle_session_picker_action(action);
                    }
                    // Drain Enter-submits the editor queued while handling the
                    // chunk (outside its borrow, so the transcript is writable).
                    let submits: Vec<String> = self.submit_inbox.borrow_mut().drain(..).collect();
                    for text in submits {
                        self.handle_submit(&text);
                    }
                    let width = term.borrow().columns();
                    let rows = term.borrow().rows();
                    self.apply_layout(width, rows);
                    tui.request_render();
                    // Ctrl-C / Ctrl-D / Esc set `exit_flag` while handling the
                    // chunk; break immediately instead of blocking on the next
                    // read (raw-mode input is a byte stream, not a signal).
                    if self.exit_flag.get() {
                        break;
                    }
                }
                Ok(None) => break, // stdin EOF
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // No input this tick — re-render only when the engine
                    // turn made progress (streaming assistant output) or the
                    // activity spinner advanced.
                    if progress || spinner_changed {
                        tui.request_render();
                    }
                }
                Err(_) => break,
            }
        }

        tui.stop();
    }

    /// Route a submitted line: `!` bash-mode lines go to the shell backend;
    /// everything else goes through slash-command dispatch. Clears the editor
    /// after handling (the post-submit reset the TS `CustomEditor` performs).
    pub fn handle_submit(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let is_bash = self.editor.borrow().input_mode == InputMode::Bash;
        if is_bash {
            self.handle_bash_line(text);
        } else {
            let action = dispatch_input(
                app_slash_commands(),
                &HashMap::new(),
                &HashSet::new(),
                self.session.streaming(),
                self.session.compacting(),
                text,
            );
            self.dispatch_action(action);
        }
        self.reset_editor_after_submit();
        self.sync_activity_mode();
    }

    /// `!` shell-mode submit: enqueue as a bash line, echo it into the
    /// transcript (bullet suppressed, like the wire `shell_command` echo), and
    /// flush the queue through the backend stub.
    fn handle_bash_line(&mut self, text: &str) {
        self.session.enqueue_message(text, Some("bash"));
        self.push_user(text, Some(String::new()));
        self.session.flush_queued_messages(&mut self.backend);
        self.update_queue_pane();
    }

    /// Apply a [`DispatchAction`] from slash-command resolution. Builtin
    /// command bodies live in the per-command `handle_*` methods below, keeping
    /// this router readable.
    fn dispatch_action(&mut self, action: DispatchAction) {
        match action {
            DispatchAction::SendNormal(msg) => {
                self.dispatch_normal_message(&msg);
            }
            DispatchAction::RunBuiltin { name, args } => self.handle_builtin(&name, &args),
            DispatchAction::RunSkill { skill_name, .. } => {
                self.push_status(&format!("skill 命令未接线：{skill_name}"), None);
            }
            DispatchAction::RunPluginCommand { .. } => {
                self.push_status("plugin 命令未接线", None);
            }
            DispatchAction::ShowError(msg) => {
                self.push_status(&msg, Some(ColorToken::Error));
            }
        }
    }

    /// Dispatch one normal (non-slash) message. When the agent is streaming
    /// and `busy_input_mode` is `steer`, the text is injected into the running
    /// turn (P6) instead of being enqueued — the TS `steerMessage` path. Queue
    /// mode (or no live turn to steer into) falls back to enqueue + flush.
    fn dispatch_normal_message(&mut self, msg: &str) {
        let intent = resolve_busy_input_intent(
            !self.session.streaming(),
            self.session.compacting(),
            true,
            self.session.busy_input_mode(),
        );
        if intent == BusyInputIntent::Steer {
            self.push_user(msg, None);
            if self.backend.inject_steer(msg) {
                self.update_queue_pane();
                return;
            }
            // No live main turn to steer into → don't lose the input; enqueue.
        }
        self.session.enqueue_message(msg, None);
        self.push_user(msg, None);
        self.session.flush_queued_messages(&mut self.backend);
        self.update_queue_pane();
        self.record_last_prompt(msg);
    }

    /// Route one builtin slash command to its handler. `name` is always the
    /// canonical registry name — `dispatch_input` resolves aliases, so
    /// `/clear` arrives as `new`, `/quit` as `exit`, `/?` as `help`, …
    fn handle_builtin(&mut self, name: &str, args: &str) {
        match name {
            // ── A. Pure / local-state commands ──
            "help" => self.handle_help(),
            "new" => self.handle_new(),
            "version" => self.handle_version(),
            "status" => self.handle_status(),
            "usage" => self.handle_usage(),
            "title" => self.handle_title(args.trim()),
            "theme" => self.handle_theme(args.trim()),
            "effort" => self.handle_effort(args.trim()),
            "permission" => self.handle_permission(args.trim()),
            "yolo" => self.handle_yolo(args.trim()),
            "auto" => self.handle_auto(args.trim()),
            "plan" => self.handle_plan(args.trim()),
            "copy" => self.handle_copy(),
            "export-md" => self.handle_export_md(args.trim()),
            "model" => self.handle_model(args.trim()),
            // ── B. Engine/session-dependent ──
            "compact" => self.handle_compact(args.trim()),
            "undo" => self.handle_undo(args.trim()),
            "sessions" => self.handle_sessions(),
            "tasks" => self.handle_tasks(),
            "wire" => self.handle_wire(args.trim()),
            // `/exit` / `/quit` leave the TUI (the event loop's `tui.stop()`
            // restores the terminal when the exit flag is set).
            "exit" => self.handle_exit(),
            // ── Not wired in slice 6b: SDK / panel / engine dependencies ──
            "mcp" => self.handle_unwired("mcp", "SDK 依赖：MCP server 状态未接入"),
            "plugins" => self.handle_unwired("plugins", "SDK 依赖：插件系统未接入"),
            "add-dir" => self.handle_unwired("add-dir", "SDK 依赖：工作区目录管理未接入"),
            "experiments" => self.handle_unwired("experiments", "SDK/面板依赖：实验特性面板未实现"),
            "reload" => self.handle_unwired("reload", "SDK 依赖：配置热重载未接入"),
            "reload-tui" => self.handle_unwired("reload-tui", "SDK 依赖：tui.toml 热重载未接入"),
            "editor" => self.handle_unwired("editor", "配置/面板依赖：外部编辑器选择面板未实现"),
            "provider" => self.handle_unwired("provider", "SDK 依赖：provider 管理未接入"),
            "secondary_model" => self.handle_unwired(
                "secondary_model",
                "引擎/SDK 依赖：secondary model 配置未接入",
            ),
            "settings" => self.handle_unwired("settings", "面板依赖：settings 面板未实现"),
            "feedback" => self.handle_unwired("feedback", "SDK/面板依赖：feedback 提交未接入"),
            "btw" => self.handle_btw(args),
            "swarm" => self.handle_unwired("swarm", "引擎/SDK 依赖：swarm 模式未接入"),
            "init" => self.handle_unwired("init", "引擎/SDK 依赖：AGENTS.md 分析未接入"),
            "fork" => self.handle_unwired("fork", "引擎/SDK 依赖：session fork 未接入"),
            "export-debug-zip" => {
                self.handle_unwired("export-debug-zip", "SDK 依赖：debug ZIP 导出未接入")
            }
            "login" => self.handle_unwired("login", "SDK 依赖：auth 流程未接入"),
            "logout" => self.handle_unwired("logout", "SDK 依赖：auth 流程未接入"),
            "web" => self.handle_unwired("web", "SDK/面板依赖：Web UI 启动未接入"),
            // Safety net: every registry command is matched above; this branch
            // only fires for a hypothetical registry addition.
            other if find_builtin_slash_command(other).is_some() => {
                self.push_status(&format!("/{other} 未实现"), None);
            }
            other => {
                self.push_status(&format!("未知命令：/{other}"), Some(ColorToken::Error));
            }
        }
    }

    /// `/help` — append a status row listing the implemented commands and the
    /// full builtin registry (39 commands + aliases).
    fn handle_help(&mut self) {
        let names: Vec<&str> = builtin_slash_commands()
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        let help = format!(
            "Dimi TUI（Rust，slice 6c）· 已实现：/version /status /usage /title /theme /effort \
             /permission /yolo /auto /plan /copy /export-md /model /undo /sessions /tasks /btw /new \
             /help /wire /exit\n内置命令（{}）：{}",
            names.len(),
            names.join(" ")
        );
        self.push_status(&help, None);
    }

    /// `/new` / `/clear` — start a fresh session: clear the transcript and
    /// reset the session runtime (queue / streaming / title / plan mode).
    fn handle_new(&mut self) {
        self.transcript.borrow_mut().clear();
        if self.session.current_session_id().is_some() {
            let _ = self.session.create_session(&mut self.backend);
            self.record_current_session();
            self.update_queue_pane();
        }
    }

    /// `/wire <path>` — cold-rebuild the transcript from a wire.jsonl file.
    fn handle_wire(&mut self, path: &str) {
        let result = self.transcript.borrow_mut().load_wire(Path::new(path));
        if let Err(e) = result {
            self.push_status(&format!("wire 加载失败：{e}"), Some(ColorToken::Error));
        }
    }

    /// `/exit` / `/quit` — arm the exit flag (the event loop breaks and
    /// restores the terminal).
    fn handle_exit(&mut self) {
        self.exit_flag.set(true);
    }

    /// `/version` — `Dimi v{version}` (port of the TS `version` case:
    /// `host.showStatus(Dimi v${version})`).
    fn handle_version(&mut self) {
        self.push_status(&format!("Dimi v{}", env!("CARGO_PKG_VERSION")), None);
    }

    /// `/status` — one-line runtime status (port of `showStatusReport`, which
    /// renders a panel in the TS; the panel is a later slice, so this is a
    /// Status row).
    fn handle_status(&mut self) {
        let model = self
            .config
            .model
            .clone()
            .unwrap_or_else(|| "(未设置)".to_string());
        let work_dir = self
            .config
            .work_dir
            .clone()
            .unwrap_or_else(|| "(未设置)".to_string());
        let session_id = self.session.current_session_id().unwrap_or("(无)");
        let title = self.session.title().unwrap_or("(未设置)");
        let streaming = if self.session.streaming() {
            "streaming"
        } else {
            "idle"
        };
        let theme = self
            .config
            .theme
            .clone()
            .unwrap_or_else(|| "dark".to_string());
        let permission = self.session.permission_mode().unwrap_or("manual");
        let plan = if self.session.plan_mode() {
            "on"
        } else {
            "off"
        };
        let effort = self
            .config
            .thinking_effort
            .clone()
            .unwrap_or_else(|| "off".to_string());
        self.push_status(
            &format!(
                "status · model: {model} · work_dir: {work_dir} · session: {session_id} · \
                 title: {title} · streaming: {streaming} · theme: {theme} · permission: \
                 {permission} · plan: {plan} · effort: {effort}"
            ),
            None,
        );
    }

    /// `/usage` — token usage. EngineBackend does not track token counts yet
    /// (slice 6a/6b), so both values report "N/A" (port of `showUsage`).
    fn handle_usage(&mut self) {
        self.push_status(
            "usage · context_tokens: N/A · max_context_tokens: N/A",
            None,
        );
    }

    /// `/title <text>` — set (or show) the session title (port of
    /// `handleTitleCommand`). Stored on the [`SessionManager`].
    fn handle_title(&mut self, title: &str) {
        if title.is_empty() {
            match self.session.title() {
                Some(t) if !t.is_empty() => {
                    self.push_status(&format!("Session title: {t}"), None);
                }
                _ => {
                    let id = self.session.current_session_id().unwrap_or("(无)");
                    self.push_status(&format!("Session title: (not set) — id: {id}"), None);
                }
            }
            return;
        }
        let new_title: String = title.chars().take(200).collect();
        self.session.set_title(Some(new_title.clone()));
        // Mirror the title onto the `/sessions` picker row for this session.
        let current_id = self.session.current_session_id().map(str::to_owned);
        if let Some(row) = self
            .known_sessions
            .iter_mut()
            .find(|r| Some(r.id.as_str()) == current_id.as_deref())
        {
            row.title = Some(new_title.clone());
        }
        self.push_status(&format!("Session title set to: {new_title}"), None);
    }

    /// `/theme <dark|light>` — switch the global palette (port of
    /// `handleThemeCommand`'s apply step; the TS theme picker for an empty
    /// argument is a later slice).
    fn handle_theme(&mut self, theme: &str) {
        let palette = match theme {
            "dark" => Some(DARK_COLORS),
            "light" => Some(LIGHT_COLORS),
            _ => None,
        };
        let Some(palette) = palette else {
            if theme.is_empty() {
                self.push_status("Usage: /theme <dark|light>", Some(ColorToken::Error));
            } else {
                self.push_status(&format!("Unknown theme: {theme}"), Some(ColorToken::Error));
            }
            return;
        };
        set_palette(palette);
        self.config.theme = Some(theme.to_string());
        self.invalidate_chrome();
        self.push_status(&format!("Theme set to \"{theme}\"."), None);
    }

    /// `/effort <off|low|high|max>` — set the thinking effort (port of
    /// `handleEffortCommand`'s apply step; persisted to config so the next
    /// engine turn forwards it to the provider).
    fn handle_effort(&mut self, effort: &str) {
        if effort.is_empty() {
            self.push_status("Usage: /effort <off|low|high|max>", Some(ColorToken::Error));
            return;
        }
        let effort = effort.to_lowercase();
        match effort.as_str() {
            "off" | "low" | "high" | "max" => {
                self.config.thinking_effort = Some(effort.clone());
                // The engine snapshots the provider per turn — update the
                // backend so the next prompt actually forwards the effort.
                self.backend.set_thinking_effort(&effort);
                self.refresh_footer();
                self.push_status(&format!("Thinking effort set to {effort}."), None);
            }
            other => self.push_status(
                &format!("Unsupported thinking effort \"{other}\". Available: off, low, high, max"),
                Some(ColorToken::Error),
            ),
        }
    }

    /// `/permission <manual|yolo|auto>` — set the permission mode (port of
    /// `applyPermissionModeWithDefault` minus config persistence). Applies to
    /// the session state and the backend's next-turn policy.
    fn handle_permission(&mut self, mode: &str) {
        if mode.is_empty() {
            self.push_status(
                "Usage: /permission <manual|yolo|auto>",
                Some(ColorToken::Error),
            );
            return;
        }
        let mode = mode.to_lowercase();
        match mode.as_str() {
            "manual" | "yolo" | "auto" => {
                self.set_permission(&mode);
                self.push_status(&format!("Permission mode set to {mode}."), None);
            }
            other => self.push_status(
                &format!("Unknown permission mode: {other}. Available: manual, yolo, auto"),
                Some(ColorToken::Error),
            ),
        }
    }

    /// `/yolo [on|off]` — toggle YOLO permission mode (port of
    /// `handleYoloCommand`).
    fn handle_yolo(&mut self, args: &str) {
        let sub = args.trim().to_lowercase();
        let current = self.session.permission_mode().unwrap_or("manual");
        match sub.as_str() {
            "" => {
                if current == "yolo" {
                    self.set_permission("manual");
                    self.push_status("YOLO mode: OFF", None);
                } else {
                    self.set_permission("yolo");
                    self.push_status("YOLO mode: ON", None);
                }
            }
            "on" => {
                if current == "yolo" {
                    self.push_status("YOLO mode is already on", None);
                } else {
                    self.set_permission("yolo");
                    self.push_status("YOLO mode: ON", None);
                }
            }
            "off" => {
                if current != "yolo" {
                    self.push_status("YOLO mode is already off", None);
                } else {
                    self.set_permission("manual");
                    self.push_status("YOLO mode: OFF", None);
                }
            }
            other => self.push_status(
                &format!("Unknown yolo subcommand: {other}"),
                Some(ColorToken::Error),
            ),
        }
    }

    /// `/auto [on|off]` — toggle Auto permission mode (port of
    /// `handleAutoCommand`).
    fn handle_auto(&mut self, args: &str) {
        let sub = args.trim().to_lowercase();
        let current = self.session.permission_mode().unwrap_or("manual");
        match sub.as_str() {
            "" => {
                if current == "auto" {
                    self.set_permission("manual");
                    self.push_status("Auto mode: OFF", None);
                } else {
                    self.set_permission("auto");
                    self.push_status("Auto mode: ON", None);
                }
            }
            "on" => {
                if current == "auto" {
                    self.push_status("Auto mode is already on", None);
                } else {
                    self.set_permission("auto");
                    self.push_status("Auto mode: ON", None);
                }
            }
            "off" => {
                if current != "auto" {
                    self.push_status("Auto mode is already off", None);
                } else {
                    self.set_permission("manual");
                    self.push_status("Auto mode: OFF", None);
                }
            }
            other => self.push_status(
                &format!("Unknown auto subcommand: {other}"),
                Some(ColorToken::Error),
            ),
        }
    }

    /// `/plan [on|off|clear]` — toggle plan mode (port of `handlePlanCommand`;
    /// no session plan file path yet, so `clear` just turns it off).
    fn handle_plan(&mut self, args: &str) {
        let sub = args.trim().to_lowercase();
        match sub.as_str() {
            "clear" => {
                self.session.set_plan_mode(false);
                self.refresh_footer();
                self.push_status("Plan cleared", None);
            }
            "on" => {
                self.session.set_plan_mode(true);
                self.refresh_footer();
                self.push_status("Plan mode: ON", None);
            }
            "off" => {
                self.session.set_plan_mode(false);
                self.refresh_footer();
                self.push_status("Plan mode: OFF", None);
            }
            "" => {
                let enabled = !self.session.plan_mode();
                self.session.set_plan_mode(enabled);
                self.refresh_footer();
                self.push_status(
                    if enabled {
                        "Plan mode: ON"
                    } else {
                        "Plan mode: OFF"
                    },
                    None,
                );
            }
            other => self.push_status(
                &format!("Unknown plan subcommand: {other}"),
                Some(ColorToken::Error),
            ),
        }
    }

    /// `/copy` — copy the last assistant reply to the clipboard (port of
    /// `handleCopyCommand`; the TS native/terminal-escape methods collapse to
    /// a `pbcopy`/`xclip` subprocess here).
    fn handle_copy(&mut self) {
        let text = self.last_assistant_text();
        if text.is_empty() {
            self.push_status("No assistant message to copy.", Some(ColorToken::Warning));
            return;
        }
        match (self.clipboard)(&text) {
            Ok(()) => {
                let count = text.chars().count();
                self.push_status(&format!("Copied to clipboard ({count} characters)."), None);
            }
            Err(msg) => self.push_status(
                &format!("Failed to copy to clipboard: {msg}"),
                Some(ColorToken::Error),
            ),
        }
    }

    /// `/export-md [path]` — export the transcript as Markdown (port of
    /// `handleExportMdCommand`). Default path: `{work_dir}/dimi-export-{short
    /// session id}.md`.
    fn handle_export_md(&mut self, args: &str) {
        let entries = self.transcript.borrow().entries().to_vec();
        if entries.is_empty() {
            self.push_status("No messages to export.", Some(ColorToken::Error));
            return;
        }
        let default_name = match self.session.current_session_id() {
            Some(id) => {
                // TS `session.ts`: `dimi-export-<shortId>-<timestamp>.md` —
                // the timestamp keeps repeated exports from overwriting.
                let short_id = id.chars().take(8).collect::<String>();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("dimi-export-{short_id}-{now}.md")
            }
            None => "dimi-export.md".to_string(),
        };
        let path = if args.is_empty() {
            let base = self
                .config
                .work_dir
                .clone()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            base.join(default_name)
        } else {
            PathBuf::from(args)
        };
        let md = transcript_to_markdown(
            &entries,
            self.session.current_session_id(),
            self.config.work_dir.as_deref(),
        );
        match write_markdown_export(&path, &md) {
            Ok(()) => self.push_status(
                &format!("Exported {} messages to {}", entries.len(), path.display()),
                None,
            ),
            Err(msg) => self.push_status(
                &format!("Failed to export session: {msg}"),
                Some(ColorToken::Error),
            ),
        }
    }

    /// `/model <name>` — switch the active model (port of `handleModelCommand`
    /// with a literal reference; the TS picker + provider catalog are later
    /// slices). Applied to config + backend so the next turn uses it.
    fn handle_model(&mut self, reference: &str) {
        if reference.is_empty() {
            let current = self
                .config
                .model
                .clone()
                .unwrap_or_else(|| "(未设置)".to_string());
            self.push_status(&format!("当前模型：{current}（/model <name> 切换）"), None);
            return;
        }
        self.config.model = Some(reference.to_string());
        self.backend.set_model(reference);
        self.refresh_footer();
        self.push_status(&format!("Switched to {reference}."), None);
    }

    /// `/compact [instruction]` — EngineBackend has no compaction surface yet
    /// (slice 6a/6b), so this reports the unwired status instead of starting
    /// one.
    fn handle_compact(&mut self, _args: &str) {
        self.push_status("命令未接线：/compact（引擎 compaction 未接入）", None);
    }

    /// `/undo [count]` — withdraw the last prompt(s) and everything that
    /// followed from the transcript (port of `handleUndoCommand`'s count path;
    /// the TS undo selector is a later slice).
    fn handle_undo(&mut self, args: &str) {
        let count = match parse_undo_count(args) {
            Some(c) => c,
            None => {
                self.push_status(
                    "Usage: /undo [count], where count is a positive integer.",
                    Some(ColorToken::Error),
                );
                return;
            }
        };
        let (anchor, len) = {
            let entries = self.transcript.borrow();
            let entries = entries.entries();
            let mut found = 0;
            let mut anchor = None;
            // Anchors are user prompts (TS `isUndoAnchorEntry`), newest first;
            // undoing `count` prompts truncates from the `count`-th last user
            // entry onwards (the prompt + its whole response).
            for (i, e) in entries.iter().enumerate().rev() {
                if e.kind == TranscriptEntryKind::User {
                    found += 1;
                    if found == count {
                        anchor = Some(i);
                        break;
                    }
                }
            }
            if anchor.is_none() {
                // Assistant-only tail (e.g. a wire replay with no user anchor):
                // a single undo withdraws the last assistant reply.
                if count == 1 {
                    anchor = entries
                        .iter()
                        .rposition(|e| e.kind == TranscriptEntryKind::Assistant);
                }
            }
            (anchor, entries.len())
        };
        let Some(idx) = anchor else {
            // Distinguish "nothing to undo" from "requested more than
            // available" (TS `formatUndoLimitMessage`): if there is at least
            // one user prompt, a count that found no anchor means over-limit.
            let user_count = self
                .transcript
                .borrow()
                .entries()
                .iter()
                .filter(|e| e.kind == TranscriptEntryKind::User)
                .count();
            if user_count > 0 && count > 1 {
                self.push_status(
                    &format!(
                        "Cannot undo {count} prompts; only {user_count} can be undone in the active context."
                    ),
                    Some(ColorToken::Warning),
                );
            } else {
                self.push_status("Nothing to undo.", None);
            }
            return;
        };
        let removed = len - idx;
        self.transcript.borrow_mut().truncate(idx);
        self.push_status(
            &format!("已撤销最后 {count} 条 prompt（移除 {removed} 条条目）"),
            None,
        );
    }

    /// `/sessions` — open the session picker (replacing the editor, port of
    /// `showSessionPicker` + `mountEditorReplacement`).
    fn handle_sessions(&mut self) {
        self.open_session_picker();
    }

    /// `/tasks` — open the background-tasks panel populated from the engine
    /// backend's tracked tasks (empty list when none).
    fn handle_tasks(&mut self) {
        let tasks: Vec<TaskInfo> = self
            .backend
            .tasks()
            .iter()
            .map(|t| TaskInfo {
                task_id: t.task_id.clone(),
                status: task_status_label(t.status),
                description: t.description.clone(),
            })
            .collect();
        self.tasks_panel.borrow_mut().set_tasks(tasks);
        self.tasks_panel.borrow_mut().set_open(true);
    }

    /// `/btw <prompt>` — open the BTW panel (controller + rendering component)
    /// and start a side-agent turn for the prompt.
    fn handle_btw(&mut self, prompt: &str) {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            self.push_status("Usage: /btw <prompt>", Some(ColorToken::Error));
            return;
        }
        // Open the panel first: it shows the prompt + "Waiting for answer..."
        // immediately. The controller is the state machine; the component
        // mirrors `active()` and keeps the prompt text.
        self.btw_controller.borrow_mut().open(BTW_AGENT_ID, prompt);
        self.btw_panel.borrow_mut().open(prompt);
        // The side turn carries the prompt as a user message (P2); its events
        // are panel-owned (P3/P7).
        if let Some(turn_id) = self.backend.start_btw_turn(prompt) {
            self.btw_turn_id = Some(turn_id);
        }
    }

    /// Status for a builtin whose body depends on an SDK / panel / engine
    /// capability that a later slice wires up.
    fn handle_unwired(&mut self, name: &str, reason: &str) {
        self.push_status(&format!("命令未接线：/{name}（{reason}）"), None);
    }

    /// Apply a permission mode to the session state, the backend's next-turn
    /// policy, and the footer badge.
    fn set_permission(&mut self, mode: &str) {
        self.session.set_permission_mode(Some(mode.to_string()));
        self.backend
            .set_permission_mode(parse_permission_mode(mode));
        self.refresh_footer();
    }

    /// The visible text of the last assistant reply, newest first (port of
    /// `findLastAssistantText`; dimi-cli has no `modelText` flag, so any
    /// non-empty assistant entry counts).
    fn last_assistant_text(&self) -> String {
        self.transcript
            .borrow()
            .entries()
            .iter()
            .rev()
            .find(|e| e.kind == TranscriptEntryKind::Assistant && !e.content.trim().is_empty())
            .map(|e| e.content.clone())
            .unwrap_or_default()
    }

    /// Rebuild the footer state from config + session (model / work_dir /
    /// permission / plan / effort). Called after any command that mutates
    /// those.
    fn refresh_footer(&mut self) {
        let mut footer_state = FooterState::new();
        if let Some(model) = &self.config.model {
            footer_state.model = model.clone();
        }
        if let Some(work_dir) = &self.config.work_dir {
            footer_state.work_dir = work_dir.clone();
        }
        footer_state.permission_mode = self
            .session
            .permission_mode()
            .unwrap_or("manual")
            .to_string();
        footer_state.plan_mode = self.session.plan_mode();
        if let Some(effort) = &self.config.thinking_effort {
            footer_state.thinking_effort = effort.clone();
        }
        self.footer.borrow_mut().update(footer_state);
    }

    /// Force every mounted component to rebuild cached ANSI after a theme
    /// switch (the TS `applyTheme` invalidation step).
    fn invalidate_chrome(&mut self) {
        invalidate_components([
            &mut *self.transcript.borrow_mut() as &mut dyn Component,
            &mut *self.activity.borrow_mut() as &mut dyn Component,
            &mut *self.btw_panel.borrow_mut() as &mut dyn Component,
            &mut *self.queue_pane.borrow_mut() as &mut dyn Component,
            &mut *self.tasks_panel.borrow_mut() as &mut dyn Component,
            &mut *self.footer.borrow_mut() as &mut dyn Component,
            &mut *self.editor.borrow_mut() as &mut dyn Component,
        ]);
        if let Some(picker) = self.session_picker.borrow_mut().as_mut() {
            picker.invalidate();
        }
    }

    /// Rebuild the queue pane from the session's queued messages (empty queue
    /// → the pane renders 0 lines).
    fn update_queue_pane(&mut self) {
        let options = QueuePaneOptions {
            messages: self.session.queued_messages().to_vec(),
            is_compacting: self.session.compacting(),
            is_streaming: self.session.streaming(),
            can_steer_immediately: true,
            enter_steers_by_default: self.session.busy_input_mode() == BusyInputMode::Steer,
        };
        self.queue_pane.borrow_mut().update(options);
    }

    /// Sync the activity pane from the session streaming state (turn running →
    /// composing spinner; idle → hidden).
    fn sync_activity_mode(&mut self) {
        let mode = if self.session.streaming() {
            ActivityMode::Composing
        } else {
            ActivityMode::Idle
        };
        self.activity.borrow_mut().set_mode(mode);
    }

    /// Record the current session in the `/sessions` picker list (upsert by
    /// id; refresh work_dir + updated_at).
    fn record_current_session(&mut self) {
        let Some(id) = self.session.current_session_id().map(str::to_owned) else {
            return;
        };
        let work_dir = self.config.work_dir.clone().unwrap_or_default();
        let now = now_ms();
        if let Some(row) = self.known_sessions.iter_mut().find(|r| r.id == id) {
            row.work_dir = work_dir;
            row.updated_at = now;
        } else {
            self.known_sessions.push(SessionRow {
                id,
                title: None,
                last_prompt: None,
                work_dir,
                updated_at: now,
            });
        }
    }

    /// Mirror a submitted prompt onto the current session's picker row.
    fn record_last_prompt(&mut self, text: &str) {
        let current_id = self.session.current_session_id().map(str::to_owned);
        if let Some(row) = self
            .known_sessions
            .iter_mut()
            .find(|r| Some(r.id.as_str()) == current_id.as_deref())
        {
            row.last_prompt = Some(text.to_owned());
        }
    }

    /// Open the `/sessions` picker (replacing the editor in the mount tree).
    fn open_session_picker(&mut self) {
        let rows = self.known_sessions.clone();
        let current = self
            .session
            .current_session_id()
            .map(str::to_owned)
            .unwrap_or_default();
        let options = SessionPickerOptions {
            sessions: rows,
            loading: false,
            current_session_id: current.clone(),
            scope: SessionScope::Cwd,
            initial_selected_session_id: Some(current),
            page_size: None,
            max_visible_sessions: Some(4),
            has_toggle_scope: false,
            home: Some(std::env::var("HOME").unwrap_or_default()),
            now_ms: Some(now_ms()),
        };
        // The picker replaces the editor, so drop the editor's cursor marker.
        self.editor.borrow_mut().inner_mut().set_focused(false);
        *self.session_picker.borrow_mut() = Some(SessionPickerComponent::new(options));
    }

    /// Close the session picker and restore the editor (port of
    /// `hideSessionPicker` → `restoreEditor`).
    fn close_session_picker(&mut self) {
        *self.session_picker.borrow_mut() = None;
        self.editor.borrow_mut().inner_mut().set_focused(true);
    }

    /// Apply a session-picker action surfaced by the component (`onSelect` /
    /// `onCancel` / `onCtrlC` / `onCtrlD` in the TS).
    fn handle_session_picker_action(&mut self, action: SessionPickerAction) {
        match action {
            SessionPickerAction::Select(row) => {
                if self
                    .session
                    .switch_session(&mut self.backend, &row.id)
                    .is_ok()
                {
                    self.refresh_footer();
                    self.update_queue_pane();
                    self.push_status(&format!("Resumed session ({}).", row.id), None);
                } else {
                    self.push_status(
                        &format!("Failed to switch session: {}", row.id),
                        Some(ColorToken::Error),
                    );
                }
                self.close_session_picker();
            }
            SessionPickerAction::Cancel => self.close_session_picker(),
            SessionPickerAction::CtrlC | SessionPickerAction::CtrlD => {
                self.close_session_picker();
                self.exit_flag.set(true);
            }
            SessionPickerAction::ToggleScope(_) => {
                // dimi-cli has no cross-scope session fetch (has_toggle_scope
                // is false), so the picker never surfaces this.
            }
        }
    }

    /// Route drained engine events to the BTW panel when a `/btw` turn is
    /// active: convert the engine events (assistant/thinking deltas, turn
    /// end) to the dimi-tui event taxonomy, feed `BtwPanelController::route_event`,
    /// and mirror the controller state into the rendering component.
    fn route_btw_events(&mut self, events: &[EngineEvent]) {
        if self.btw_controller.borrow().active().is_none() {
            return;
        }
        let Some(btw_turn_id) = self.btw_turn_id else {
            return;
        };
        let mut consumed = false;
        let mut btw_ended = false;
        for event in events {
            let tui_event = match event {
                EngineEvent::AssistantDelta { turn_id, delta } if *turn_id == btw_turn_id => {
                    Some(Event::AssistantDelta {
                        agent_id: Some(BTW_AGENT_ID.to_owned()),
                        delta: delta.clone(),
                    })
                }
                EngineEvent::ThinkingDelta { turn_id, delta } if *turn_id == btw_turn_id => {
                    Some(Event::ThinkingDelta {
                        agent_id: Some(BTW_AGENT_ID.to_owned()),
                        delta: delta.clone(),
                    })
                }
                EngineEvent::TurnEnded {
                    turn_id,
                    reason,
                    error,
                    ..
                } if *turn_id == btw_turn_id => {
                    btw_ended = true;
                    let reason = match reason.as_str() {
                        "completed" => TurnEndReason::Completed,
                        "cancelled" => TurnEndReason::Cancelled,
                        "blocked" => TurnEndReason::Blocked,
                        _ => TurnEndReason::Failed,
                    };
                    let error_payload = error.as_ref().and_then(|e| {
                        let code = e.get("code")?.as_str()?.to_owned();
                        let message = e
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or_default()
                            .to_owned();
                        Some(ErrorPayload {
                            code,
                            message,
                            details: None,
                        })
                    });
                    Some(Event::TurnEnded {
                        agent_id: Some(BTW_AGENT_ID.to_owned()),
                        turn_id: turn_id.to_string(),
                        reason,
                        error: error_payload,
                    })
                }
                _ => None,
            };
            if let Some(tui_event) = tui_event
                && self.btw_controller.borrow_mut().route_event(&tui_event)
            {
                consumed = true;
            }
        }
        if consumed && let Some(state) = self.btw_controller.borrow().active().cloned() {
            self.btw_panel.borrow_mut().sync(&state);
        }
        if btw_ended {
            // The turn is done — drop the btw-turn bookkeeping so a later
            // `/btw` starts clean and the backend stops skipping its turn id.
            self.btw_turn_id = None;
            self.backend.btw_turn_ids.remove(&btw_turn_id);
        }
    }

    fn push_status(&mut self, content: &str, color: Option<ColorToken>) {
        self.transcript
            .borrow_mut()
            .push(status_entry(content, color));
    }

    fn push_user(&mut self, content: &str, bullet: Option<String>) {
        self.transcript
            .borrow_mut()
            .push(user_entry(content, bullet));
    }

    /// Post-submit editor reset: back to prompt mode with an empty buffer.
    fn reset_editor_after_submit(&mut self) {
        let mut ed = self.editor.borrow_mut();
        ed.set_input_mode(InputMode::Prompt);
        ed.inner_mut().set_text("");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_tui::theme::{DARK_COLORS, current_theme, set_palette};
    use dimi_tui::wire_transcript::TranscriptEntryKind;

    fn wire_fixture() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dimi-tui/testdata/sample-wire.jsonl")
            .to_string_lossy()
            .into_owned()
    }

    fn entries(app: &DimiApp) -> Vec<dimi_tui::wire_transcript::TranscriptEntry> {
        app.transcript.borrow().entries().to_vec()
    }

    #[test]
    fn new_starts_with_an_active_session() {
        let app = DimiApp::new(Config::default());
        assert!(app.session.current_session_id().is_some());
    }

    #[test]
    fn handle_submit_help_appends_help_status() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/help");
        let e = entries(&app);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, TranscriptEntryKind::Status);
        assert!(e[0].content.contains("/help"), "content: {}", e[0].content);
        assert!(e[0].content.contains("/wire"));
        assert!(
            e[0].content.contains("model"),
            "lists builtins: {}",
            e[0].content
        );
    }

    #[test]
    fn handle_submit_normal_message_appends_user_and_not_configured_status() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("hello");
        let e = entries(&app);
        assert_eq!(e.len(), 2, "user + provider-not-configured status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[0].content, "hello");
        assert_eq!(e[1].kind, TranscriptEntryKind::Status);
        assert!(
            e[1].content.contains("未配置 provider"),
            "content: {}",
            e[1].content
        );
        assert_eq!(e[1].status_color, Some(ColorToken::Error));
    }

    #[test]
    fn handle_submit_empty_is_noop() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("   ");
        assert!(entries(&app).is_empty());
        assert!(app.editor.borrow().inner().get_text().is_empty());
    }

    #[test]
    fn handle_submit_clear_empties_transcript() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("hello");
        assert_eq!(entries(&app).len(), 2);
        app.handle_submit("/clear");
        assert!(entries(&app).is_empty(), "transcript should be cleared");
    }

    #[test]
    fn handle_submit_wire_loads_fixture() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit(&format!("/wire {}", wire_fixture()));
        let e = entries(&app);
        assert_eq!(e.len(), 6, "sample-wire entries: {e:#?}");
        assert_eq!(e[0].content, "Hello there!");
    }

    #[test]
    fn handle_submit_wire_missing_file_appends_error_status() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/wire /nonexistent/wire.jsonl");
        let e = entries(&app);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, TranscriptEntryKind::Status);
        assert!(e[0].content.contains("wire 加载失败"));
        assert_eq!(e[0].status_color, Some(ColorToken::Error));
    }

    #[test]
    fn handle_submit_unwired_builtin_appends_not_wired() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        // `/mcp` is a registry command whose body depends on the SDK MCP
        // surface (a later slice) → "命令未接线" status with the reason.
        app.handle_submit("/mcp");
        let e = entries(&app);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, TranscriptEntryKind::Status);
        assert!(e[0].content.contains("未接线"), "content: {}", e[0].content);
        assert!(e[0].content.contains("/mcp"));
    }

    #[test]
    fn handle_submit_unknown_slash_command_is_a_normal_message() {
        // TS semantics: an unknown slash command falls through to a message.
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/does-not-exist");
        let e = entries(&app);
        assert_eq!(e.len(), 2, "message + status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[0].content, "/does-not-exist");
    }

    #[test]
    fn handle_submit_version_appends_version_status() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/version");
        let e = entries(&app);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, TranscriptEntryKind::Status);
        assert!(
            e[0].content
                .contains(&format!("Dimi v{}", env!("CARGO_PKG_VERSION"))),
            "content: {}",
            e[0].content
        );
    }

    #[test]
    fn handle_submit_theme_light_switches_palette() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/theme light");
        assert_eq!(current_theme().color(ColorToken::Primary), "#1565C0");
        assert_eq!(app.config.theme.as_deref(), Some("light"));
        let e = entries(&app);
        assert!(
            e[0].content.contains("Theme set to \"light\""),
            "{}",
            e[0].content
        );
        // Restore the global palette so other tests start from dark.
        set_palette(DARK_COLORS);
    }

    #[test]
    fn handle_submit_theme_unknown_errors() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/theme neon");
        let e = entries(&app);
        assert_eq!(e[0].status_color, Some(ColorToken::Error));
        assert!(e[0].content.contains("Unknown theme"), "{}", e[0].content);
    }

    #[test]
    fn handle_submit_title_sets_session_title() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/title my project");
        assert_eq!(app.session.title(), Some("my project"));
        let e = entries(&app);
        assert!(
            e[0].content.contains("Session title set to: my project"),
            "{}",
            e[0].content
        );
    }

    #[test]
    fn handle_submit_title_without_args_shows_current() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/title");
        let e = entries(&app);
        assert!(e[0].content.contains("(not set)"), "{}", e[0].content);
        // Setting then querying shows the stored title.
        app.handle_submit("/title foo");
        app.handle_submit("/title");
        let e = entries(&app);
        assert!(
            e.last().unwrap().content.contains("Session title: foo"),
            "{}",
            e.last().unwrap().content
        );
    }

    #[test]
    fn handle_submit_permission_yolo_updates_session_and_footer() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/permission yolo");
        assert_eq!(app.session.permission_mode(), Some("yolo"));
        let e = entries(&app);
        assert!(
            e[0].content.contains("Permission mode set to yolo"),
            "{}",
            e[0].content
        );
        let footer = app.footer.borrow();
        assert_eq!(footer.state.permission_mode, "yolo");
    }

    #[test]
    fn handle_submit_yolo_auto_shortcuts_toggle() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/yolo");
        assert_eq!(app.session.permission_mode(), Some("yolo"));
        app.handle_submit("/yolo off");
        assert_eq!(app.session.permission_mode(), Some("manual"));
        app.handle_submit("/auto");
        assert_eq!(app.session.permission_mode(), Some("auto"));
        // `on` when already active is a no-op status.
        app.handle_submit("/auto on");
        assert_eq!(app.session.permission_mode(), Some("auto"));
        let e = entries(&app);
        assert!(
            e.last().unwrap().content.contains("already on"),
            "{}",
            e.last().unwrap().content
        );
    }

    #[test]
    fn handle_submit_effort_updates_config_and_backend() {
        set_palette(DARK_COLORS);
        let mut config = Config::default();
        config.model = Some("m".to_owned());
        config.base_url = Some("http://example.test/v1".to_owned());
        config.api_key = Some("k".to_owned());
        let mut app = DimiApp::new(config);
        app.handle_submit("/effort high");
        assert_eq!(app.config.thinking_effort.as_deref(), Some("high"));
        assert_eq!(
            app.backend
                .provider
                .as_ref()
                .and_then(|p| p.thinking_effort.as_deref()),
            Some("high"),
            "/effort must reach the backend provider so the next turn forwards it"
        );
        let e = entries(&app);
        assert!(
            e[0].content.contains("Thinking effort set to high"),
            "{}",
            e[0].content
        );
        // Invalid effort errors.
        app.handle_submit("/effort extreme");
        let e = entries(&app);
        assert_eq!(e[1].status_color, Some(ColorToken::Error));
        assert!(
            e[1].content.contains("Unsupported thinking effort"),
            "{}",
            e[1].content
        );
    }

    #[test]
    fn handle_submit_plan_toggles() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        assert!(!app.session.plan_mode());
        app.handle_submit("/plan");
        assert!(app.session.plan_mode());
        app.handle_submit("/plan off");
        assert!(!app.session.plan_mode());
        app.handle_submit("/plan on");
        assert!(app.session.plan_mode());
        app.handle_submit("/plan clear");
        assert!(!app.session.plan_mode());
    }

    #[test]
    fn handle_submit_model_updates_config_and_backend() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/model gpt-5");
        assert_eq!(app.config.model.as_deref(), Some("gpt-5"));
        let e = entries(&app);
        assert!(
            e[0].content.contains("Switched to gpt-5"),
            "{}",
            e[0].content
        );
    }

    #[test]
    fn handle_submit_compact_shows_unwired() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/compact");
        let e = entries(&app);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, TranscriptEntryKind::Status);
        assert!(e[0].content.contains("未接线"), "{}", e[0].content);
        assert!(e[0].content.contains("/compact"));
    }

    #[test]
    fn handle_submit_undo_removes_last_turn() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.transcript.borrow_mut().push(user_entry("q1", None));
        app.transcript.borrow_mut().push(assistant_entry("a1"));
        app.transcript.borrow_mut().push(status_entry("s1", None));
        app.transcript.borrow_mut().push(user_entry("q2", None));
        app.transcript.borrow_mut().push(assistant_entry("a2"));
        app.handle_submit("/undo");
        let e = entries(&app);
        // q2 + its response a2 are withdrawn; the undo status is appended.
        assert_eq!(e.len(), 4, "q1 + a1 + s1 + undo status: {e:#?}");
        assert_eq!(e[0].content, "q1");
        assert_eq!(e[1].content, "a1");
        assert_eq!(e[2].content, "s1");
        assert_eq!(e[3].kind, TranscriptEntryKind::Status);
        assert!(e[3].content.contains("已撤销"), "{}", e[3].content);
    }

    #[test]
    fn handle_submit_undo_nothing_when_empty() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/undo");
        let e = entries(&app);
        assert!(e[0].content.contains("Nothing to undo"), "{}", e[0].content);
    }

    #[test]
    fn handle_submit_export_md_writes_file() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.transcript
            .borrow_mut()
            .push(user_entry("a question", None));
        app.transcript
            .borrow_mut()
            .push(assistant_entry("an answer"));
        let dir = std::env::temp_dir().join(format!("dimi_cli_export_md_{}", std::process::id()));
        let path = dir.join("export.md");
        std::fs::remove_dir_all(&dir).ok();
        app.handle_submit(&format!("/export-md {}", path.display()));
        let content = std::fs::read_to_string(&path).expect("export file exists");
        assert!(content.contains("a question"), "content: {content}");
        assert!(content.contains("an answer"), "content: {content}");
        let e = entries(&app);
        assert!(
            e.last().unwrap().content.contains("Exported 2 messages"),
            "status: {}",
            e.last().unwrap().content
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn handle_submit_export_md_empty_transcript_errors() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/export-md /tmp/never-written.md");
        let e = entries(&app);
        assert_eq!(e[0].status_color, Some(ColorToken::Error));
        assert!(
            e[0].content.contains("No messages to export"),
            "{}",
            e[0].content
        );
        assert!(!Path::new("/tmp/never-written.md").exists());
    }

    #[test]
    fn handle_submit_copy_no_assistant_shows_warning() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/copy");
        let e = entries(&app);
        assert_eq!(e[0].status_color, Some(ColorToken::Warning));
        assert!(
            e[0].content.contains("No assistant message"),
            "{}",
            e[0].content
        );
    }

    #[test]
    fn handle_submit_copy_copies_last_assistant() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.transcript.borrow_mut().push(user_entry("hi", None));
        app.transcript
            .borrow_mut()
            .push(assistant_entry("hello world"));
        app.transcript
            .borrow_mut()
            .push(status_entry("ignored", None));
        let captured: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let probe = captured.clone();
        app.clipboard = Box::new(move |text: &str| {
            *probe.borrow_mut() = Some(text.to_owned());
            Ok(())
        });
        app.handle_submit("/copy");
        assert_eq!(captured.borrow().as_deref(), Some("hello world"));
        let e = entries(&app);
        assert!(
            e.last().unwrap().content.contains("Copied to clipboard"),
            "{}",
            e.last().unwrap().content
        );
    }

    #[test]
    fn handle_submit_status_reports_fields() {
        set_palette(DARK_COLORS);
        let config = Config {
            model: Some("gpt-5".to_string()),
            work_dir: Some("/tmp/proj".to_string()),
            theme: Some("dark".to_string()),
            ..Config::default()
        };
        let mut app = DimiApp::new(config);
        app.session.set_title(Some("titled".to_string()));
        app.handle_submit("/status");
        let e = entries(&app);
        let content = &e[0].content;
        assert!(content.contains("model: gpt-5"), "{content}");
        assert!(content.contains("work_dir: /tmp/proj"), "{content}");
        assert!(content.contains("session: session-1"), "{content}");
        assert!(content.contains("title: titled"), "{content}");
        assert!(content.contains("theme: dark"), "{content}");
        assert!(content.contains("streaming: idle"), "{content}");
    }

    #[test]
    fn handle_submit_usage_reports_na() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/usage");
        let e = entries(&app);
        assert!(
            e[0].content.contains("context_tokens: N/A"),
            "{}",
            e[0].content
        );
        assert!(
            e[0].content.contains("max_context_tokens: N/A"),
            "{}",
            e[0].content
        );
    }

    #[test]
    fn handle_submit_help_lists_all_builtins() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/help");
        let e = entries(&app);
        let content = &e[0].content;
        for command in builtin_slash_commands() {
            assert!(
                content.contains(&command.name),
                "/{} missing from help: {content}",
                command.name
            );
        }
        assert!(content.contains("/wire"));
    }

    #[test]
    fn handle_submit_sessions_opens_the_picker() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/sessions");
        // The picker replaces the editor (no transcript status row).
        assert!(entries(&app).is_empty());
        let mut picker = app.session_picker.borrow_mut();
        assert!(picker.is_some(), "/sessions should mount the picker");
        let lines = picker.as_mut().unwrap().render(80);
        let joined = dimi_tui::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Sessions"), "{joined}");
        assert!(joined.contains("session-1"), "{joined}");
    }

    #[test]
    fn handle_submit_tasks_opens_the_panel() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/tasks");
        // No transcript status row; the panel opens showing the empty list.
        assert!(entries(&app).is_empty());
        {
            let mut panel = app.tasks_panel.borrow_mut();
            assert!(panel.is_open(), "/tasks should open the tasks panel");
            let joined = dimi_tui::ansi::strip_ansi(&panel.render(80).join("\n"));
            assert!(joined.contains("No background tasks"), "{joined}");
        }
    }

    #[test]
    fn handle_tasks_renders_backend_tracked_tasks() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        // The engine backend tracks background tasks (task.started / settled);
        // seed one directly and confirm `/tasks` renders it.
        app.backend.tasks.push(BackgroundTaskInfo {
            task_id: "task_1".to_owned(),
            kind: BackgroundTaskKind::Agent,
            status: BackgroundTaskStatus::Running,
            agent_id: None,
            description: Some("run tests".to_owned()),
            exit_code: None,
            stop_reason: None,
            detached: None,
        });
        app.handle_submit("/tasks");
        let joined = {
            let mut panel = app.tasks_panel.borrow_mut();
            dimi_tui::ansi::strip_ansi(&panel.render(80).join("\n"))
        };
        assert!(joined.contains("task_1"), "{joined}");
        assert!(joined.contains("running"), "{joined}");
        assert!(joined.contains("run tests"), "{joined}");
    }

    #[test]
    fn handle_btw_opens_panel_even_without_provider() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/btw hello");
        // The panel opens and shows the prompt + waiting state even though the
        // engine turn cannot start (no provider → backend pushes the
        // not-configured status).
        let joined = {
            let mut panel = app.btw_panel.borrow_mut();
            assert!(panel.is_open());
            dimi_tui::ansi::strip_ansi(&panel.render(80).join("\n"))
        };
        assert!(joined.contains("Q: hello"), "{joined}");
        assert!(joined.contains("Waiting for answer"), "{joined}");
    }

    #[test]
    fn handle_btw_without_prompt_errors() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/btw");
        let e = entries(&app);
        assert_eq!(e[0].status_color, Some(ColorToken::Error));
        assert!(e[0].content.contains("Usage: /btw"), "{}", e[0].content);
        assert!(!app.btw_panel.borrow().is_open());
    }

    #[test]
    fn render_lines_shows_queue_when_messages_queued() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        // Streaming gates flush → the message stays in the queue pane.
        app.session.set_streaming(true);
        app.session.enqueue_message("queued prompt", None);
        app.update_queue_pane();
        let joined = app.render_lines(80, 10).join("\n");
        assert!(joined.contains("queued prompt"), "{joined}");

        // Idle flush drains the queue → the pane disappears.
        app.session.set_streaming(false);
        app.session.flush_queued_messages(&mut app.backend);
        app.update_queue_pane();
        let joined = app.render_lines(80, 10).join("\n");
        assert!(
            !joined.contains("queued prompt"),
            "queue should drain: {joined}"
        );
    }

    #[test]
    fn render_lines_includes_open_panels() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        // A transcript entry switches the container to the capped window path
        // (the empty-transcript welcome renders uncapped).
        app.transcript.borrow_mut().push(status_entry("base", None));
        // Open the btw panel and queue a message.
        app.btw_controller.borrow_mut().open(BTW_AGENT_ID, "side q");
        app.btw_panel.borrow_mut().open("side q");
        app.session.enqueue_message("queued", None);
        app.update_queue_pane();
        let lines = app.render_lines(80, 12);
        let joined = lines.join("\n");
        assert!(joined.contains("Q: side q"), "{joined}");
        assert!(joined.contains("queued"), "{joined}");
        // The frame still fills the terminal.
        assert_eq!(lines.len(), 12, "frame must fill the terminal");
    }

    #[test]
    fn theme_switch_recolors_rendered_chrome() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.footer.borrow_mut().update(FooterState {
            model: "gpt-5".to_owned(),
            ..Default::default()
        });
        let dark = app.footer.borrow_mut().render(80).join("\n");
        assert!(
            dark.contains("224;224;224"),
            "dark Text tone #E0E0E0: {dark:?}"
        );
        app.handle_submit("/theme light");
        let light = app.footer.borrow_mut().render(80).join("\n");
        assert!(
            light.contains("26;26;26"),
            "light Text tone #1A1A1A: {light:?}"
        );
        set_palette(DARK_COLORS);
    }

    #[test]
    fn session_picker_enter_switches_and_esc_cancels() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        assert_eq!(app.session.current_session_id(), Some("session-1"));
        // Create a second session so the picker has something to switch to.
        app.handle_new();
        assert_eq!(app.session.current_session_id(), Some("session-2"));
        assert_eq!(app.known_sessions.len(), 2);

        app.handle_submit("/sessions");
        assert!(app.session_picker.borrow().is_some());

        // Up to session-1, Enter selects → switch.
        app.session_picker
            .borrow_mut()
            .as_mut()
            .unwrap()
            .handle_input("\x1b[A");
        app.session_picker
            .borrow_mut()
            .as_mut()
            .unwrap()
            .handle_input("\r");
        let action = app
            .session_picker
            .borrow_mut()
            .as_mut()
            .unwrap()
            .take_action()
            .expect("select action");
        app.handle_session_picker_action(action);
        assert_eq!(app.session.current_session_id(), Some("session-1"));
        assert!(
            app.session_picker.borrow().is_none(),
            "picker closes after select"
        );

        // Reopen, Esc cancels → editor restored, session unchanged.
        app.handle_submit("/sessions");
        assert!(app.session_picker.borrow().is_some());
        app.session_picker
            .borrow_mut()
            .as_mut()
            .unwrap()
            .handle_input("\x1b");
        let action = app
            .session_picker
            .borrow_mut()
            .as_mut()
            .unwrap()
            .take_action()
            .expect("cancel action");
        app.handle_session_picker_action(action);
        assert!(app.session_picker.borrow().is_none());
        assert_eq!(
            app.session.current_session_id(),
            Some("session-1"),
            "cancel keeps the session"
        );
    }

    #[test]
    fn session_picker_title_shows_on_rows() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/title my project");
        app.handle_submit("/sessions");
        let mut picker = app.session_picker.borrow_mut();
        let joined = dimi_tui::ansi::strip_ansi(&picker.as_mut().unwrap().render(80).join("\n"));
        assert!(joined.contains("my project"), "{joined}");
    }

    #[test]
    fn handle_submit_all_unwired_commands_report_reason() {
        set_palette(DARK_COLORS);
        for name in [
            "plugins",
            "add-dir",
            "experiments",
            "reload",
            "reload-tui",
            "editor",
            "provider",
            "secondary_model",
            "settings",
            "feedback",
            "swarm",
            "init",
            "fork",
            "export-debug-zip",
            "login",
            "logout",
            "web",
            "mcp",
        ] {
            let mut app = DimiApp::new(Config::default());
            app.handle_submit(&format!("/{name}"));
            let e = entries(&app);
            assert_eq!(e.len(), 1, "/{name} should push one status");
            assert_eq!(e[0].kind, TranscriptEntryKind::Status);
            assert!(
                e[0].content.contains("命令未接线"),
                "/{name} content: {}",
                e[0].content
            );
            assert!(
                e[0].content.contains(&format!("/{name}")),
                "/{name} should name itself: {}",
                e[0].content
            );
        }
    }

    #[test]
    fn handle_submit_bash_mode_sends_bash_line() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        // `!` at an empty prompt enters bash mode (the `!` is not buffered).
        app.editor.borrow_mut().handle_input("!");
        assert_eq!(app.editor.borrow().input_mode, InputMode::Bash);
        app.handle_submit("ls -la");
        let e = entries(&app);
        // user echo (bullet suppressed) + provider-not-configured status
        // (bash lines route through the engine turn in slice 6a).
        assert_eq!(e.len(), 2, "bash echo + status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[0].content, "ls -la");
        assert_eq!(e[0].bullet.as_deref(), Some(""));
        assert_eq!(e[1].kind, TranscriptEntryKind::Status);
        assert!(
            e[1].content.contains("未配置 provider"),
            "content: {}",
            e[1].content
        );
        // Editor returns to prompt mode and is cleared.
        assert_eq!(app.editor.borrow().input_mode, InputMode::Prompt);
        assert!(app.editor.borrow().inner().get_text().is_empty());
    }

    #[test]
    fn handle_submit_clears_editor() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("hello");
        assert!(app.editor.borrow().inner().get_text().is_empty());
        assert_eq!(app.editor.borrow().input_mode, InputMode::Prompt);
    }

    #[test]
    fn editor_on_submit_queues_into_inbox() {
        set_palette(DARK_COLORS);
        let app = DimiApp::new(Config::default());
        // Drive the editor as the TUI would: type text + Enter.
        let ed = app.editor.clone();
        let text = "typed message";
        for ch in text.chars() {
            ed.borrow_mut().handle_input(&ch.to_string());
        }
        ed.borrow_mut().handle_input("\r"); // Enter
        let queued: Vec<String> = app.submit_inbox.borrow().clone();
        assert_eq!(queued, vec![text.to_owned()]);
    }

    #[test]
    fn render_lines_returns_transcript_footer_editor_order() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.transcript
            .borrow_mut()
            .load_wire(Path::new(&wire_fixture()))
            .unwrap();
        // A tail marker guarantees transcript lines are visible in the window.
        app.transcript
            .borrow_mut()
            .push(status_entry("TAIL-MARKER", None));
        app.footer.borrow_mut().update(FooterState {
            model: "claude-sonnet-4-5".to_owned(),
            work_dir: "/tmp/dimi".to_owned(),
            context_usage: 50,
            context_tokens: 5000,
            max_context_tokens: 200000,
            ..Default::default()
        });

        let width = 80;
        let rows = 12;
        let lines = app.render_lines(width, rows);

        // Transcript fills its budget → the frame is exactly `rows` tall.
        assert_eq!(
            lines.len(),
            rows,
            "frame must fill the terminal: {} lines",
            lines.len()
        );

        let footer_height = 2;
        let editor_height = 3;
        let transcript_budget = rows - footer_height - editor_height; // 7

        // Layout order: transcript window, then footer, then editor.
        let transcript_slice = &lines[..transcript_budget];
        let footer_slice = &lines[transcript_budget..transcript_budget + footer_height];
        let editor_slice = &lines[transcript_budget + footer_height..];

        // Transcript region: carries the tail marker + wire data. Every
        // visible transcript line must come from the full wire render (subset
        // check), and the wire entries themselves loaded.
        let binding = app.transcript.borrow();
        let entries = binding.entries();
        assert_eq!(entries.len(), 7, "6 wire entries + tail marker");
        assert_eq!(entries[0].content, "Hello there!");
        let full_render = dimi_tui::wire_transcript::render_transcript(entries, width);
        for line in transcript_slice {
            assert!(
                full_render.contains(line),
                "transcript line must come from the wire render: {line:?}"
            );
        }
        let transcript_text = transcript_slice.join("\n");
        assert!(transcript_text.contains("TAIL-MARKER"), "{transcript_text}");

        // Footer region: model + cwd slots.
        let footer_text = footer_slice.join("\n");
        assert!(footer_text.contains("claude-sonnet-4-5"), "{footer_text}");
        assert!(footer_text.contains("dimi"), "{footer_text}");

        // Editor region: box borders + the `>` prompt symbol.
        let editor_text = editor_slice.join("\n");
        assert!(
            editor_text.contains('╭'),
            "editor top border: {editor_text}"
        );
        assert!(editor_text.contains('>'), "editor prompt: {editor_text}");
        assert!(
            editor_text.contains('╰'),
            "editor bottom border: {editor_text}"
        );
    }

    #[test]
    fn render_lines_small_rows_caps_transcript() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        for i in 0..50 {
            app.transcript
                .borrow_mut()
                .push(status_entry(&format!("msg {i}"), None));
        }
        let lines = app.render_lines(80, 8);
        assert_eq!(lines.len(), 8, "frame fills the terminal");
        let joined = lines.join("\n");
        assert!(joined.contains("msg 49"), "tail visible: {joined}");
    }
}

/// Slice-6a engine-dialogue tests: real `Engine::run_turn` against the
/// network-free `ScriptedLlmClient`, driving the transcript through the same
/// `handle_submit` → backend → mpsc channel → `pump_engine_turns` path the
/// live event loop uses.
#[cfg(test)]
mod engine_tests {
    use super::*;
    use dimi_engine::llm::{
        AssistantTurn, ChatRequest, LlmStreamEvent, ScriptedLlmClient, StreamedTurn,
    };
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    fn entries(app: &DimiApp) -> Vec<dimi_tui::wire_transcript::TranscriptEntry> {
        app.transcript.borrow().entries().to_vec()
    }

    fn scripted_app(events: Vec<LlmStreamEvent>) -> DimiApp {
        set_palette(DARK_COLORS);
        let config = Config {
            model: Some("test-model".to_string()),
            work_dir: Some("/tmp".to_string()),
            base_url: Some("http://localhost:1/v1".to_string()),
            api_key: Some("test-key".to_string()),
            ..Config::default()
        };
        let llm: std::sync::Arc<dyn LlmClient> =
            std::sync::Arc::new(ScriptedLlmClient::once(events));
        DimiApp::with_llm(config, llm)
    }

    fn app_with_llm(llm: std::sync::Arc<dyn LlmClient>) -> DimiApp {
        set_palette(DARK_COLORS);
        let config = Config {
            model: Some("test-model".to_string()),
            work_dir: Some("/tmp".to_string()),
            base_url: Some("http://localhost:1/v1".to_string()),
            api_key: Some("test-key".to_string()),
            ..Config::default()
        };
        DimiApp::with_llm(config, llm)
    }

    /// A scripted client that also records every request — lets a test assert
    /// what conversation actually reached the LLM (history, `/btw` prompt,
    /// steer, cross-turn tool messages) while the engine drives the scripted
    /// segments deterministically.
    struct RecordingScriptedClient {
        inner: ScriptedLlmClient,
        requests: std::sync::Mutex<Vec<ChatRequest>>,
    }

    impl RecordingScriptedClient {
        fn new(segments: Vec<Vec<LlmStreamEvent>>) -> Self {
            Self {
                inner: ScriptedLlmClient::new(segments),
                requests: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for RecordingScriptedClient {
        async fn stream_chat(
            &self,
            request: &ChatRequest,
        ) -> Result<StreamedTurn, dimi_engine::llm::LlmError> {
            self.requests.lock().unwrap().push(request.clone());
            self.inner.stream_chat(request).await
        }
    }

    /// Pump engine turns until `pred` holds or the deadline elapses (each
    /// pump is one live drain batch). Returns whether `pred` became true —
    /// tests use it to reach a mid-turn phase deterministically.
    fn pump_until(
        app: &mut DimiApp,
        deadline_ms: u64,
        mut pred: impl FnMut(&DimiApp) -> bool,
    ) -> bool {
        let step = std::time::Duration::from_millis(5);
        let mut elapsed = 0u64;
        while elapsed < deadline_ms {
            app.pump_engine_turns();
            if pred(app) {
                return true;
            }
            std::thread::sleep(step);
            elapsed += 5;
        }
        false
    }

    /// A client that records the request it saw and returns an empty turn —
    /// proves the assembled conversation (messages) reaches the LLM boundary.
    struct RecordingClient {
        requests: std::sync::Mutex<Vec<ChatRequest>>,
    }

    #[async_trait::async_trait]
    impl LlmClient for RecordingClient {
        async fn stream_chat(
            &self,
            request: &ChatRequest,
        ) -> Result<StreamedTurn, dimi_engine::llm::LlmError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(StreamedTurn {
                events: vec![LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                }],
                assistant: AssistantTurn {
                    tool_calls: vec![],
                    text: String::new(),
                    thinking: String::new(),
                },
            })
        }
    }

    #[test]
    fn handle_submit_runs_engine_turn_and_appends_assistant_text() {
        let mut app = scripted_app(vec![
            LlmStreamEvent::Text {
                delta: "Hello, ".to_string(),
            },
            LlmStreamEvent::Text {
                delta: "world!".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        app.handle_submit("hello");
        assert!(
            app.session.streaming(),
            "a turn started → the agent is busy"
        );
        app.wait_for_turn();
        let e = entries(&app);
        assert!(
            !app.session.streaming(),
            "turn ended → the agent is idle again"
        );
        assert_eq!(e.len(), 3, "user + assistant + turn-end status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[0].content, "hello");
        assert_eq!(e[1].kind, TranscriptEntryKind::Assistant);
        assert_eq!(e[1].content, "Hello, world!");
        assert_eq!(e[2].kind, TranscriptEntryKind::Status);
        assert!(e[2].content.contains("完成"), "status: {}", e[2].content);
    }

    #[test]
    fn engine_turn_carries_conversation_history_to_the_llm() {
        set_palette(DARK_COLORS);
        let recorder = std::sync::Arc::new(RecordingClient {
            requests: std::sync::Mutex::new(Vec::new()),
        });
        let config = Config {
            model: Some("test-model".to_string()),
            base_url: Some("http://localhost:1/v1".to_string()),
            api_key: Some("test-key".to_string()),
            ..Config::default()
        };
        let mut app = DimiApp::with_llm(config, recorder.clone());
        // Turn 2: dispatch "follow up" on top of the existing history.
        app.transcript.borrow_mut().push(user_entry("hi", None));
        app.transcript.borrow_mut().push(assistant_entry("yo"));
        app.handle_submit("follow up");
        app.wait_for_turn();
        let requests = recorder.requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "one turn ran");
        let request_messages = &requests[0].messages;
        assert_eq!(
            request_messages.len(),
            3,
            "history + new prompt: {request_messages:#?}"
        );
        assert_eq!(request_messages[0].content, serde_json::json!("hi"));
        assert_eq!(request_messages[1].content, serde_json::json!("yo"));
        assert_eq!(request_messages[2].role, "user");
        assert_eq!(request_messages[2].content, serde_json::json!("follow up"));
    }

    #[test]
    fn transcript_messages_skips_non_conversation_entries() {
        let entries = vec![
            user_entry("a question", None),
            assistant_entry("an answer"),
            status_entry("some status", None),
        ];
        let messages = transcript_messages(&entries);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, serde_json::json!("a question"));
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, serde_json::json!("an answer"));
    }

    #[test]
    fn handle_submit_bash_mode_runs_engine_turn() {
        let mut app = scripted_app(vec![
            LlmStreamEvent::Text {
                delta: "ran it".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        app.editor.borrow_mut().handle_input("!");
        assert_eq!(app.editor.borrow().input_mode, InputMode::Bash);
        app.handle_submit("ls -la");
        app.wait_for_turn();
        let e = entries(&app);
        assert_eq!(e.len(), 3, "bash echo + assistant + status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[0].content, "ls -la");
        assert_eq!(e[0].bullet.as_deref(), Some(""));
        assert_eq!(e[1].kind, TranscriptEntryKind::Assistant);
        assert_eq!(e[1].content, "ran it");
        // Editor returns to prompt mode and is cleared.
        assert_eq!(app.editor.borrow().input_mode, InputMode::Prompt);
        assert!(app.editor.borrow().inner().get_text().is_empty());
    }

    #[test]
    fn handle_submit_executes_bash_tool_and_attaches_result() {
        // The model asks to run `echo hi`; the engine's ToolRegistry (BashTool
        // registered by `with_llm`) executes it and the transcript shows the
        // tool card with the result attached.
        let mut app = scripted_app(vec![
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: Some("Bash".to_string()),
                arguments_part: None,
            },
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: None,
                arguments_part: Some("{\"command\":\"echo hi\"}".to_string()),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("tool_calls".to_string()),
            },
        ]);
        app.handle_submit("run something");
        app.wait_for_turn();
        let e = entries(&app);
        assert_eq!(e.len(), 3, "user + tool card + status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[1].kind, TranscriptEntryKind::ToolCall);
        let call = e[1].tool_call.as_ref().expect("tool call data");
        assert_eq!(call.name, "Bash");
        assert_eq!(
            call.args.get("command").and_then(|v| v.as_str()),
            Some("echo hi")
        );
        let result = e[1].tool_result.as_ref().expect("tool result attached");
        assert_eq!(result.output.trim(), "hi");
        assert!(!result.is_error);
        assert_eq!(e[2].kind, TranscriptEntryKind::Status);
        assert!(e[2].content.contains("完成"), "status: {}", e[2].content);
    }

    #[test]
    fn handle_submit_btw_runs_turn_and_fills_panel() {
        let mut app = scripted_app(vec![
            LlmStreamEvent::Text {
                delta: "btw answer".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        app.handle_submit("/btw hello");
        assert!(app.btw_panel.borrow().is_open());
        app.wait_for_turn();
        let joined = {
            let mut panel = app.btw_panel.borrow_mut();
            dimi_tui::ansi::strip_ansi(&panel.render(80).join("\n"))
        };
        assert!(joined.contains("Q: hello"), "{joined}");
        assert!(
            joined.contains("btw answer"),
            "answer routed to panel: {joined}"
        );
        assert_eq!(
            app.btw_panel.borrow().phase(),
            crate::btw_panel::BtwPhase::Done,
            "completed turn → done phase"
        );
    }

    #[test]
    fn activity_pane_reflects_streaming_state() {
        let mut app = scripted_app(vec![
            LlmStreamEvent::Text {
                delta: "hi".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        // Idle at start → the activity pane is hidden.
        assert_eq!(app.activity.borrow().mode(), ActivityMode::Idle);
        app.handle_submit("hello");
        // A turn started → the composing spinner is visible.
        assert!(app.session.streaming());
        assert_eq!(app.activity.borrow().mode(), ActivityMode::Composing);
        let lines = {
            let mut a = app.activity.borrow_mut();
            a.render(80)
        };
        assert_eq!(
            lines.len(),
            1,
            "activity visible while streaming: {lines:?}"
        );
        assert!(lines.join("\n").contains('🌑'), "{lines:?}");

        app.wait_for_turn();
        assert!(!app.session.streaming());
        let lines = {
            let mut a = app.activity.borrow_mut();
            a.render(80)
        };
        assert!(lines.is_empty(), "activity hidden after the turn ends");
    }

    // ── P1: cross-batch tool result (live pump, not wait_for_turn) ──────────
    #[test]
    fn live_cross_batch_tool_result_attaches_to_prior_batch_card() {
        // The tool-call card and its result are drained in SEPARATE live pump
        // batches (the tool sleeps between them). The result must attach to
        // the card created by the earlier batch — not be silently dropped.
        let client = std::sync::Arc::new(RecordingScriptedClient::new(vec![vec![
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: Some("Bash".to_string()),
                arguments_part: None,
            },
            LlmStreamEvent::ToolCall {
                tool_call_id: "call_1".to_string(),
                name: None,
                arguments_part: Some("{\"command\":\"sleep 0.3 && echo done\"}".to_string()),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("tool_calls".to_string()),
            },
        ]]));
        let mut app = app_with_llm(client.clone());
        app.handle_submit("run a slow tool");

        // First batch: pump until the tool-call card lands. The tool is still
        // sleeping, so the result is NOT in this batch.
        let card_idx = {
            let mut idx = None;
            for _ in 0..200 {
                app.pump_engine_turns();
                if let Some(i) = entries(&app)
                    .iter()
                    .position(|e| e.kind == TranscriptEntryKind::ToolCall)
                {
                    idx = Some(i);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            idx.expect("tool-call card must appear in the first batch")
        };
        assert!(
            entries(&app)[card_idx].tool_result.is_none(),
            "result must not land in the call-only batch"
        );

        // Second batch: the result arrives later and must attach to the card.
        app.wait_for_turn();
        let e = entries(&app);
        let result = e[card_idx]
            .tool_result
            .as_ref()
            .expect("result must attach across live pump batches");
        assert!(result.output.contains("done"), "output: {}", result.output);
        assert!(!result.is_error);
    }

    // ── P2: the /btw prompt reaches the LLM ─────────────────────────────────
    #[test]
    fn btw_prompt_reaches_the_llm_request() {
        let client = std::sync::Arc::new(RecordingScriptedClient::new(vec![vec![
            LlmStreamEvent::Text {
                delta: "side note".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]]));
        let mut app = app_with_llm(client.clone());
        app.handle_submit("/btw hi");
        app.wait_for_turn();
        let requests = client.requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "one btw turn ran");
        let texts: Vec<&str> = requests[0]
            .messages
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(
            texts.contains(&"hi"),
            "btw prompt must be a user message in the request: {texts:?}"
        );
    }

    // ── P3: btw answer stays in the panel, not the main transcript ──────────
    #[test]
    fn btw_answer_stays_in_the_panel_not_the_main_transcript() {
        let mut app = scripted_app(vec![
            LlmStreamEvent::Text {
                delta: "btw answer".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        app.handle_submit("/btw hello");
        app.wait_for_turn();
        let panel = {
            let mut panel = app.btw_panel.borrow_mut();
            dimi_tui::ansi::strip_ansi(&panel.render(80).join("\n"))
        };
        assert!(
            panel.contains("btw answer"),
            "answer routed to panel: {panel}"
        );
        assert!(panel.contains("Q: hello"), "prompt shown in panel: {panel}");
        let e = entries(&app);
        assert!(
            e.iter().all(|x| !x.content.contains("btw answer")),
            "btw answer must not leak into the main transcript: {e:#?}"
        );
        assert!(
            e.iter()
                .all(|x| !(x.kind == TranscriptEntryKind::User && x.content == "hello")),
            "btw prompt must not be echoed into the main transcript: {e:#?}"
        );
    }

    // ── P4: turn end flushes messages queued while streaming ────────────────
    #[test]
    fn turn_end_flushes_messages_queued_while_streaming() {
        let client = std::sync::Arc::new(RecordingScriptedClient::new(vec![vec![], vec![]]));
        let mut app = app_with_llm(client.clone());
        app.session.set_busy_input_mode(BusyInputMode::Queue);

        app.handle_submit("first");
        assert!(app.session.streaming(), "turn 1 busy");
        app.handle_submit("second"); // queued while busy (queue mode)
        assert_eq!(
            app.session.queued_messages().len(),
            1,
            "queued while streaming"
        );

        // Turn 1 ends → the idle flush must dispatch the queued prompt.
        app.wait_for_turn();
        assert!(
            app.session.queued_messages().is_empty(),
            "queue must drain when the turn ends"
        );
        assert!(app.session.streaming(), "a second turn started");

        app.wait_for_turn();
        let requests = client.requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "both turns dispatched: {requests:#?}");
        let last = requests[1].messages.last().unwrap();
        assert_eq!(last.role, "user");
        assert_eq!(last.content, serde_json::json!("second"));
    }

    // ── P5: turn 2 request carries turn 1's tool history ────────────────────
    #[test]
    fn second_turn_request_carries_the_tool_exchange_history() {
        let client = std::sync::Arc::new(RecordingScriptedClient::new(vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: None,
                },
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: None,
                    arguments_part: Some("{\"command\":\"echo hi\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![], // turn 1 step 2 after the tool runs
            vec![], // turn 2
        ]));
        let mut app = app_with_llm(client.clone());
        app.handle_submit("run tool");
        app.wait_for_turn();
        let e = entries(&app);
        assert!(
            e.iter()
                .any(|x| x.kind == TranscriptEntryKind::ToolCall && x.tool_result.is_some()),
            "turn 1 tool card with result: {e:#?}"
        );

        app.handle_submit("follow up");
        app.wait_for_turn();
        let requests = client.requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            3,
            "turn1 step1 + step2 + turn2: {requests:#?}"
        );
        let req = &requests[2];
        let assistant = req
            .messages
            .iter()
            .find(|m| m.role == "assistant" && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty()))
            .expect("assistant tool_calls message in turn 2");
        let call = &assistant.tool_calls.as_ref().unwrap()[0];
        assert_eq!(call.id, "call_1");
        assert_eq!(call.function.name, "Bash");
        assert!(
            call.function.arguments.contains("echo hi"),
            "arguments: {}",
            call.function.arguments
        );
        let tool_msg = req
            .messages
            .iter()
            .find(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("call_1"))
            .expect("tool result message in turn 2");
        assert_eq!(tool_msg.content.as_str().unwrap().trim(), "hi");
        let last = req.messages.last().unwrap();
        assert_eq!(last.role, "user");
        assert_eq!(last.content, serde_json::json!("follow up"));
    }

    // ── P6: streaming submit steers into the running turn ───────────────────
    #[test]
    fn streaming_submit_steers_into_the_running_turn() {
        let client = std::sync::Arc::new(RecordingScriptedClient::new(vec![
            // turn 1 step 1: a slow tool call leaves a window to steer
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: None,
                },
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: None,
                    arguments_part: Some("{\"command\":\"sleep 0.4\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            // turn 1 step 2: final text
            vec![
                LlmStreamEvent::Text {
                    delta: "main done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]));
        let mut app = app_with_llm(client.clone());
        app.session.set_busy_input_mode(BusyInputMode::Steer);
        app.handle_submit("main task");
        assert!(app.session.streaming());

        // Reach the tool phase while the turn is still streaming (sleeping).
        assert!(
            pump_until(&mut app, 2000, |a| {
                entries(a)
                    .iter()
                    .any(|e| e.kind == TranscriptEntryKind::ToolCall)
            }),
            "turn must reach the tool phase"
        );

        app.handle_submit("steer now");
        assert!(
            app.session.queued_messages().is_empty(),
            "steer must inject, not enqueue"
        );
        let e = entries(&app);
        assert!(
            e.iter()
                .any(|x| x.kind == TranscriptEntryKind::User && x.content == "steer now"),
            "steer echoed into the transcript: {e:#?}"
        );

        app.wait_for_turn();
        let requests = client.requests.lock().unwrap();
        assert!(requests.len() >= 2, "requests: {requests:#?}");
        let texts: Vec<&str> = requests[1]
            .messages
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(
            texts.contains(&"steer now"),
            "steer must reach the LLM request: {texts:?}"
        );
    }

    // ── P7: btw turn end does not clear concurrent user streaming ───────────
    #[test]
    fn btw_turn_end_does_not_clear_concurrent_user_streaming() {
        let client = std::sync::Arc::new(RecordingScriptedClient::new(vec![
            // user turn step 1: a slow tool call keeps it alive while btw runs
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: None,
                },
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_1".to_string(),
                    name: None,
                    arguments_part: Some("{\"command\":\"sleep 0.8\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            // btw turn: quick text answer
            vec![
                LlmStreamEvent::Text {
                    delta: "side answer".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
            // user turn step 2: final text
            vec![
                LlmStreamEvent::Text {
                    delta: "main done".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]));
        let mut app = app_with_llm(client.clone());

        app.handle_submit("main task");
        assert!(app.session.streaming());
        assert!(
            pump_until(&mut app, 3000, |a| {
                entries(a)
                    .iter()
                    .any(|e| e.kind == TranscriptEntryKind::ToolCall)
            }),
            "user turn reaches its tool phase"
        );

        // Start a btw turn while the user turn is still streaming.
        app.handle_submit("/btw side question");
        assert!(app.btw_panel.borrow().is_open());
        assert!(
            pump_until(&mut app, 3000, |a| {
                a.btw_panel.borrow().phase() == crate::btw_panel::BtwPhase::Done
            }),
            "btw turn finishes while the user turn is still sleeping"
        );
        assert!(
            app.session.streaming(),
            "btw turn end must not clear the concurrent user turn's streaming"
        );

        // The user turn eventually finishes on its own.
        assert!(
            pump_until(&mut app, 3000, |a| !a.session.streaming()),
            "user turn ends after its tool result"
        );
    }

    // ── P3/P7: btw bookkeeping is released on completion ────────────────────
    #[test]
    fn btw_completion_releases_its_turn_and_later_main_turns_skip_the_panel() {
        let client = std::sync::Arc::new(RecordingScriptedClient::new(vec![
            // btw turn
            vec![
                LlmStreamEvent::Text {
                    delta: "side answer".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
            // main turn
            vec![
                LlmStreamEvent::Text {
                    delta: "main reply".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]));
        let mut app = app_with_llm(client.clone());
        app.handle_submit("/btw side question");
        app.wait_for_turn();
        assert!(app.btw_panel.borrow().is_open());
        // Completing the btw turn releases its bookkeeping.
        assert!(app.btw_turn_id.is_none(), "btw turn id released");
        assert!(
            app.backend.btw_turn_ids.is_empty(),
            "backend btw turn ids released"
        );

        // A later main turn must not be routed into the (still-open) panel.
        app.handle_submit("main prompt");
        app.wait_for_turn();
        let panel = {
            let mut panel = app.btw_panel.borrow_mut();
            dimi_tui::ansi::strip_ansi(&panel.render(80).join("\n"))
        };
        assert!(
            panel.contains("side answer"),
            "panel keeps its answer: {panel}"
        );
        assert!(
            !panel.contains("main reply"),
            "main turn must not leak into the panel: {panel}"
        );
    }

    // ── P1: a result for an unknown call is ignored safely ──────────────────
    #[test]
    fn tool_result_for_unknown_call_is_ignored_safely() {
        let mut app = scripted_app(vec![
            LlmStreamEvent::Text {
                delta: "hi".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        app.transcript.borrow_mut().push(tool_call_entry(
            "call_1",
            "Bash",
            Some(&serde_json::json!({ "command": "ls" })),
        ));
        // A stray result for an unrelated id arrives after the turn: it must
        // be ignored (no crash, no wrong card).
        app.backend
            .tx
            .send(EngineEvent::ToolResult {
                turn_id: 999,
                tool_call_id: "stray_9".to_string(),
                output: "boom".to_string(),
                is_error: Some(true),
                synthetic: None,
            })
            .unwrap();
        app.pump_engine_turns();
        let e = entries(&app);
        let card = e
            .iter()
            .find(|x| x.kind == TranscriptEntryKind::ToolCall)
            .unwrap();
        assert!(
            card.tool_result.is_none(),
            "stray result must not attach: {e:#?}"
        );
    }

    // ── P5: a tool card without a result maps to assistant(tool_calls) only ─
    #[test]
    fn transcript_messages_maps_tool_cards_without_result_to_assistant_only() {
        let entries = vec![tool_call_entry(
            "call_1",
            "Bash",
            Some(&serde_json::json!({ "command": "ls" })),
        )];
        let messages = transcript_messages(&entries);
        assert_eq!(messages.len(), 1, "no dangling tool message: {messages:#?}");
        assert_eq!(messages[0].role, "assistant");
        let calls = messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "Bash");
        assert!(calls[0].function.arguments.contains("ls"));
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    #[test]
    fn exit_command_arms_exit_flag() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        assert!(!app.exit_flag.get());
        app.handle_submit("/exit");
        assert!(app.exit_flag.get(), "/exit should arm the exit flag");
        // No transcript entry for exit itself (the loop breaks immediately).
        assert!(app.transcript.borrow().entries().is_empty());
    }

    #[test]
    fn quit_alias_arms_exit_flag() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/quit");
        assert!(app.exit_flag.get(), "/quit alias should arm the exit flag");
    }

    #[test]
    fn ctrl_c_byte_through_editor_arms_exit_flag() {
        set_palette(DARK_COLORS);
        let app = DimiApp::new(Config::default());
        let mut ed = app.editor.borrow_mut();
        Component::handle_input(&mut *ed, "\x03");
        drop(ed);
        assert!(app.exit_flag.get(), "ctrl-c byte should arm the exit flag");
    }

    #[test]
    fn esc_byte_through_editor_arms_exit_flag() {
        set_palette(DARK_COLORS);
        let app = DimiApp::new(Config::default());
        let mut ed = app.editor.borrow_mut();
        Component::handle_input(&mut *ed, "\x1b");
        drop(ed);
        assert!(app.exit_flag.get(), "esc should arm the exit flag");
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    #[test]
    fn empty_editor_arrow_routes_to_transcript_scroll() {
        set_palette(DARK_COLORS);
        let app = DimiApp::new(Config::default());
        // Simulate the event-loop routing decision: empty editor + Up should
        // scroll the transcript, not the editor.
        let editor_empty = app.editor.borrow().inner().get_text().is_empty();
        assert!(editor_empty);
        // The transcript has no entries yet; push 3 and cap the window so
        // scrolling has room.
        app.transcript.borrow_mut().push(status_entry("a", None));
        app.transcript.borrow_mut().push(status_entry("b", None));
        app.transcript.borrow_mut().push(status_entry("c", None));
        app.transcript.borrow_mut().set_max_lines(2);
        // Offset 0 = showing the tail (3 entries, 2 visible).
        let _ = app.transcript.borrow_mut().render(80);
        assert_eq!(
            app.transcript.borrow().scroll_offset(),
            0,
            "tail window starts at 0"
        );
        // Up scrolls back one line.
        let mut t = app.transcript.borrow_mut();
        dimi_tui::component::Component::handle_input(&mut *t, "\x1b[A");
        drop(t);
        assert_eq!(app.transcript.borrow().scroll_offset(), 1);
    }
}

#[cfg(test)]
mod undo_limit_tests {
    use super::*;
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    #[test]
    fn undo_over_limit_reports_available_count() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.transcript.borrow_mut().push(user_entry("q1", None));
        app.transcript.borrow_mut().push(assistant_entry("a1"));
        app.handle_submit("/undo 5");
        let e = app.transcript.borrow().entries().to_vec();
        assert!(
            e.last()
                .unwrap()
                .content
                .contains("Cannot undo 5 prompts; only 1 can be undone"),
            "over-limit undo must match TS copy: {}",
            e.last().unwrap().content
        );
    }
}
