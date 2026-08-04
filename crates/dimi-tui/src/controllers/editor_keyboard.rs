//! Editor keyboard controller — the pure parts of
//! `apps/dimi/src/tui/controllers/editor-keyboard.ts`.
//!
//! Ports the pending-exit / pending-undo-esc state machines, the Ctrl+C /
//! Ctrl+D / Esc / Shift-Tab / Ctrl+S / up-arrow decision trees, and the
//! Ctrl+S steer-item projection. The `CustomEditor` wiring, the external
//! editor subprocess, clipboard image reading/compression, and the
//! `session.cancel()` / `cancelCompaction()` async calls are
//! `// TODO(legacy)`.

use crate::controllers::{MessageMode, QueuedMessage, StreamingPhase};

/// `CTRL_C_HINT`.
pub const CTRL_C_HINT: &str = "Press Ctrl+C again to exit";
/// `CTRL_D_HINT`.
pub const CTRL_D_HINT: &str = "Press Ctrl+D again to exit";
/// `EXIT_CONFIRM_WINDOW_MS`.
pub const EXIT_CONFIRM_WINDOW_MS: u64 = 1500;
/// `DOUBLE_ESC_WINDOW_MS`.
pub const DOUBLE_ESC_WINDOW_MS: u64 = 600;
/// `LLM_NOT_SET_MESSAGE`.
pub const LLM_NOT_SET_MESSAGE: &str = "LLM not set, send \"/login\" to login";
/// `NO_ACTIVE_SESSION_MESSAGE`.
pub const NO_ACTIVE_SESSION_MESSAGE: &str = "No active session. Send /login to login.";

/// The pending double-press exit kind (`PendingExit.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    CtrlC,
    CtrlD,
}

/// One unit of Ctrl-S steer input (`SteerInputItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteerInputItem {
    pub text: String,
}

/// A snapshot of everything the keyboard decision trees read.
#[derive(Debug, Clone)]
pub struct EditorContext {
    /// `host.cancelInFlight !== undefined`.
    pub cancel_in_flight: bool,
    /// `btwPanelController` active + running.
    pub btw_running: bool,
    /// `btwPanelController` active.
    pub btw_active: bool,
    pub is_compacting: bool,
    pub streaming_phase: StreamingPhase,
    pub has_text: bool,
    pub has_session: bool,
    pub plan_mode: bool,
    /// `state.activeDialog` (e.g. `"session-picker"`).
    pub active_dialog: Option<String>,
    pub todo_panel_has_overflow: bool,
    pub input_mode: MessageMode,
    /// `state.appState.model.trim().length > 0`.
    pub model_set: bool,
    pub queue: Vec<QueuedMessage>,
    pub last_queued: Option<QueuedMessage>,
}

impl EditorContext {
    pub fn queue_is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn has_steerable_queued(&self) -> bool {
        self.queue.iter().any(|m| m.mode != MessageMode::Bash)
    }
}

/// The outcome of a keyboard decision — the host maps these onto its own
/// callbacks (`stop()`, `session.cancel()`, `openUndoSelector()`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// Host `cancelInFlight` was consumed.
    CancelInFlight,
    /// `btw.cancelRunning()` succeeded.
    BtwCancelRunning,
    /// `btw.closeOrCancel()` succeeded.
    BtwCloseOrCancel,
    /// Editor text was cleared (returns early from the handler).
    ClearEditorText,
    /// Cancel the in-flight compaction (`session.cancelCompaction`).
    CancelCompaction,
    /// Cancel the in-flight stream (`session.cancel` + shell cancel).
    CancelStream,
    /// First Ctrl-C / Ctrl-D press arms a pending exit (hint shown); the
    /// second press emits [`EditorAction::Exit`].
    ArmPendingExit(ExitKind),
    /// Second press → exit (`host.stop()`).
    Exit,
    /// Hide the session picker dialog.
    HideSessionPicker,
    /// Toggle plan mode.
    TogglePlanMode { enabled: bool },
    /// Open the undo selector (double-Esc).
    OpenUndoSelector,
    /// No active session error.
    NoSession,
    /// LLM not set error (Ctrl-S steer).
    LlmNotSet,
    /// Toggle tool output expansion.
    ToggleToolOutputExpansion,
    /// Toggle todo panel expansion (todo panel has overflow).
    ToggleTodoExpansion,
    /// Recall the last queued message into the editor.
    RecallLastQueued(QueuedMessage),
    /// None of the above (no state change).
    None,
}

