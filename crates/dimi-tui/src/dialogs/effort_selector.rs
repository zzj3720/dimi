//! Horizontal segmented thinking-effort picker — port of
//! `apps/dimi/src/tui/components/dialogs/effort-selector.ts`
//! (`EffortSelectorComponent`).
//!
//! Mirrors the thinking control rendered under `/model`: a single row of
//! segments, the active one wrapped in `[ ]`. ←/→ step the active segment,
//! Enter commits, and Alt+S (when available) applies session-only.

use crate::component::Component;
use crate::keys::matches_key;
use crate::theme::{ColorToken, current_theme};
use crate::wrap::{truncate_to_width, wrap_text_with_ansi};

use super::model_common::effort_label;

/// Action surfaced via [`EffortSelectorComponent::take_action`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffortSelectorAction {
    Select(String),
    SessionOnly(String),
    Cancel,
}

/// `EffortSelectorOptions`.
#[derive(Debug, Clone)]
pub struct EffortSelectorOptions {
    pub title: Option<String>,
    /// Selectable thinking efforts (e.g. `["off","low","high","max"]`).
    pub efforts: Vec<String>,
    /// Currently active effort (highlighted).
    pub current_value: String,
    /// Whether Alt+S (session-only) is available.
    pub has_session_only: bool,
    /// Warning-colored lines below the key-hint line.
    pub warning: Option<String>,
}

/// `EffortSelectorComponent`.
pub struct EffortSelectorComponent {
    opts: EffortSelectorOptions,
    active_index: usize,
    action: Option<EffortSelectorAction>,
}

impl EffortSelectorComponent {
    pub fn new(opts: EffortSelectorOptions) -> Self {
        let idx = opts.efforts.iter().position(|e| *e == opts.current_value);
        let active_index = idx.unwrap_or(0);
        EffortSelectorComponent {
            opts,
            active_index,
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<EffortSelectorAction> {
        self.action.take()
    }
}

impl Component for EffortSelectorComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let mut hint_parts = vec!["←→ switch", "Enter select"];
        if self.opts.has_session_only {
            hint_parts.push("Alt+S session-only");
        }
        hint_parts.push("Esc cancel");

        let title = self
            .opts
            .title
            .clone()
            .unwrap_or_else(|| "Select thinking effort".to_owned());
        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            theme.bold_fg(ColorToken::Primary, &format!(" {title}")),
            theme.fg(
                ColorToken::TextMuted,
                &format!(" {}", hint_parts.join(" · ")),
            ),
        ];
        if let Some(warning) = &self.opts.warning {
            for line in wrap_text_with_ansi(warning, width.saturating_sub(1).max(1)) {
                lines.push(theme.fg(ColorToken::Warning, &format!(" {line}")));
            }
        }
        lines.push(String::new());

        let segments: Vec<String> = self
            .opts
            .efforts
            .iter()
            .enumerate()
            .map(|(index, effort)| {
                let label = effort_label(effort);
                if index == self.active_index {
                    theme.bold_fg(ColorToken::Primary, &format!("[ {label} ]"))
                } else {
                    theme.fg(ColorToken::Text, &format!("  {label}  "))
                }
            })
            .collect();
        lines.push(format!("  {}", segments.join("  ")));

        lines.push(String::new());
        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "...", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") {
            self.action = Some(EffortSelectorAction::Cancel);
            return;
        }
        if matches_key(data, "left") {
            self.active_index = self.active_index.saturating_sub(1);
            return;
        }
        if matches_key(data, "right") {
            self.active_index = self
                .active_index
                .saturating_add(1)
                .min(self.opts.efforts.len().saturating_sub(1));
            return;
        }
        if matches_key(data, "alt+s") && self.opts.has_session_only {
            if let Some(effort) = self.opts.efforts.get(self.active_index) {
                self.action = Some(EffortSelectorAction::SessionOnly(effort.clone()));
            }
            return;
        }
        if matches_key(data, "enter") {
            if let Some(effort) = self.opts.efforts.get(self.active_index) {
                self.action = Some(EffortSelectorAction::Select(effort.clone()));
            }
        }
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> EffortSelectorOptions {
        EffortSelectorOptions {
            title: None,
            efforts: vec!["off", "low", "high", "max"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            current_value: "high".to_owned(),
            has_session_only: true,
            warning: None,
        }
    }

    #[test]
    fn renders_segments_with_active_brackets() {
        let mut c = EffortSelectorComponent::new(opts());
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("[ High ]"), "{joined}");
        assert!(joined.contains("Off"), "{joined}");
        assert!(joined.contains("←→ switch"), "{joined}");
        assert!(joined.contains("Alt+S session-only"), "{joined}");
    }

    #[test]
    fn left_right_step_segments() {
        let mut c = EffortSelectorComponent::new(opts());
        c.handle_input("\x1b[D"); // left
        c.handle_input("\r");
        assert_eq!(
            c.take_action(),
            Some(EffortSelectorAction::Select("low".to_owned()))
        );
        c.handle_input("\x1b[C"); // right
        c.handle_input("\x1b[C"); // right
        c.handle_input("\r");
        assert_eq!(
            c.take_action(),
            Some(EffortSelectorAction::Select("max".to_owned()))
        );
    }

    #[test]
    fn alt_s_session_only() {
        let mut c = EffortSelectorComponent::new(opts());
        c.handle_input("\x1bs");
        assert_eq!(
            c.take_action(),
            Some(EffortSelectorAction::SessionOnly("high".to_owned()))
        );
    }

    #[test]
    fn escape_cancels() {
        let mut c = EffortSelectorComponent::new(opts());
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(EffortSelectorAction::Cancel));
    }
}
