//! BTW panel controller — port of `apps/dimi/src/tui/controllers/btw-panel.ts`.
//!
//! State machine for the "by the way" interactive subagent panel: open /
//! close / cancel / send input, busy-notice gating, and child-event routing
//! (assistant/thinking deltas, hook results, turn end). The `BtwPanelComponent`
//! rendering and the async `session.prompt` / `session.cancel` are
//! `// TODO(legacy)`.

use std::collections::BTreeSet;

use crate::controllers::event_handler::format_hook_result_plain;
use crate::controllers::events::{ErrorPayload, Event, TurnEndReason};

/// `BTW_BUSY_NOTICE` — shown when the user sends input while the panel's agent
/// is still running.
pub const BTW_BUSY_NOTICE: &str = "Wait for /btw to finish before sending another question.";

/// Pure state of the active BTW panel (`BtwPanelComponent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtwPanelState {
    pub agent_id: String,
    pub running: bool,
    pub answer: String,
    pub thinking: String,
    pub done: bool,
    pub failed_message: Option<String>,
    pub busy_notice: bool,
    /// True when the panel carries no answer yet (cancel-on-unmount policy).
    pub empty: bool,
}

impl BtwPanelState {
    fn new(agent_id: &str) -> Self {
        BtwPanelState {
            agent_id: agent_id.to_owned(),
            running: false,
            answer: String::new(),
            thinking: String::new(),
            done: false,
            failed_message: None,
            busy_notice: false,
            empty: true,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// `shouldCancelOnUnmount`.
    fn should_cancel_on_unmount(&self) -> bool {
        self.running || self.empty
    }
}

/// The BTW panel controller (port of `BtwPanelController`).
#[derive(Debug, Default)]
pub struct BtwPanelController {
    active: Option<BtwPanelState>,
    panels_by_agent_id: BTreeSet<String>,
}

impl BtwPanelController {
    pub fn new() -> Self {
        BtwPanelController::default()
    }

    pub fn active(&self) -> Option<&BtwPanelState> {
        self.active.as_ref()
    }

    /// `open` — create the panel and submit the initial prompt.
    pub fn open(&mut self, agent_id: &str, initial_prompt: &str) {
        // TODO(legacy): create BtwPanelComponent; mount with Spacer;
        //   editor.connectedAbove = true
        self.active = Some(BtwPanelState::new(agent_id));
        self.panels_by_agent_id.insert(agent_id.to_owned());
        self.submit_prompt(initial_prompt);
    }

    /// `clear`.
    pub fn clear(&mut self) {
        if let Some(active) = &self.active {
            if active.should_cancel_on_unmount() {
                // TODO(legacy): void this.cancelAgent(active.agentId)
            }
        }
        self.active = None;
        self.panels_by_agent_id.clear();
        // TODO(legacy): state.btwPanelContainer.clear(); editor.connectedAbove = false
    }

    /// `closeOrCancel` — dismiss the panel, cancelling when it is still
    /// running / empty. Returns true when a panel was active.
    pub fn close_or_cancel(&mut self) -> bool {
        let Some(active) = &self.active else {
            return false;
        };
        let should_cancel = active.should_cancel_on_unmount();
        self.close();
        if should_cancel {
            // TODO(legacy): void this.cancelAgent(active.agentId)
        }
        true
    }

    /// `cancelRunning` — cancel only when the panel's agent is running.
    pub fn cancel_running(&mut self) -> bool {
        let Some(active) = &self.active else {
            return false;
        };
        if !active.running {
            return false;
        }
        // TODO(legacy): void this.cancelAgent(active.agentId)
        true
    }

    /// `sendUserInput` — submit text, or show the busy notice when running.
    /// Returns true when a panel is active (the input was consumed).
    pub fn send_user_input(&mut self, text: &str) -> bool {
        let Some(active) = &mut self.active else {
            return false;
        };
        if active.running {
            active.busy_notice = true;
            // TODO(legacy): state.editor.setText(input);
            //   panel.addTransientNotice(BTW_BUSY_NOTICE)
            return true;
        }
        self.submit_prompt(text);
        // TODO(legacy): state.ui.setFocus(state.editor); requestRender()
        true
    }

    /// `scroll` — the scroll decision lives in the component; returns false
    /// (no scroll performed) in the pure state machine.
    pub fn scroll(&mut self, _direction: ScrollDirection) -> bool {
        // TODO(legacy): panel.scroll(direction) + requestRender()
        false
    }

    /// `routeEvent` — route a child-agent event to the matching panel.
    /// Returns true when consumed.
    pub fn route_event(&mut self, event: &Event) -> bool {
        // `event.agentId` (child-agent attribution) keys the panel lookup; the
        // wire `turn.ended` carries the agent id, so no active-panel fallback
        // is needed.
        let Some(agent_id) = event.agent_id().map(str::to_owned) else {
            return false;
        };
        if !self.panels_by_agent_id.contains(&agent_id) {
            return false;
        }
        let Some(panel) = &mut self.active else {
            return false;
        };
        if panel.agent_id != agent_id {
            return false;
        }
        match event {
            Event::AssistantDelta { delta, .. } => {
                panel.answer.push_str(delta);
                panel.empty = false;
                true
            }
            Event::ThinkingDelta { delta, .. } => {
                panel.thinking.push_str(delta);
                true
            }
            Event::HookResult {
                hook_event,
                content,
                blocked,
                ..
            } => {
                panel
                    .answer
                    .push_str(&format_hook_result_plain(hook_event, content, *blocked));
                panel.empty = false;
                true
            }
            Event::TurnEnded { reason, error, .. } => {
                if *reason == TurnEndReason::Completed {
                    panel.done = true;
                    panel.running = false;
                } else {
                    panel.failed_message = Some(format_btw_turn_end(*reason, error.as_ref()));
                    panel.running = false;
                }
                true
            }
            _ => true,
        }
    }

    fn submit_prompt(&mut self, _prompt: &str) {
        if let Some(panel) = &mut self.active {
            panel.running = true;
            panel.empty = false;
            panel.busy_notice = false;
            // TODO(legacy): withInteractiveAgent(agentId, () => session.prompt(prompt))
        }
    }

    fn close(&mut self) {
        let Some(agent_id) = self.active.as_ref().map(|p| p.agent_id.clone()) else {
            return;
        };
        // Only unregister when the panel is actually mounted.
        if !self.panels_by_agent_id.contains(&agent_id) {
            return;
        }
        self.unregister(&agent_id);
        // TODO(legacy): state.btwPanelContainer.clear();
        //   editor.connectedAbove = false; setFocus(editor)
    }

    fn unregister(&mut self, agent_id: &str) {
        self.panels_by_agent_id.remove(agent_id);
        if self.active.as_ref().is_some_and(|p| p.agent_id == agent_id) {
            self.active = None;
        }
    }
}

/// Scroll direction for [`BtwPanelController::scroll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// `formatBtwTurnEnd` — the failure message for a non-completed BTW turn.
pub fn format_btw_turn_end(reason: TurnEndReason, error: Option<&ErrorPayload>) -> String {
    if reason == TurnEndReason::Cancelled {
        return "Interrupted by user".to_owned();
    }
    if let Some(error) = error {
        if error.code == "provider.filtered" {
            return "Provider safety policy blocked the response.".to_owned();
        }
        return format!("[{}] {}", error.code, error.message);
    }
    if reason == TurnEndReason::Blocked {
        return "Prompt hook blocked the request.".to_owned();
    }
    format!(
        "BTW turn ended with reason: {}",
        match reason {
            TurnEndReason::Completed => "completed",
            TurnEndReason::Cancelled => "cancelled",
            TurnEndReason::Failed => "failed",
            TurnEndReason::Blocked => "blocked",
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::events::ErrorPayload;

    #[test]
    fn open_submit_then_route_delta_and_turn_end() {
        let mut btw = BtwPanelController::new();
        assert!(btw.active().is_none());

        btw.open("child", "hello");
        let active = btw.active().unwrap();
        assert!(active.running);
        // The initial prompt was submitted, so the panel already has a turn
        // (TS `isEmpty()` = `turns.length === 0` → false).
        assert!(!active.empty);

        // A child assistant delta routes into the panel.
        assert!(btw.route_event(&Event::AssistantDelta {
            agent_id: Some("child".to_owned()),
            delta: "hi there".to_owned(),
        }));
        assert_eq!(btw.active().unwrap().answer, "hi there");
        assert!(!btw.active().unwrap().empty);

        // An event for another agent is not consumed.
        assert!(!btw.route_event(&Event::AssistantDelta {
            agent_id: Some("other".to_owned()),
            delta: "x".to_owned(),
        }));

        // Turn end completes the panel.
        assert!(btw.route_event(&Event::TurnEnded {
            agent_id: Some("child".to_owned()),
            turn_id: "1".to_owned(),
            reason: TurnEndReason::Completed,
            error: None,
        }));
        assert!(btw.active().unwrap().done);
        assert!(!btw.active().unwrap().running);
    }

    #[test]
    fn send_user_input_gates_on_running() {
        let mut btw = BtwPanelController::new();
        btw.open("child", "start");
        // Running → busy notice, no new prompt.
        assert!(btw.send_user_input("second"));
        let active = btw.active().unwrap();
        assert!(active.busy_notice);
        assert!(active.running);

        // Complete the turn, then the next input submits.
        btw.route_event(&Event::TurnEnded {
            agent_id: Some("child".to_owned()),
            turn_id: "1".to_owned(),
            reason: TurnEndReason::Completed,
            error: None,
        });
        assert!(btw.send_user_input("third"));
        assert!(btw.active().unwrap().running);
        assert!(!btw.active().unwrap().busy_notice);
    }

    #[test]
    fn close_or_cancel_cancels_when_running_or_empty() {
        let mut btw = BtwPanelController::new();
        assert!(!btw.close_or_cancel()); // nothing active
        btw.open("child", "start");
        assert!(btw.close_or_cancel());
        assert!(btw.active().is_none());

        // A completed panel closes without a pending cancel.
        btw.open("child", "start");
        btw.route_event(&Event::TurnEnded {
            agent_id: Some("child".to_owned()),
            turn_id: "1".to_owned(),
            reason: TurnEndReason::Completed,
            error: None,
        });
        assert!(btw.close_or_cancel());
    }

    #[test]
    fn cancel_running_only_when_running() {
        let mut btw = BtwPanelController::new();
        btw.open("child", "start");
        assert!(btw.cancel_running());
        // After the turn ends it is no longer running.
        btw.route_event(&Event::TurnEnded {
            agent_id: Some("child".to_owned()),
            turn_id: "1".to_owned(),
            reason: TurnEndReason::Failed,
            error: Some(ErrorPayload {
                code: "boom".to_owned(),
                message: "it broke".to_owned(),
                details: None,
            }),
        });
        assert!(!btw.active().unwrap().running);
        assert!(!btw.cancel_running());
    }

    #[test]
    fn format_btw_turn_end_variants() {
        assert_eq!(
            format_btw_turn_end(TurnEndReason::Cancelled, None),
            "Interrupted by user"
        );
        assert_eq!(
            format_btw_turn_end(
                TurnEndReason::Failed,
                Some(&ErrorPayload {
                    code: "provider.filtered".to_owned(),
                    message: "x".to_owned(),
                    details: None,
                })
            ),
            "Provider safety policy blocked the response."
        );
        assert_eq!(
            format_btw_turn_end(
                TurnEndReason::Failed,
                Some(&ErrorPayload {
                    code: "e2".to_owned(),
                    message: "bad".to_owned(),
                    details: None,
                })
            ),
            "[e2] bad"
        );
        assert_eq!(
            format_btw_turn_end(TurnEndReason::Blocked, None),
            "Prompt hook blocked the request."
        );
        assert_eq!(
            format_btw_turn_end(TurnEndReason::Completed, None),
            "BTW turn ended with reason: completed"
        );
    }
}
