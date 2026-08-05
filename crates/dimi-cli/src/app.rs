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

use dimi_tui::chrome::WelcomeState;
use dimi_tui::commands::{
    DispatchAction, SlashCommand, builtin_slash_commands, dispatch_input,
    find_builtin_slash_command,
};
use dimi_tui::component::{Component, SharedComponent};
use dimi_tui::custom_editor::{CustomEditor, CustomEditorCallbacks, InputMode};
use dimi_tui::editor::EditorOptions;
use dimi_tui::footer::{FooterComponent, FooterState};
use dimi_tui::keys::matches_key;
use dimi_tui::process_terminal::ProcessTerminal;
use dimi_tui::session::{BusyInputMode, SessionBackend, SessionError, SessionManager};
use dimi_tui::terminal::Terminal;
use dimi_tui::theme::ColorToken;
use dimi_tui::tui::Tui;

use crate::config::Config;
use crate::transcript::{TranscriptContainer, status_entry, user_entry};

/// Child index of the editor in the mounted TUI tree (transcript=0, footer=1,
/// editor=2).
const EDITOR_CHILD_INDEX: usize = 2;

/// Status shown whenever a normal prompt would reach the engine — the engine
/// is not wired in this slice (dimi-engine direct connection is slice 6+).
pub const ENGINE_NOT_WIRED_MSG: &str = "引擎未接线（dimi-engine 直连为后续切片）";

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

/// The engine stub behind [`SessionManager`]: every dispatch echoes a status
/// row back into the transcript and reports the agent as *not* busy, so the
/// state machine stays idle between inputs.
pub struct EchoBackend {
    transcript: Rc<RefCell<TranscriptContainer>>,
}

impl EchoBackend {
    pub fn new(transcript: Rc<RefCell<TranscriptContainer>>) -> Self {
        EchoBackend { transcript }
    }
}

impl SessionBackend for EchoBackend {
    fn create_session(&mut self) -> Result<String, SessionError> {
        Ok("echo-session".to_owned())
    }
    fn resume_session(&mut self, session_id: &str) -> Result<String, SessionError> {
        Ok(session_id.to_owned())
    }
    fn switch_session(&mut self, session_id: &str) -> Result<String, SessionError> {
        Ok(session_id.to_owned())
    }
    fn send_bash_line(&mut self, text: &str) -> bool {
        self.transcript
            .borrow_mut()
            .push(status_entry(&format!("$ {text}（引擎未接线）"), None));
        false
    }
    fn send_prompt(&mut self, _text: &str) -> bool {
        self.transcript
            .borrow_mut()
            .push(status_entry(ENGINE_NOT_WIRED_MSG, None));
        false
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
    backend: EchoBackend,
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

        let mut backend = EchoBackend::new(transcript.clone());
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

        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 4096];
        loop {
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
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
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
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
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
    fn handle_submit_normal_message_appends_user_and_status() {
        set_palette(DARK_COLORS);
        let mut app = DimiApp::new(Config::default());
        app.handle_submit("hello");
        let e = entries(&app);
        assert_eq!(e.len(), 2, "user + engine-not-wired status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[0].content, "hello");
        assert_eq!(e[1].kind, TranscriptEntryKind::Status);
        assert!(
            e[1].content.contains("引擎未接线"),
            "content: {}",
            e[1].content
        );
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
        // user echo (bullet suppressed) + bash status.
        assert_eq!(e.len(), 2, "bash echo + status: {e:#?}");
        assert_eq!(e[0].kind, TranscriptEntryKind::User);
        assert_eq!(e[0].content, "ls -la");
        assert_eq!(e[0].bullet.as_deref(), Some(""));
        assert_eq!(e[1].kind, TranscriptEntryKind::Status);
        assert!(e[1].content.contains("ls -la"));
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
