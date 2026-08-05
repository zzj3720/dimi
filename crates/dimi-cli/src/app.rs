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
use std::path::Path;
use std::rc::Rc;

use dimi_engine::engine::Engine;
use dimi_engine::events::EngineEvent;
use dimi_engine::llm::{LlmClient, OpenAiCompatibleClient};
use dimi_engine::permission::{PermissionMode, PolicyConfig};
use dimi_engine::tool::{BashTool, ToolRegistry};
use dimi_engine::types::{EngineTool, EngineTurnInput, LlmMessage, ProviderConfig};
use dimi_tui::chrome::WelcomeState;
use dimi_tui::commands::{
    DispatchAction, SlashCommand, builtin_slash_commands, dispatch_input,
    find_builtin_slash_command,
};
use dimi_tui::component::{Component, SharedComponent};
use dimi_tui::components::messages::tool_renderers::ToolResultData;
use dimi_tui::custom_editor::{CustomEditor, CustomEditorCallbacks, InputMode};
use dimi_tui::editor::EditorOptions;
use dimi_tui::footer::{FooterComponent, FooterState};
use dimi_tui::keys::matches_key;
use dimi_tui::process_terminal::ProcessTerminal;
use dimi_tui::session::{BusyInputMode, SessionBackend, SessionError, SessionManager};
use dimi_tui::terminal::Terminal;
use dimi_tui::theme::ColorToken;
use dimi_tui::tui::Tui;
use dimi_tui::wire_transcript::{TranscriptEntry, TranscriptEntryKind};

use crate::config::Config;
use crate::transcript::{
    TranscriptContainer, assistant_entry, status_entry, tool_call_entry, user_entry,
};

/// Child index of the editor in the mounted TUI tree (transcript=0, footer=1,
/// editor=2).
const EDITOR_CHILD_INDEX: usize = 2;

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

/// Assemble the OpenAI-shaped conversation from the transcript entries
/// (user/assistant text only — tool/thinking/status entries are display-only
/// and skipped). Minimal context assembly: no system prompt, no compaction
/// (slice 6a leftover).
fn transcript_messages(entries: &[TranscriptEntry]) -> Vec<LlmMessage> {
    entries
        .iter()
        .filter_map(|e| match e.kind {
            TranscriptEntryKind::User => Some(LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(e.content.clone()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning: None,
            }),
            TranscriptEntryKind::Assistant => Some(LlmMessage {
                role: "assistant".to_string(),
                content: serde_json::Value::String(e.content.clone()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning: None,
            }),
            _ => None,
        })
        .collect()
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
    /// Provider config (`None` = not configured → clear error status).
    provider: Option<ProviderConfig>,
    policy: PolicyConfig,
    /// Working directory handed to tool execution.
    work_dir: Option<String>,
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
            provider,
            policy: PolicyConfig {
                mode: PermissionMode::Auto,
                rules: vec![],
                session_approved_patterns: vec![],
            },
            work_dir,
            next_turn_id: 1,
            next_session_id: 1,
        }
    }

    /// Spawn one engine turn for `text` (used by both prompt and `!` bash
    /// lines — slice 6a routes bash through the model). Returns whether a
    /// turn started (always true once the provider is configured).
    fn start_turn(&mut self, _text: &str) -> bool {
        let Some(provider) = self.provider.clone() else {
            self.transcript.borrow_mut().push(status_entry(
                ENGINE_NOT_CONFIGURED_MSG,
                Some(ColorToken::Error),
            ));
            return false;
        };

        let transcript = self.transcript.borrow();
        let messages = transcript_messages(transcript.entries());
        drop(transcript);
        let input = EngineTurnInput {
            turn_id: self.next_turn_id,
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
        self.next_turn_id += 1;

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
                .run_turn(&input, llm.as_ref(), tools.as_ref(), &policy, &mut on_event)
                .await;
        });
        self.current_turn = Some(handle);
        true
    }

    /// Drain every buffered engine event (non-blocking) into transcript
    /// entries. Returns whether anything changed (the caller re-renders).
    fn drain_events(&mut self) -> bool {
        let mut batch: Vec<EngineEvent> = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            batch.push(event);
        }
        if batch.is_empty() {
            return false;
        }
        self.apply_engine_events(&batch);
        true
    }

    /// Map a batch of engine events to transcript entries (minimal slice 6a
    /// mapping: assistant text + tool calls/results + turn-end status).
    fn apply_engine_events(&mut self, events: &[EngineEvent]) {
        let mut pending_assistant = String::new();
        let mut entries: Vec<TranscriptEntry> = Vec::new();
        let mut turn_ended = false;
        for event in events {
            match event {
                EngineEvent::AssistantDelta { delta, .. } => pending_assistant.push_str(delta),
                EngineEvent::ToolCallStarted {
                    tool_call_id,
                    name,
                    args,
                    ..
                } => {
                    flush_assistant(&mut entries, &mut pending_assistant);
                    entries.push(tool_call_entry(tool_call_id, name, args.as_ref()));
                }
                EngineEvent::ToolResult {
                    tool_call_id,
                    output,
                    is_error,
                    ..
                } => {
                    if let Some(entry) = entries
                        .iter_mut()
                        .rev()
                        .find(|e| e.tool_call.as_ref().is_some_and(|c| c.id == *tool_call_id))
                    {
                        entry.tool_result = Some(ToolResultData {
                            tool_call_id: tool_call_id.clone(),
                            output: output.clone(),
                            is_error: is_error.unwrap_or(false),
                        });
                    }
                }
                EngineEvent::TurnEnded { reason, error, .. } => {
                    flush_assistant(&mut entries, &mut pending_assistant);
                    if reason == "completed" {
                        entries.push(status_entry("✓ turn 完成", None));
                    } else {
                        let message = error
                            .as_ref()
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("engine turn failed");
                        entries.push(status_entry(
                            &format!("✗ turn {reason}: {message}"),
                            Some(ColorToken::Error),
                        ));
                    }
                    turn_ended = true;
                }
                _ => {}
            }
        }
        flush_assistant(&mut entries, &mut pending_assistant);
        if !entries.is_empty() {
            let mut transcript = self.transcript.borrow_mut();
            for entry in entries {
                transcript.push(entry);
            }
        }
        if turn_ended {
            // The session's busy flag must drop so the next idle flush can
            // dispatch new input (TS `streamingPhase` idle parity).
            self.turn_idle = true;
        }
    }
}

