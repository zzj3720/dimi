//! Session lifecycle state machine — port of the session-management *logic*
//! in `apps/dimi/src/tui/dimi-tui.ts` (`init`/`createSession` /
//! `resumeSession` / `switchToSession`, `enqueueMessage`,
//! `flushQueuedMessages`, and the `sendMessage` busy-input gate).
//!
//! This is a **pure** state machine: it holds no live SDK/engine reference and
//! performs no I/O. Every engine interaction goes through the injectable
//! [`SessionBackend`] trait, so the transitions are unit-testable with a mock.
//! The real backend (wiring a `dimi` SDK `Session` / `Harness`) is host-side
//! and intentionally not part of this module.

use crate::chrome::QueuedMessage;

/// How Enter behaves while the agent is busy — mirrors `busyInputMode`
/// (`'steer' | 'queue'`) and `DEFAULT_BUSY_INPUT_MODE` in
/// `apps/dimi/src/tui/config.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BusyInputMode {
    /// Enter during a busy turn injects the input immediately (`steer`).
    #[default]
    Steer,
    /// Enter during a busy turn queues the input (`queue`).
    Queue,
}

/// A session-engine operation failed (create/resume/switch). Carries a
/// human-readable message, matching the TS error paths that call
/// `showError(...)` on failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionError(pub String);

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SessionError {}

/// Injectable engine behind the session state machine.
///
/// `create_session` / `resume_session` / `switch_session` resolve a session and
/// return its id (or an error). `send_bash_line` / `send_prompt` dispatch one
/// queued message and report whether the agent became busy as a result
/// (`true` = a shell command / agent turn started). The state machine uses the
/// busy report to decide whether more queued prompts can go out in the same
/// idle cycle.
///
/// # TODO(legacy)
/// The real implementation wires a `dimi` SDK `Session` / `Harness`
/// (`apps/dimi/src/tui/dimi-tui.ts`: `this.harness.createSession(...)`,
/// `this.harness.resumeSession(...)`, `session.prompt(...)`,
/// `runShellCommandFromInput(...)`). Those are host-side SDK/engine calls and
/// are intentionally not ported here — the state machine only talks to this
/// trait.
pub trait SessionBackend {
    /// Create a fresh session. Returns the new session id.
    fn create_session(&mut self) -> Result<String, SessionError>;
    /// Resume an existing session by id. Returns the session id.
    fn resume_session(&mut self, session_id: &str) -> Result<String, SessionError>;
    /// Switch to an existing session by id. Returns the session id.
    fn switch_session(&mut self, session_id: &str) -> Result<String, SessionError>;
    /// Dispatch one bash-mode (`!`) queued line. Returns whether the agent
    /// became busy (a shell command started running).
    fn send_bash_line(&mut self, text: &str) -> bool;
    /// Dispatch one prompt-mode queued message. Returns whether a turn started
    /// (the agent became busy).
    fn send_prompt(&mut self, text: &str) -> bool;
}

/// Session-management state machine.
///
/// Models the TS lifecycle logic that does not depend on the live SDK:
///
/// - `current_session_id`: the active session (or `None` before any session).
/// - `streaming` / `compacting`: whether the agent is busy (streaming a turn /
///   running a shell command) and whether a compaction is in progress. Both
///   gate flushing.
/// - `queued_messages`: user input that arrived while the agent was busy.
///   Bash-mode (`!`) messages flush as a **single batch** of bash lines;
///   prompt-mode messages flush **one at a time** — only the first is sent
///   when the agent becomes idle, the rest wait for the next idle cycle.
/// - `busy_input_mode`: `'steer'` (Enter injects mid-turn) vs `'queue'` (Enter
///   enqueues while busy).
#[derive(Debug, Clone)]
pub struct SessionManager {
    current_session_id: Option<String>,
    streaming: bool,
    compacting: bool,
    queued_messages: Vec<QueuedMessage>,
    busy_input_mode: BusyInputMode,
}