/// The editor keyboard controller (port of `EditorKeyboardController`).
#[derive(Debug, Default)]
pub struct EditorKeyboardController {
    pending_exit: Option<ExitKind>,
    pending_undo_esc: bool,
}

impl EditorKeyboardController {
    pub fn new() -> Self {
        EditorKeyboardController::default()
    }

    pub fn pending_exit(&self) -> Option<ExitKind> {
        self.pending_exit
    }

    pub fn pending_undo_esc(&self) -> bool {
        self.pending_undo_esc
    }

    /// `onCtrlC` — the full decision tree.
    pub fn handle_ctrl_c(&mut self, ctx: &EditorContext) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        if ctx.cancel_in_flight {
            self.clear_pending_exit();
            actions.push(EditorAction::CancelInFlight);
            return actions;
        }
        // The btw panel stacks above the transcript, so Ctrl+C cancels/closes
        // it before touching an in-flight compaction or stream.
        if ctx.btw_running {
            self.clear_pending_exit();
            actions.push(EditorAction::BtwCancelRunning);
            return actions;
        }
        if ctx.btw_active {
            self.clear_pending_exit();
            actions.push(EditorAction::BtwCloseOrCancel);
            return actions;
        }
        if ctx.is_compacting {
            self.clear_pending_exit();
            if ctx.has_text {
                actions.push(EditorAction::ClearEditorText);
                return actions;
            }
            actions.push(EditorAction::CancelCompaction);
            return actions;
        }
        if !ctx.streaming_phase.is_idle() {
            self.clear_pending_exit();
            if ctx.has_text {
                actions.push(EditorAction::ClearEditorText);
                return actions;
            }
            actions.push(EditorAction::CancelStream);
            return actions;
        }
        if self.pending_exit == Some(ExitKind::CtrlC) {
            self.clear_pending_exit();
            actions.push(EditorAction::Exit);
            return actions;
        }
        if ctx.has_text {
            actions.push(EditorAction::ClearEditorText);
        }
        self.arm_pending_exit(ExitKind::CtrlC);
        actions.push(EditorAction::ArmPendingExit(ExitKind::CtrlC));
        actions
    }

    /// `onCtrlD`.
    pub fn handle_ctrl_d(&mut self) -> Vec<EditorAction> {
        if self.pending_exit == Some(ExitKind::CtrlD) {
            self.clear_pending_exit();
            return vec![EditorAction::Exit];
        }
        self.arm_pending_exit(ExitKind::CtrlD);
        vec![EditorAction::ArmPendingExit(ExitKind::CtrlD)]
    }

    /// `onEscape`.
    pub fn handle_escape(&mut self, ctx: &EditorContext) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        if self.pending_exit.is_some() {
            self.clear_pending_exit();
        }
        if ctx.active_dialog.as_deref() == Some("session-picker") {
            self.clear_pending_undo_esc();
            actions.push(EditorAction::HideSessionPicker);
            return actions;
        }
        if ctx.btw_active {
            self.clear_pending_undo_esc();
            actions.push(EditorAction::BtwCloseOrCancel);
            return actions;
        }
        if ctx.is_compacting {
            self.clear_pending_undo_esc();
            actions.push(EditorAction::CancelCompaction);
            return actions;
        }
        if !ctx.streaming_phase.is_idle() {
            self.clear_pending_undo_esc();
            actions.push(EditorAction::CancelStream);
            return actions;
        }
        // Idle: a second Esc within the double-tap window opens the undo
        // selector.
        if self.pending_undo_esc {
            self.clear_pending_undo_esc();
            actions.push(EditorAction::OpenUndoSelector);
            return actions;
        }
        self.arm_pending_undo_esc();
        actions
    }

    /// `onShiftTab` — plan-mode toggle.
    pub fn handle_shift_tab(&mut self, ctx: &EditorContext) -> Vec<EditorAction> {
        if !ctx.has_session {
            return vec![EditorAction::NoSession];
        }
        let next = !ctx.plan_mode;
        vec![EditorAction::TogglePlanMode { enabled: next }]
    }

    /// `onToggleToolExpand`.
    pub fn handle_toggle_tool_expand(&mut self) -> EditorAction {
        EditorAction::ToggleToolOutputExpansion
    }

    /// `onToggleTodoExpand` — returns whether the action was handled.
    pub fn handle_toggle_todo_expand(&mut self, ctx: &EditorContext) -> Option<EditorAction> {
        if !ctx.todo_panel_has_overflow {
            return None;
        }
        // Disarm a pending double-press exit confirmation so expanding the
        // todo list in between two Ctrl-C presses does not accidentally exit.
        self.clear_pending_exit();
        Some(EditorAction::ToggleTodoExpansion)
    }

    /// `onUpArrowEmpty`.
    pub fn handle_up_arrow_empty(&mut self, ctx: &EditorContext) -> Vec<EditorAction> {
        // TODO(legacy): btwPanelController.scroll('up') — component decision.
        if ctx.streaming_phase.is_idle() && !ctx.is_compacting {
            return Vec::new();
        }
        if let Some(recalled) = ctx.last_queued.clone() {
            return vec![EditorAction::RecallLastQueued(recalled)];
        }
        Vec::new()
    }

    /// `onDownArrowEmpty` — always delegates to btw scroll (component).
    pub fn handle_down_arrow_empty(&mut self) -> Vec<EditorAction> {
        // TODO(legacy): btwPanelController.scroll('down')
        Vec::new()
    }

    /// `onCtrlS` — the steer gating decision (whether the editor is in a
    /// steerable phase).
    pub fn steer_phase_allowed(&self, ctx: &EditorContext) -> bool {
        !ctx.streaming_phase.is_idle()
            && ctx.streaming_phase != StreamingPhase::Shell
            && !ctx.is_compacting
    }

    // -----------------------------------------------------------------------
    // Pending-exit / undo-esc state machine
    // -----------------------------------------------------------------------

    /// `clearPendingExit`.
    pub fn clear_pending_exit(&mut self) {
        if self.pending_exit.is_none() {
            return;
        }
        // TODO(legacy): clearTimeout(timer); footer.setTransientHint(null)
        self.pending_exit = None;
    }

    /// `armPendingExit`.
    pub fn arm_pending_exit(&mut self, kind: ExitKind) {
        self.clear_pending_exit();
        // TODO(legacy): footer.setTransientHint(hint); setTimeout(…, EXIT_CONFIRM_WINDOW_MS)
        self.pending_exit = Some(kind);
    }

    /// `armPendingUndoEsc`.
    pub fn arm_pending_undo_esc(&mut self) {
        self.clear_pending_undo_esc();
        // TODO(legacy): setTimeout(…, DOUBLE_ESC_WINDOW_MS)
        self.pending_undo_esc = true;
    }

    /// `clearPendingUndoEsc`.
    pub fn clear_pending_undo_esc(&mut self) {
        // TODO(legacy): clearTimeout(timer)
        self.pending_undo_esc = false;
    }

    /// `dispose`.
    pub fn dispose(&mut self) {
        self.clear_pending_exit();
        self.clear_pending_undo_esc();
    }
}

