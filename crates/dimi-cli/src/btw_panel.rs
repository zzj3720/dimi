//! `BtwPanelComponent` — the rendering half of the BTW panel (port of
//! `apps/dimi/src/tui/components/panes/btw-panel.ts`).
//!
//! The state machine lives in [`dimi_tui::controllers::btw::BtwPanelController`]
//! (pure: it does not retain the submitted prompt). This component holds the
//! prompt and mirrors the controller's answer / thinking / phase / failure into
//! the rendered turns via [`BtwPanelComponent::sync`], so the panel is driven
//! by `controller.active()` exactly as the TS reads its `BtwPanelComponent`.
//! Renders nothing (0 lines) while closed, so the layout budget is untouched
//! until a `/btw` prompt opens it.

use dimi_tui::component::Component;
use dimi_tui::components::text::Text;
use dimi_tui::controllers::btw::{BTW_BUSY_NOTICE, BtwPanelState};
use dimi_tui::theme::{ColorToken, current_theme};
use dimi_tui::width::visible_width;
use dimi_tui::wrap::truncate_to_width;

/// The BTW turn phase (`BtwPanelPhase` in the TS source).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtwPhase {
    Running,
    Done,
    Failed,
}

/// The BTW panel: a bordered pane showing the current turn's prompt, its
/// answer (or dimmed thinking, or a waiting placeholder), and any failure
/// message — plus the busy notice while input is gated.
pub struct BtwPanelComponent {
    prompt: Option<String>,
    answer: String,
    thinking: String,
    phase: BtwPhase,
    failed_message: Option<String>,
    busy_notice: bool,
}

impl BtwPanelComponent {
    pub fn new() -> Self {
        BtwPanelComponent {
            prompt: None,
            answer: String::new(),
            thinking: String::new(),
            phase: BtwPhase::Running,
            failed_message: None,
            busy_notice: false,
        }
    }

    /// Whether the panel is currently mounted (a prompt is shown).
    pub fn is_open(&self) -> bool {
        self.prompt.is_some()
    }

    /// The current turn phase (drives tests and any host-side decision).
    /// `#[allow(dead_code)]`: read by tests; the app drives the panel through
    /// the controller + `sync` and never reads the phase directly.
    #[allow(dead_code)]
    pub fn phase(&self) -> BtwPhase {
        self.phase
    }

    /// Open the panel with the user's prompt — the rendering half of
    /// [`BtwPanelController::open`] (the controller discards the prompt).
    pub fn open(&mut self, prompt: &str) {
        self.prompt = Some(prompt.to_owned());
        self.answer.clear();
        self.thinking.clear();
        self.phase = BtwPhase::Running;
        self.failed_message = None;
        self.busy_notice = false;
    }

    /// Close the panel (renders nothing afterwards).
    pub fn close(&mut self) {
        self.prompt = None;
        self.answer.clear();
        self.thinking.clear();
        self.phase = BtwPhase::Running;
        self.failed_message = None;
        self.busy_notice = false;
    }

    /// Mirror the controller's active state into the render fields. Call after
    /// [`BtwPanelController::route_event`] so the panel follows the state
    /// machine (the component never routes events itself).
    pub fn sync(&mut self, state: &BtwPanelState) {
        self.answer = state.answer.clone();
        self.thinking = state.thinking.clone();
        self.phase = if state.done {
            BtwPhase::Done
        } else if state.failed_message.is_some() {
            BtwPhase::Failed
        } else {
            BtwPhase::Running
        };
        self.failed_message = state.failed_message.clone();
        self.busy_notice = state.busy_notice;
    }

    /// `renderTopBorder` — `╭ BTW ─ <hint> ───╮` (accent title, border dashes).
    fn render_top_border(&self, width: usize) -> String {
        let theme = current_theme();
        let hint = "Esc close ";
        let mut title = theme.bold_fg(ColorToken::Accent, " BTW ");
        title.push_str(&theme.fg(ColorToken::Border, "─ "));
        title.push_str(&theme.fg(ColorToken::TextMuted, hint));
        let inner_width = width.saturating_sub(2).max(1);
        let clipped = if visible_width(&title) > inner_width {
            truncate_to_width(&title, inner_width, "", false)
        } else {
            title
        };
        let dash_count = inner_width.saturating_sub(visible_width(&clipped));
        format!(
            "{}{}{}{}",
            theme.fg(ColorToken::Border, "╭"),
            clipped,
            theme.fg(ColorToken::Border, &"─".repeat(dash_count)),
            theme.fg(ColorToken::Border, "╮"),
        )
    }

