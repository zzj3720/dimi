//! Queue controller — host-side logic for the `QueuePaneComponent` (port of
//! the queue responsibilities in `apps/dimi/src/tui/dimi-tui.ts` —
//! `enqueueMessage` / `shiftQueuedMessage` / `recallLastQueued` /
//! `clearQueuedMessages` / `flushQueuedMessages` — and the pane-option
//! decisions in `updateQueueDisplay`).
//!
//! The controller is **pure**: it holds no live editor/TUI/session. Methods
//! that would mutate the host instead return a [`QueueAction`] the host
//! applies (e.g. putting an edited item back into the editor, dispatching a
//! flush to the session).

use crate::chrome::QueuedMessage;
use crate::session::BusyInputMode;

/// Pure action the host must apply after a [`QueueController`] method returns
/// it. The controller has no handle to the real editor/TUI/session, so it
/// reports intent rather than mutating UI state.
#[derive(Debug, Clone)]
pub enum QueueAction {
    /// ↑-recall: move the most recently queued message back to the editor as a
    /// draft (restore its mode so a `!` command runs as a shell command
    /// again — see `editor-keyboard.ts` `onUpArrowEmpty` + `recallLastQueued`).
    EditTop { message: QueuedMessage },
    /// Flush every queued message to the active session (FIFO). The host
    /// dispatches each message — bash lines as shell commands, prompts to the
    /// agent (see `flushQueuedMessages` in `dimi-tui.ts`).
    Flush { messages: Vec<QueuedMessage> },
    /// Send a normal prompt-mode message to the agent.
    SendNormal { text: String },
    /// Run a bash-mode (`!`) line as a shell command.
    SendBashLine { text: String },
}

/// Host-side queue for the queue pane — port of `state.queuedMessages` plus
/// the decisions that feed `QueuePaneOptions`.
#[derive(Debug, Clone)]
pub struct QueueController {
    messages: Vec<QueuedMessage>,
    /// `can_steer_immediately` = `!deferUserMessages` (dimi-tui.ts
    /// `updateQueueDisplay`: `canSteerImmediately: !this.deferUserMessages`).
    can_steer_immediately: bool,
    busy_input_mode: BusyInputMode,
}

impl QueueController {
    pub fn new(can_steer_immediately: bool, busy_input_mode: BusyInputMode) -> Self {
        QueueController {
            messages: Vec::new(),
            can_steer_immediately,
            busy_input_mode,
        }
    }

    pub fn messages(&self) -> &[QueuedMessage] {
        &self.messages
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// `can_steer_immediately` — feeds `QueuePaneOptions.can_steer_immediately`
    /// (whether the host can inject input mid-turn at all).
    pub fn can_steer_immediately(&self) -> bool {
        self.can_steer_immediately
    }

    pub fn set_can_steer_immediately(&mut self, value: bool) {
        self.can_steer_immediately = value;
    }

    /// `enter_steers_by_default` — feeds `QueuePaneOptions.enter_steers_by_default`
    /// (true when `busy_input_mode` is `steer`, per `updateQueueDisplay`).
    pub fn enter_steers_by_default(&self) -> bool {
        self.busy_input_mode == BusyInputMode::Steer
    }

    pub fn busy_input_mode(&self) -> BusyInputMode {
        self.busy_input_mode
    }

    pub fn set_busy_input_mode(&mut self, mode: BusyInputMode) {
        self.busy_input_mode = mode;
    }

    /// Append a queued message (port of `enqueueMessage`). `mode` is
    /// `Some("bash")` for `!` shell commands, `None` for normal prompts.
    pub fn enqueue(&mut self, text: &str, mode: Option<&'static str>) {
        self.messages.push(QueuedMessage::new(text, mode));
    }

    /// FIFO dequeue of the oldest message (port of `shiftQueuedMessage`).
    pub fn dequeue(&mut self) -> Option<QueuedMessage> {
        if self.messages.is_empty() {
            return None;
        }
        Some(self.messages.remove(0))
    }

    /// ↑-recall: pop the most recently queued message so the host can restore
    /// it as an editor draft (port of `recallLastQueued` — the pane's ↑ hint).
    /// Returns `EditTop` with the recalled message, or `None` when the queue
    /// is empty.
    pub fn edit_top(&mut self) -> Option<QueueAction> {
        let message = self.messages.pop()?;
        Some(QueueAction::EditTop { message })
    }

    /// Flush every queued message to the session (port of
    /// `flushQueuedMessages`): drains the whole queue FIFO into a single
    /// `Flush` action for the host to dispatch.
    pub fn flush(&mut self) -> QueueAction {
        QueueAction::Flush {
            messages: std::mem::take(&mut self.messages),
        }
    }

    /// Clear the queue (port of `clearQueuedMessages`).
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// FIFO dequeue of the oldest message mapped to a send action
    /// (`SendBashLine` for bash-mode, `SendNormal` otherwise). Convenience for
    /// hosts that drain the queue one message at a time.
    pub fn dequeue_action(&mut self) -> Option<QueueAction> {
        let msg = self.dequeue()?;
        if msg.mode == Some("bash") {
            Some(QueueAction::SendBashLine { text: msg.text })
        } else {
            Some(QueueAction::SendNormal { text: msg.text })
        }
    }
}

impl Default for QueueController {
    fn default() -> Self {
        // `deferUserMessages` starts false in dimi-tui.ts, so steering is
        // available by default.
        QueueController::new(true, BusyInputMode::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_dequeue_is_fifo() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        q.enqueue("first", None);
        q.enqueue("second", None);
        q.enqueue("third", None);
        assert_eq!(q.dequeue().unwrap().text, "first");
        assert_eq!(q.dequeue().unwrap().text, "second");
        assert_eq!(q.dequeue().unwrap().text, "third");
        assert!(q.dequeue().is_none());
        assert!(q.is_empty());
    }

    #[test]
    fn edit_top_pops_most_recent_and_removes_it() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        q.enqueue("first", None);
        q.enqueue("second", None);
        match q.edit_top().unwrap() {
            QueueAction::EditTop { message } => {
                assert_eq!(message.text, "second");
                assert_eq!(message.mode, None);
            }
            other => panic!("expected EditTop, got {other:?}"),
        }
        assert_eq!(q.len(), 1);
        assert_eq!(q.dequeue().unwrap().text, "first");
        assert!(q.is_empty());
    }