impl SessionManager {
    pub fn new(busy_input_mode: BusyInputMode) -> Self {
        SessionManager {
            current_session_id: None,
            streaming: false,
            compacting: false,
            queued_messages: Vec::new(),
            busy_input_mode,
        }
    }

    pub fn current_session_id(&self) -> Option<&str> {
        self.current_session_id.as_deref()
    }

    pub fn streaming(&self) -> bool {
        self.streaming
    }

    pub fn compacting(&self) -> bool {
        self.compacting
    }

    pub fn busy_input_mode(&self) -> BusyInputMode {
        self.busy_input_mode
    }

    pub fn queued_messages(&self) -> &[QueuedMessage] {
        &self.queued_messages
    }

    pub fn set_busy_input_mode(&mut self, mode: BusyInputMode) {
        self.busy_input_mode = mode;
    }

    /// Reflect the agent's streaming state (a turn is composing/streaming or a
    /// shell command is running). Mirrors `streamingPhase !== 'idle'`.
    pub fn set_streaming(&mut self, streaming: bool) {
        self.streaming = streaming;
    }

    /// Reflect whether a compaction is in progress (`isCompacting`).
    pub fn set_compacting(&mut self, compacting: bool) {
        self.compacting = compacting;
    }

    /// Append a queued message (port of `enqueueMessage`). `mode` is
    /// `Some("bash")` for `!` shell commands, `None`/`Some("prompt")` for
    /// normal prompts. Enqueueing itself never depends on idle/busy — the
    /// idle/busy decision happens at flush time.
    pub fn enqueue_message(&mut self, text: &str, mode: Option<&'static str>) {
        self.queued_messages.push(QueuedMessage::new(text, mode));
    }

    /// Create a fresh session (port of `createSessionFromCurrentState` +
    /// `setSession` in `createNewSession`). On success the runtime is reset:
    /// the queue is cleared and streaming/compacting return to idle.
    pub fn create_session(&mut self, backend: &mut dyn SessionBackend) -> Result<(), SessionError> {
        let id = backend.create_session()?;
        self.reset_runtime();
        self.current_session_id = Some(id);
        Ok(())
    }

    /// Resume an existing session (port of `resumeSession`). On success the
    /// runtime is reset exactly like a new session (`switchToSession` calls
    /// `resetSessionRuntime` before switching).
    pub fn resume_session(
        &mut self,
        backend: &mut dyn SessionBackend,
        session_id: &str,
    ) -> Result<(), SessionError> {
        let id = backend.resume_session(session_id)?;
        self.reset_runtime();
        self.current_session_id = Some(id);
        Ok(())
    }

    /// Switch to an existing session (port of `switchToSession` /
    /// `resumeSession` for an already-active session). On success the runtime
    /// is reset (`resetSessionRuntime` clears `state.queuedMessages`).
    pub fn switch_session(
        &mut self,
        backend: &mut dyn SessionBackend,
        session_id: &str,
    ) -> Result<(), SessionError> {
        let id = backend.switch_session(session_id)?;
        self.reset_runtime();
        self.current_session_id = Some(id);
        Ok(())
    }

    /// Clear the queue and return to an idle, non-compacting runtime — port of
    /// the queue-relevant parts of `resetSessionRuntime` in `dimi-tui.ts`.
    fn reset_runtime(&mut self) {
        self.queued_messages.clear();
        self.streaming = false;
        self.compacting = false;
    }