    /// `renderBody` — the turn card at the inner content width.
    fn render_body(&self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let mut lines: Vec<String> = Vec::new();
        match &self.prompt {
            Some(prompt) => {
                let prompt_line = theme.fg(ColorToken::Accent, &format!("Q: {prompt}"));
                lines.extend(Text::new(&prompt_line, 0, 0).render(width));
                let answer = self.answer.trim();
                let thinking = self.thinking.trim();
                if !answer.is_empty() {
                    lines.extend(Text::new(answer, 0, 0).render(width));
                } else if !thinking.is_empty() {
                    let dim = theme.fg(ColorToken::TextDim, thinking);
                    lines.extend(Text::new(&dim, 0, 0).render(width));
                } else if self.failed_message.is_none() {
                    lines.push(theme.fg(ColorToken::TextDim, "Waiting for answer..."));
                }
                if let Some(err) = &self.failed_message {
                    let styled = theme.fg(ColorToken::Error, err);
                    lines.extend(Text::new(&styled, 0, 0).render(width));
                }
            }
            None => {
                lines.push(theme.fg(ColorToken::TextDim, "Ready for a side question..."));
            }
        }
        if self.busy_notice {
            lines.push(theme.fg(ColorToken::TextDim, BTW_BUSY_NOTICE));
        }
        lines
    }

    /// `renderBodyLine` — `│ content │` (clipped to the inner width, padded).
    fn render_body_line(&self, line: &str, width: usize) -> String {
        let theme = current_theme();
        let content_width = width.saturating_sub(4).max(1);
        let clipped = if visible_width(line) > content_width {
            truncate_to_width(line, content_width, "…", false)
        } else {
            line.to_owned()
        };
        let padding = content_width.saturating_sub(visible_width(&clipped));
        format!(
            "{}{}{}{}{}",
            theme.fg(ColorToken::Border, "│"),
            " ",
            clipped,
            " ".repeat(padding),
            theme.fg(ColorToken::Border, "│"),
        )
    }
}

impl Default for BtwPanelComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for BtwPanelComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.prompt.is_none() {
            return Vec::new();
        }
        let safe_width = width.max(4);
        let content_width = safe_width.saturating_sub(4).max(1);
        let body = self.render_body(content_width);
        let mut lines = vec![self.render_top_border(safe_width)];
        for line in body {
            lines.push(self.render_body_line(&line, safe_width));
        }
        lines
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_tui::ansi::strip_ansi;
    use dimi_tui::controllers::btw::BtwPanelController;
    use dimi_tui::controllers::events::{Event, TurnEndReason};
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    const AGENT: &str = "btw";

    #[test]
    fn closed_renders_zero_lines() {
        set_palette(DARK_COLORS);
        let mut p = BtwPanelComponent::new();
        assert!(!p.is_open());
        assert!(Component::render(&mut p, 80).is_empty());
    }

    #[test]
    fn open_renders_prompt_then_answer_then_done() {
        set_palette(DARK_COLORS);
        let mut controller = BtwPanelController::new();
        let mut panel = BtwPanelComponent::new();

        controller.open(AGENT, "hello");
        panel.open("hello");
        assert!(panel.is_open());
        let joined = strip_ansi(&Component::render(&mut panel, 80).join("\n"));
        assert!(joined.contains("Q: hello"), "prompt shown: {joined}");
        assert!(joined.contains("Waiting for answer"), "{joined}");

        // An assistant delta routes through the controller → sync → render.
        controller.route_event(&Event::AssistantDelta {
            agent_id: Some(AGENT.to_owned()),
            delta: "hi there".to_owned(),
        });
        panel.sync(controller.active().unwrap());
        let joined = strip_ansi(&Component::render(&mut panel, 80).join("\n"));
        assert!(joined.contains("hi there"), "answer shown: {joined}");

        // Turn end completes the phase.
        controller.route_event(&Event::TurnEnded {
            agent_id: Some(AGENT.to_owned()),
            turn_id: "1".to_owned(),
            reason: TurnEndReason::Completed,
            error: None,
        });
        panel.sync(controller.active().unwrap());
        assert_eq!(panel.phase(), BtwPhase::Done);
        let joined = strip_ansi(&Component::render(&mut panel, 80).join("\n"));
        assert!(
            joined.contains("hi there"),
            "answer persists after done: {joined}"
        );
        assert!(joined.contains("BTW"), "top border: {joined}");
    }

    #[test]
    fn failed_turn_sets_failed_phase_and_shows_error() {
        set_palette(DARK_COLORS);
        let mut controller = BtwPanelController::new();
        let mut panel = BtwPanelComponent::new();
        controller.open(AGENT, "question");
        panel.open("question");

        controller.route_event(&Event::TurnEnded {
            agent_id: Some(AGENT.to_owned()),
            turn_id: "1".to_owned(),
            reason: TurnEndReason::Failed,
            error: Some(dimi_tui::controllers::events::ErrorPayload {
                code: "boom".to_owned(),
                message: "it broke".to_owned(),
                details: None,
            }),
        });
        panel.sync(controller.active().unwrap());
        assert_eq!(panel.phase(), BtwPhase::Failed);
        let joined = strip_ansi(&Component::render(&mut panel, 80).join("\n"));
        assert!(joined.contains("[boom] it broke"), "error shown: {joined}");
    }

    #[test]
    fn close_hides_the_panel() {
        set_palette(DARK_COLORS);
        let mut panel = BtwPanelComponent::new();
        panel.open("hi");
        assert!(!Component::render(&mut panel, 80).is_empty());
        panel.close();
        assert!(!panel.is_open());
        assert!(Component::render(&mut panel, 80).is_empty());
    }
}