    #[test]
    fn edit_top_preserves_bash_mode() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        q.enqueue("ls -la", Some("bash"));
        match q.edit_top().unwrap() {
            QueueAction::EditTop { message } => assert_eq!(message.mode, Some("bash")),
            other => panic!("expected EditTop, got {other:?}"),
        }
    }

    #[test]
    fn edit_top_empty_returns_none() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        assert!(q.edit_top().is_none());
    }

    #[test]
    fn flush_drains_all_in_order() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        q.enqueue("ls", Some("bash"));
        q.enqueue("prompt", None);
        q.enqueue("pwd", Some("bash"));
        match q.flush() {
            QueueAction::Flush { messages } => {
                let texts: Vec<&str> = messages.iter().map(|m| m.text.as_str()).collect();
                assert_eq!(texts, vec!["ls", "prompt", "pwd"]);
            }
            other => panic!("expected Flush, got {other:?}"),
        }
        assert!(q.is_empty());
    }

    #[test]
    fn flush_empty_returns_empty_flush() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        match q.flush() {
            QueueAction::Flush { messages } => assert!(messages.is_empty()),
            other => panic!("expected Flush, got {other:?}"),
        }
    }

    #[test]
    fn clear_empties_queue() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        q.enqueue("a", None);
        q.enqueue("b", None);
        q.clear();
        assert!(q.is_empty());
        assert!(q.dequeue().is_none());
    }

    #[test]
    fn dequeue_action_maps_mode_to_send_variant() {
        let mut q = QueueController::new(true, BusyInputMode::Queue);
        q.enqueue("echo hi", Some("bash"));
        q.enqueue("hello", None);
        match q.dequeue_action().unwrap() {
            QueueAction::SendBashLine { text } => assert_eq!(text, "echo hi"),
            other => panic!("expected SendBashLine, got {other:?}"),
        }
        match q.dequeue_action().unwrap() {
            QueueAction::SendNormal { text } => assert_eq!(text, "hello"),
            other => panic!("expected SendNormal, got {other:?}"),
        }
    }

    #[test]
    fn pane_option_decisions() {
        let mut q = QueueController::new(false, BusyInputMode::Queue);
        assert!(!q.can_steer_immediately());
        assert!(!q.enter_steers_by_default());
        q.set_can_steer_immediately(true);
        q.set_busy_input_mode(BusyInputMode::Steer);
        assert!(q.can_steer_immediately());
        assert!(q.enter_steers_by_default());
    }
}
