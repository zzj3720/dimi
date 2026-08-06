//! Undo message selector — port of
//! `apps/dimi/src/tui/components/dialogs/undo-selector.ts`
//! (`UndoSelectorComponent`).
//!
//! Lists recent user messages that can be undone; the cursor starts on the
//! most recent message (last choice) and Enter commits the selection.

use crate::component::Component;
use crate::keys::matches_key;
use crate::searchable_list::SearchableList;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

use super::SELECT_POINTER;

const MAX_VISIBLE_CHOICES: usize = 5;
const PREFERRED_SELECTED_OFFSET: usize = 2;

/// One undoable message (`UndoChoice`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoChoice {
    pub id: String,
    pub count: usize,
    pub input: String,
    pub label: String,
}

/// Action surfaced via [`UndoSelectorComponent::take_action`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoSelectorAction {
    Select(UndoChoice),
    Cancel,
}

/// `UndoSelectorOptions`.
#[derive(Debug, Clone)]
pub struct UndoSelectorOptions {
    pub choices: Vec<UndoChoice>,
}

/// `UndoSelectorComponent`.
pub struct UndoSelectorComponent {
    list: SearchableList<UndoChoice>,
    submitted: bool,
    action: Option<UndoSelectorAction>,
}

impl UndoSelectorComponent {
    pub fn new(opts: UndoSelectorOptions) -> Self {
        let initial_index = opts.choices.len().saturating_sub(1);
        let list = SearchableList::new(
            opts.choices.clone(),
            |choice| choice.label.clone(),
            None,
            Some(initial_index),
            false,
        );
        UndoSelectorComponent {
            list,
            submitted: false,
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<UndoSelectorAction> {
        self.action.take()
    }

    fn render_choice_line(
        &self,
        choice: &UndoChoice,
        is_selected: bool,
        in_undo_range: bool,
        width: usize,
    ) -> String {
        let theme = current_theme();
        let pointer = if is_selected { SELECT_POINTER } else { " " };
        let prefix = format!("  {pointer} ");
        let label_budget = 8usize.max(width.saturating_sub(visible_width(&prefix)));
        let label = truncate_to_width(&choice.label, label_budget, "…", false);
        let token = if is_selected {
            ColorToken::Primary
        } else if in_undo_range {
            ColorToken::TextDim
        } else {
            ColorToken::Text
        };
        let mut line = theme.fg(
            if is_selected {
                ColorToken::Primary
            } else {
                ColorToken::TextDim
            },
            &prefix,
        );
        line.push_str(&if is_selected {
            theme.bold_fg(token, &label)
        } else {
            theme.fg(token, &label)
        });
        line
    }
}

impl Component for UndoSelectorComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let view = self.list.view();
        let hint_parts = ["↑↓ navigate", "Enter select", "Esc cancel"];

        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            theme.bold_fg(ColorToken::Primary, " Select messages to undo"),
            theme.fg(
                ColorToken::TextMuted,
                &format!(" {}", hint_parts.join(" · ")),
            ),
            String::new(),
        ];

        if view.items.is_empty() {
            lines.push(theme.fg(ColorToken::TextMuted, "   No messages"));
        } else {
            let visible_count = MAX_VISIBLE_CHOICES.min(view.items.len());
            let max_start = view.items.len() - visible_count;
            let start = view
                .selected_index
                .saturating_sub(PREFERRED_SELECTED_OFFSET)
                .min(max_start);
            let end = start + visible_count;

            for i in start..end {
                if let Some(choice) = view.items.get(i) {
                    lines.push(self.render_choice_line(
                        choice,
                        i == view.selected_index,
                        i > view.selected_index,
                        width,
                    ));
                }
            }
        }

        lines.push(String::new());
        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "...", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if self.submitted {
            return;
        }

        if matches_key(data, "escape") {
            self.action = Some(UndoSelectorAction::Cancel);
            return;
        }

        if self.list.handle_key(data) {
            return;
        }

        if matches_key(data, "enter") {
            if let Some(selected) = self.list.selected() {
                self.submitted = true;
                self.action = Some(UndoSelectorAction::Select(selected));
            }
        }
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices() -> Vec<UndoChoice> {
        vec![
            UndoChoice {
                id: "a".to_owned(),
                count: 1,
                input: "first".to_owned(),
                label: "1. first message".to_owned(),
            },
            UndoChoice {
                id: "b".to_owned(),
                count: 2,
                input: "second".to_owned(),
                label: "2. second message".to_owned(),
            },
            UndoChoice {
                id: "c".to_owned(),
                count: 3,
                input: "third".to_owned(),
                label: "3. third message".to_owned(),
            },
        ]
    }

    #[test]
    fn starts_on_most_recent() {
        let mut c = UndoSelectorComponent::new(UndoSelectorOptions { choices: choices() });
        c.handle_input("\r");
        match c.take_action() {
            Some(UndoSelectorAction::Select(choice)) => assert_eq!(choice.id, "c"),
            other => panic!("expected select, got {other:?}"),
        }
    }

    #[test]
    fn renders_all_choices() {
        let mut c = UndoSelectorComponent::new(UndoSelectorOptions { choices: choices() });
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Select messages to undo"), "{joined}");
        assert!(joined.contains("1. first message"), "{joined}");
        assert!(joined.contains("3. third message"), "{joined}");
    }

    #[test]
    fn up_moves_selection() {
        let mut c = UndoSelectorComponent::new(UndoSelectorOptions { choices: choices() });
        c.handle_input("\x1b[A");
        c.handle_input("\r");
        match c.take_action() {
            Some(UndoSelectorAction::Select(choice)) => assert_eq!(choice.id, "b"),
            other => panic!("expected select, got {other:?}"),
        }
    }
}