    /// Dispatch queued messages to the backend (port of `flushQueuedMessages`,
    /// driven by the replay renderer / shell completion / compaction
    /// completion). No-op unless the agent is idle, not compacting, and a
    /// session is active. The policy (mirrors the TS `flushQueuedMessages`
    /// while-loop — it drains the **entire** queue in one pass):
    ///
    /// 1. **Drain bash-queued messages all at once** — every `!` line in the
    ///    queue (in order) is dispatched through `SessionBackend::send_bash_line`.
    /// 2. **Send every remaining prompt message** through
    ///    `SessionBackend::send_prompt` (the TS flush is invoked when the
    ///    agent is already idle, so it does not stop after the first prompt).
    ///    If a backend reports busy, `streaming` is set so the host does not
    ///    re-enter flush until the next idle cycle.
    pub fn flush_queued_messages(&mut self, backend: &mut dyn SessionBackend) {
        if self.streaming || self.compacting || self.current_session_id.is_none() {
            return;
        }

        // Strict FIFO drain, mirroring the TS `flushQueuedMessages` while-loop
        // (`shiftQueuedMessage` one at a time): bash lines go through
        // `send_bash_line`, prompts through `send_prompt`, preserving the
        // original queue order (a bash/prompt interleave stays interleaved).
        while let Some(msg) = self.queued_messages.first().cloned() {
            self.queued_messages.remove(0);
            if msg.mode == Some("bash") {
                backend.send_bash_line(&msg.text);
            } else {
                let busy = backend.send_prompt(&msg.text);
                if busy {
                    self.streaming = true;
                }
            }
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        SessionManager::new(BusyInputMode::default())
    }
}

/// Outcome of an Enter submitted while the agent may be busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyInputIntent {
    /// Agent is idle — send the input directly.
    Send,
    /// Agent is busy and `busy_input_mode` is `steer` — inject immediately.
    Steer,
    /// Agent is busy (or cannot be steered) — enqueue for later dispatch.
    Queue,
}