/// Push the accumulated assistant text as one entry (empty → no-op).
fn flush_assistant(entries: &mut Vec<TranscriptEntry>, pending: &mut String) {
    if pending.is_empty() {
        return;
    }
    entries.push(assistant_entry(pending));
    pending.clear();
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
        self.start_turn(text)
    }
    fn send_prompt(&mut self, text: &str) -> bool {
        self.start_turn(text)
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
            footer,
            editor,
            session,
            backend,
            runtime,
            config,
            submit_inbox,
            exit_flag,
        };
        app.apply_config();
        app
    }

    /// Apply the loaded config to the chrome (welcome + footer) and cold-build
    /// the transcript from `config.wire` if set.
    fn apply_config(&mut self) {
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

        let mut footer_state = FooterState::new();
        if let Some(model) = &self.config.model {
            footer_state.model = model.clone();
        }
        if let Some(work_dir) = &self.config.work_dir {
            footer_state.work_dir = work_dir.clone();
        }
        self.footer.borrow_mut().update(footer_state);

        if let Some(wire) = &self.config.wire {
            let _ = self.transcript.borrow_mut().load_wire(Path::new(wire));
        }
    }

    /// Compute the transcript's visible-line budget from the current terminal
    /// size: `rows - footer_height - editor_height`. The footer and editor are
    /// rendered first (at `width`) to learn their heights; the TUI mounts them
    /// after the transcript, so the container must cap itself to this budget.
    fn apply_layout(&mut self, width: usize, rows: usize) {
        let footer_height = self.footer.borrow_mut().render(width).len();
        let editor_height = self.editor.borrow_mut().render(width).len();
        let max_lines = rows.saturating_sub(footer_height + editor_height);
        self.transcript.borrow_mut().set_max_lines(max_lines);
    }

    /// Pure frame render for headless tests: transcript (capped to the rows
    /// left after footer + editor), then footer, then editor. Returns exactly
    /// `rows` lines when the transcript fills the budget it was given.
    ///
    /// `#[allow(dead_code)]`: this is the headless-test render surface of a
    /// binary crate — the live app renders through `Tui::do_render` instead,
    /// so `cargo build` (without test cfg) would otherwise flag it unused.
    #[allow(dead_code)]
    pub fn render_lines(&mut self, width: usize, rows: usize) -> Vec<String> {
        let footer_lines = self.footer.borrow_mut().render(width);
        let editor_lines = self.editor.borrow_mut().render(width);
        let chrome_lines = footer_lines.len() + editor_lines.len();
        let max_lines = rows.saturating_sub(chrome_lines);
        self.transcript.borrow_mut().set_max_lines(max_lines);
        let transcript_lines = self.transcript.borrow_mut().render(width);
        let mut out = transcript_lines;
        out.extend(footer_lines);
        out.extend(editor_lines);
        out
    }

    /// Drain buffered engine events into the transcript (non-blocking; called
    /// every event-loop iteration and after submits). Returns whether anything
    /// changed so the caller can skip redundant re-renders.
    pub fn pump_engine_turns(&mut self) -> bool {
        let mut changed = false;
        if self.backend.drain_events() {
            changed = true;
        }
        if self.backend.turn_idle {
            self.backend.turn_idle = false;
            self.session.set_streaming(false);
            changed = true;
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
        tui.add_child(Box::new(SharedComponent::new(self.footer.clone())));
        tui.add_child(Box::new(SharedComponent::new(self.editor.clone())));

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
                    // buffer splitting) into the TUI input handler.
                    let mut on_input = |data: &str| {
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
                    // turn made progress (streaming assistant output).
                    if progress {
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
    }

    /// `!` shell-mode submit: enqueue as a bash line, echo it into the
    /// transcript (bullet suppressed, like the wire `shell_command` echo), and
    /// flush the queue through the backend stub.
    fn handle_bash_line(&mut self, text: &str) {
        self.session.enqueue_message(text, Some("bash"));
        self.push_user(text, Some(String::new()));
        self.session.flush_queued_messages(&mut self.backend);
    }

    /// Apply a [`DispatchAction`] from slash-command resolution.
    fn dispatch_action(&mut self, action: DispatchAction) {
        match action {
            DispatchAction::SendNormal(msg) => {
                self.session.enqueue_message(&msg, None);
                self.push_user(&msg, None);
                self.session.flush_queued_messages(&mut self.backend);
            }
            DispatchAction::RunBuiltin { name, args } => match name.as_str() {
                "help" => self.handle_help(),
                // `/clear` resolves to the registry `/new` via alias.
                "new" => self.handle_clear(),
                "wire" => self.handle_wire(args.trim()),
                // `/exit` / `/quit` leave the TUI (restores the terminal via
                // the event loop's `tui.stop()` on the exit flag).
                "exit" | "quit" => self.exit_flag.set(true),
                other if find_builtin_slash_command(other).is_some() => {
                    self.push_status(
                        &format!("/{other} 未实现（本切片仅实现 /help /new /wire /exit）"),
                        None,
                    );
                }
                other => {
                    self.push_status(&format!("未知命令：/{other}"), Some(ColorToken::Error));
                }
            },
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

    /// `/help` — append a status row listing the implemented commands and the
    /// full builtin registry.
    fn handle_help(&mut self) {
        let names: Vec<&str> = builtin_slash_commands()
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        let help = format!(
            "Dimi TUI（Rust，slice 6）· 已实现：/help /new(/clear) /wire <path>\n内置命令：{}",
            names.join(" ")
        );
        self.push_status(&help, None);
    }

    /// `/new` / `/clear` — clear the transcript.
    fn handle_clear(&mut self) {
        self.transcript.borrow_mut().clear();
    }

    /// `/wire <path>` — cold-rebuild the transcript from a wire.jsonl file.
    fn handle_wire(&mut self, path: &str) {
        let result = self.transcript.borrow_mut().load_wire(Path::new(path));
        if let Err(e) = result {
            self.push_status(&format!("wire 加载失败：{e}"), Some(ColorToken::Error));
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
    use dimi_tui::theme::{DARK_COLORS, set_palette};
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
    fn handle_submit_unimplemented_builtin_appends_not_implemented() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("/theme");
        let e = entries(&app);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, TranscriptEntryKind::Status);
        assert!(e[0].content.contains("未实现"), "content: {}", e[0].content);
        assert!(e[0].content.contains("/theme"));
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