/// `computeSteerItems` — the Ctrl-S queue projection: steerable (non-bash)
/// queued messages plus the editor draft, leaving only bash messages queued.
pub fn compute_steer_items(
    queue: &[QueuedMessage],
    editor_text: &str,
    editor_is_bash: bool,
) -> (Vec<SteerInputItem>, Vec<QueuedMessage>) {
    let mut items = Vec::new();
    for m in queue {
        if m.mode == MessageMode::Bash {
            continue;
        }
        let trimmed = m.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        items.push(SteerInputItem {
            text: trimmed.to_owned(),
        });
    }
    if !editor_is_bash {
        let trimmed = editor_text.trim();
        if !trimmed.is_empty() {
            items.push(SteerInputItem {
                text: trimmed.to_owned(),
            });
        }
    }
    let remaining: Vec<QueuedMessage> = queue
        .iter()
        .filter(|m| m.mode == MessageMode::Bash)
        .cloned()
        .collect();
    (items, remaining)
}

/// `onRecall` — recalling a `!`-prefixed entry strips the marker and returns
/// to bash mode; a plain entry returns to prompt mode.
pub fn recall_mode_for(entry: &str) -> (MessageMode, Option<String>) {
    if let Some(rest) = entry.strip_prefix('!') {
        (MessageMode::Bash, Some(rest.to_owned()))
    } else {
        (MessageMode::Prompt, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EditorContext {
        EditorContext {
            cancel_in_flight: false,
            btw_running: false,
            btw_active: false,
            is_compacting: false,
            streaming_phase: StreamingPhase::Idle,
            has_text: false,
            has_session: true,
            plan_mode: false,
            active_dialog: None,
            todo_panel_has_overflow: false,
            input_mode: MessageMode::Prompt,
            model_set: true,
            queue: Vec::new(),
            last_queued: None,
        }
    }

    #[test]
    fn ctrl_c_first_press_arms_exit_second_exits() {
        let mut k = EditorKeyboardController::new();
        let c = ctx();
        let actions = k.handle_ctrl_c(&c);
        assert!(actions.contains(&EditorAction::ArmPendingExit(ExitKind::CtrlC)));
        assert_eq!(k.pending_exit(), Some(ExitKind::CtrlC));

        let actions = k.handle_ctrl_c(&c);
        assert!(actions.contains(&EditorAction::Exit));
        assert_eq!(k.pending_exit(), None);
    }

    #[test]
    fn ctrl_c_cancels_stream_when_busy() {
        let mut k = EditorKeyboardController::new();
        let mut c = ctx();
        c.streaming_phase = StreamingPhase::Composing;
        let actions = k.handle_ctrl_c(&c);
        assert!(actions.contains(&EditorAction::CancelStream));
        assert_eq!(k.pending_exit(), None);
    }

    #[test]
    fn ctrl_c_with_text_clears_text_first() {
        let mut k = EditorKeyboardController::new();
        let mut c = ctx();
        c.streaming_phase = StreamingPhase::Composing;
        c.has_text = true;
        let actions = k.handle_ctrl_c(&c);
        assert!(actions.contains(&EditorAction::ClearEditorText));
        assert!(!actions.contains(&EditorAction::CancelStream));
    }

    #[test]
    fn ctrl_c_btw_takes_priority_over_stream() {
        let mut k = EditorKeyboardController::new();
        let mut c = ctx();
        c.streaming_phase = StreamingPhase::Composing;
        c.btw_active = true;
        let actions = k.handle_ctrl_c(&c);
        assert!(actions.contains(&EditorAction::BtwCloseOrCancel));
    }

    #[test]
    fn ctrl_d_two_presses_exit() {
        let mut k = EditorKeyboardController::new();
        assert!(
            k.handle_ctrl_d()
                .contains(&EditorAction::ArmPendingExit(ExitKind::CtrlD))
        );
        assert!(k.handle_ctrl_d().contains(&EditorAction::Exit));
    }

    #[test]
    fn escape_double_opens_undo_selector() {
        let mut k = EditorKeyboardController::new();
        let c = ctx();
        let first = k.handle_escape(&c);
        assert!(!first.contains(&EditorAction::OpenUndoSelector));
        assert!(k.pending_undo_esc());
        let second = k.handle_escape(&c);
        assert!(second.contains(&EditorAction::OpenUndoSelector));
        assert!(!k.pending_undo_esc());
    }

    #[test]
    fn escape_hides_session_picker() {
        let mut k = EditorKeyboardController::new();
        let mut c = ctx();
        c.active_dialog = Some("session-picker".to_owned());
        let actions = k.handle_escape(&c);
        assert!(actions.contains(&EditorAction::HideSessionPicker));
    }

    #[test]
    fn shift_tab_requires_session() {
        let mut k = EditorKeyboardController::new();
        let mut c = ctx();
        c.has_session = false;
        assert!(k.handle_shift_tab(&c).contains(&EditorAction::NoSession));
        c.has_session = true;
        c.plan_mode = false;
        let actions = k.handle_shift_tab(&c);
        assert!(actions.contains(&EditorAction::TogglePlanMode { enabled: true }));
    }

    #[test]
    fn todo_expand_disarms_pending_exit() {
        let mut k = EditorKeyboardController::new();
        k.arm_pending_exit(ExitKind::CtrlC);
        let mut c = ctx();
        c.todo_panel_has_overflow = true;
        let action = k.handle_toggle_todo_expand(&c);
        assert_eq!(action, Some(EditorAction::ToggleTodoExpansion));
        assert_eq!(k.pending_exit(), None);

        c.todo_panel_has_overflow = false;
        assert_eq!(k.handle_toggle_todo_expand(&c), None);
    }

    #[test]
    fn up_arrow_empty_recalls_queued_message() {
        let mut k = EditorKeyboardController::new();
        let mut c = ctx();
        c.streaming_phase = StreamingPhase::Composing;
        c.last_queued = Some(QueuedMessage::bash("! ls"));
        let actions = k.handle_up_arrow_empty(&c);
        assert!(actions.contains(&EditorAction::RecallLastQueued(QueuedMessage::bash("! ls"))));
        // Idle + not compacting → no recall.
        c.streaming_phase = StreamingPhase::Idle;
        assert!(k.handle_up_arrow_empty(&c).is_empty());
    }

    #[test]
    fn steer_phase_gating() {
        let k = EditorKeyboardController::new();
        let mut c = ctx();
        assert!(!k.steer_phase_allowed(&c)); // idle
        c.streaming_phase = StreamingPhase::Composing;
        assert!(k.steer_phase_allowed(&c));
        c.streaming_phase = StreamingPhase::Shell;
        assert!(!k.steer_phase_allowed(&c));
        c.streaming_phase = StreamingPhase::Composing;
        c.is_compacting = true;
        assert!(!k.steer_phase_allowed(&c));
    }

    #[test]
    fn compute_steer_items_bash_remain_queued() {
        let queue = vec![
            QueuedMessage::prompt("  first  "),
            QueuedMessage::bash("! ls"),
            QueuedMessage::prompt("second"),
            QueuedMessage::bash("! pwd"),
        ];
        let (items, remaining) = compute_steer_items(&queue, "editor draft", false);
        assert_eq!(
            items,
            vec![
                SteerInputItem {
                    text: "first".to_owned()
                },
                SteerInputItem {
                    text: "second".to_owned()
                },
                SteerInputItem {
                    text: "editor draft".to_owned()
                },
            ]
        );
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().all(|m| m.mode == MessageMode::Bash));
    }

    #[test]
    fn compute_steer_items_skips_editor_in_bash_mode() {
        let (items, _) = compute_steer_items(&[], "! echo hi", true);
        assert!(items.is_empty());
    }

    #[test]
    fn recall_mode_strips_bang() {
        assert_eq!(
            recall_mode_for("! ls"),
            (MessageMode::Bash, Some(" ls".to_owned()))
        );
        assert_eq!(recall_mode_for("hello"), (MessageMode::Prompt, None));
    }
}