/// Decide what Enter during a busy turn should do — the gate in
/// `sendMessage` / `steerMessage` in `apps/dimi/src/tui/dimi-tui.ts`:
///
/// ```ts
/// if (this.deferUserMessages || this.state.appState.isCompacting) → queue
/// else if (this.state.appState.streamingPhase === 'idle')          → send
/// else if (this.state.appState.busyInputMode === 'steer')          → steer
/// else                                                              → queue
/// ```
///
/// `can_steer_immediately` is `!deferUserMessages` (see `updateQueueDisplay`:
/// `canSteerImmediately: !this.deferUserMessages`), so a `false` value forces
/// queueing exactly like `deferUserMessages === true`.
pub fn resolve_busy_input_intent(
    is_idle: bool,
    is_compacting: bool,
    can_steer_immediately: bool,
    busy_input_mode: BusyInputMode,
) -> BusyInputIntent {
    if is_compacting || !can_steer_immediately {
        return BusyInputIntent::Queue;
    }
    if is_idle {
        return BusyInputIntent::Send;
    }
    match busy_input_mode {
        BusyInputMode::Steer => BusyInputIntent::Steer,
        BusyInputMode::Queue => BusyInputIntent::Queue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test backend recording every dispatch. `send_prompt` reports busy
    /// (`prompt_busy`) so tests can exercise the "stop when busy" branch.
    /// Defaults to `prompt_busy: true` — sending a prompt starts a turn in
    /// the real system.
    struct MockBackend {
        create_calls: usize,
        resume_calls: Vec<String>,
        switch_calls: Vec<String>,
        bash_lines: Vec<String>,
        prompts: Vec<String>,
        dispatch_log: Vec<String>,
        prompt_busy: bool,
        fail_create: bool,
        next_id: usize,
    }

    impl Default for MockBackend {
        fn default() -> Self {
            MockBackend {
                create_calls: 0,
                resume_calls: Vec::new(),
                switch_calls: Vec::new(),
                bash_lines: Vec::new(),
                prompts: Vec::new(),
                dispatch_log: Vec::new(),
                prompt_busy: true,
                fail_create: false,
                next_id: 0,
            }
        }
    }

    impl SessionBackend for MockBackend {
        fn create_session(&mut self) -> Result<String, SessionError> {
            self.create_calls += 1;
            if self.fail_create {
                return Err(SessionError("create failed".to_owned()));
            }
            self.next_id += 1;
            Ok(format!("s{}", self.next_id))
        }

        fn resume_session(&mut self, session_id: &str) -> Result<String, SessionError> {
            self.resume_calls.push(session_id.to_owned());
            Ok(session_id.to_owned())
        }

        fn switch_session(&mut self, session_id: &str) -> Result<String, SessionError> {
            self.switch_calls.push(session_id.to_owned());
            Ok(session_id.to_owned())
        }

        fn send_bash_line(&mut self, text: &str) -> bool {
            self.bash_lines.push(text.to_owned());
            self.dispatch_log.push(format!("bash:{text}"));
            false
        }

        fn send_prompt(&mut self, text: &str) -> bool {
            self.prompts.push(text.to_owned());
            self.dispatch_log.push(format!("prompt:{text}"));
            self.prompt_busy
        }
    }

    #[test]
    fn enqueue_works_idle_or_busy_flush_gated_on_idle() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend::default();
        m.create_session(&mut b).unwrap();

        // Busy: enqueue appends but flush is a no-op.
        m.set_streaming(true);
        m.enqueue_message("one", None);
        m.enqueue_message("two", None);
        assert_eq!(m.queued_messages().len(), 2);
        m.flush_queued_messages(&mut b);
        assert!(b.prompts.is_empty());
        assert_eq!(m.queued_messages().len(), 2);

        // Idle: flush dispatches every prompt and marks the agent busy.
        m.set_streaming(false);
        m.flush_queued_messages(&mut b);
        assert_eq!(b.prompts, vec!["one", "two"]);
        assert!(m.streaming());
        assert!(m.queued_messages().is_empty());
    }

    #[test]
    fn bash_queue_flushes_as_one_batch() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend::default();
        m.create_session(&mut b).unwrap();
        m.enqueue_message("ls", Some("bash"));
        m.enqueue_message("echo hi", Some("bash"));
        m.enqueue_message("pwd", Some("bash"));
        m.flush_queued_messages(&mut b);
        assert_eq!(b.bash_lines, vec!["ls", "echo hi", "pwd"]);
        assert!(b.prompts.is_empty());
        assert!(m.queued_messages().is_empty());
        // Bash dispatch alone does not flip the machine to busy — the host
        // reflects the shell phase via set_streaming.
        assert!(!m.streaming());
    }

    #[test]
    fn prompt_queue_flushes_all_in_one_pass() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend::default();
        m.create_session(&mut b).unwrap();
        m.enqueue_message("p1", None);
        m.enqueue_message("p2", None);
        m.enqueue_message("p3", None);

        m.flush_queued_messages(&mut b); // idle → all prompts (TS while-loop)
        assert_eq!(b.prompts, vec!["p1", "p2", "p3"]);
        assert!(m.streaming());
        assert!(m.queued_messages().is_empty());
    }

    #[test]
    fn flush_preserves_fifo_order() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend::default();
        m.create_session(&mut b).unwrap();
        m.enqueue_message("prompt-a", None);
        m.enqueue_message("ls", Some("bash"));
        m.enqueue_message("prompt-b", None);
        m.flush_queued_messages(&mut b);
        // Strict FIFO: the interleaved queue order is preserved (TS
        // `flushQueuedMessages` shiftQueuedMessage one at a time).
        assert_eq!(
            b.dispatch_log,
            vec!["prompt:prompt-a", "bash:ls", "prompt:prompt-b"]
        );
        assert!(m.queued_messages().is_empty());
        assert!(m.streaming());
    }

    #[test]
    fn switch_and_create_session_reset_the_queue() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend::default();
        m.create_session(&mut b).unwrap();
        assert_eq!(m.current_session_id(), Some("s1"));

        m.enqueue_message("queued", None);
        m.set_streaming(true);
        m.set_compacting(true);
        m.switch_session(&mut b, "target-9").unwrap();
        assert_eq!(m.current_session_id(), Some("target-9"));
        assert!(m.queued_messages().is_empty());
        assert!(!m.streaming());
        assert!(!m.compacting());

        m.enqueue_message("queued", None);
        m.create_session(&mut b).unwrap();
        assert_eq!(m.current_session_id(), Some("s2"));
        assert!(m.queued_messages().is_empty());
    }

    #[test]
    fn resume_session_sets_current_session() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend::default();
        m.enqueue_message("queued", None);
        m.resume_session(&mut b, "resumed-1").unwrap();
        assert_eq!(b.resume_calls, vec!["resumed-1"]);
        assert_eq!(m.current_session_id(), Some("resumed-1"));
        assert!(m.queued_messages().is_empty());
    }

    #[test]
    fn create_session_error_keeps_state() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend {
            fail_create: true,
            ..Default::default()
        };
        m.enqueue_message("keep", None);
        let err = m.create_session(&mut b).unwrap_err();
        assert_eq!(err.0, "create failed");
        assert_eq!(m.current_session_id(), None);
        // Queue untouched when the backend operation fails.
        assert_eq!(m.queued_messages().len(), 1);
    }

    #[test]
    fn busy_input_intent_decision() {
        use BusyInputIntent::{Queue, Send, Steer};

        // Idle → send regardless of busy_input_mode.
        assert_eq!(
            resolve_busy_input_intent(true, false, true, BusyInputMode::Steer),
            Send
        );
        assert_eq!(
            resolve_busy_input_intent(true, false, true, BusyInputMode::Queue),
            Send
        );

        // Busy → steer when mode is steer, queue when mode is queue.
        assert_eq!(
            resolve_busy_input_intent(false, false, true, BusyInputMode::Steer),
            Steer
        );
        assert_eq!(
            resolve_busy_input_intent(false, false, true, BusyInputMode::Queue),
            Queue
        );

        // Compacting always queues, even when steering would otherwise apply.
        assert_eq!(
            resolve_busy_input_intent(false, true, true, BusyInputMode::Steer),
            Queue
        );

        // Cannot steer immediately (deferUserMessages) always queues.
        assert_eq!(
            resolve_busy_input_intent(false, false, false, BusyInputMode::Steer),
            Queue
        );
        assert_eq!(
            resolve_busy_input_intent(true, false, false, BusyInputMode::Steer),
            Queue
        );
    }

    #[test]
    fn flush_gated_by_streaming_compacting_and_no_session() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend::default();

        // No session yet: flush is a no-op.
        m.enqueue_message("p1", None);
        m.flush_queued_messages(&mut b);
        assert!(b.prompts.is_empty());
        assert_eq!(m.queued_messages().len(), 1);

        m.create_session(&mut b).unwrap();

        // Streaming gates flush.
        m.enqueue_message("p1", None);
        m.set_streaming(true);
        m.flush_queued_messages(&mut b);
        assert!(b.prompts.is_empty());
        assert_eq!(m.queued_messages().len(), 1);

        // Compacting gates flush.
        m.set_streaming(false);
        m.set_compacting(true);
        m.flush_queued_messages(&mut b);
        assert!(b.prompts.is_empty());
        assert_eq!(m.queued_messages().len(), 1);

        // Idle + not compacting → flush dispatches.
        m.set_compacting(false);
        m.flush_queued_messages(&mut b);
        assert_eq!(b.prompts, vec!["p1"]);
    }

    #[test]
    fn flush_reflects_backend_busy_report() {
        let mut m = SessionManager::new(BusyInputMode::Queue);
        let mut b = MockBackend {
            prompt_busy: false,
            ..Default::default()
        };
        m.create_session(&mut b).unwrap();
        m.enqueue_message("p1", None);
        m.flush_queued_messages(&mut b);
        assert_eq!(b.prompts, vec!["p1"]);
        // Backend reported no turn started → machine is not marked busy.
        assert!(!m.streaming());
    }
}
