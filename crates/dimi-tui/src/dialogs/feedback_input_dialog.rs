//! Feedback input dialog — blue rounded box collecting a single line of user
//! feedback. Port of
//! `apps/dimi/src/tui/components/dialogs/feedback-input-dialog.ts`
//! (`FeedbackInputDialogComponent`).

use crate::component::{Component, Focusable};
use crate::dialogs::input_line::{InputEvent, InputLine};
use crate::keys::matches_key;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

/// `FeedbackInputDialogResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackInputDialogResult {
    Ok(String),
    Cancel,
}

/// `FeedbackInputDialogComponent`.
pub struct FeedbackInputDialogComponent {
    input: InputLine,
    focused: bool,
    done: bool,
    empty_hinted: bool,
    action: Option<FeedbackInputDialogResult>,
}

impl Default for FeedbackInputDialogComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedbackInputDialogComponent {
    pub fn new() -> Self {
        FeedbackInputDialogComponent {
            input: InputLine::new(),
            focused: false,
            done: false,
            empty_hinted: false,
            action: None,
        }
    }

    /// Host polls after `handle_input` (mirrors the `onDone` callback).
    pub fn take_action(&mut self) -> Option<FeedbackInputDialogResult> {
        self.action.take()
    }

    fn submit(&mut self, value: &str) {
        if self.done {
            return;
        }
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.empty_hinted = true;
            return;
        }
        self.done = true;
        self.action = Some(FeedbackInputDialogResult::Ok(trimmed.to_owned()));
    }

    fn cancel(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        self.action = Some(FeedbackInputDialogResult::Cancel);
    }
}

impl Component for FeedbackInputDialogComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.input.set_focused(self.focused && !self.done);

        let safe_width = width;
        if safe_width == 0 {
            return vec![String::new()];
        }
        let inner_width = safe_width.saturating_sub(4).max(1);
        let pad = "  ";
        let theme = current_theme();

        let border = |s: String| theme.fg(ColorToken::Primary, &s);
        let title_styled = theme.bold_fg(ColorToken::TextStrong, "Send feedback to Dimi");
        let subtitle_text = if self.empty_hinted {
            "Feedback cannot be empty."
        } else {
            "Tell us what's working or what's not."
        };
        let subtitle_styled = theme.fg(ColorToken::TextDim, subtitle_text);
        let footer_styled = theme.fg(ColorToken::TextDim, "Enter to submit  ·  Esc to cancel");

        let title_line = truncate_to_width(&title_styled, inner_width, "…", false);
        let subtitle_line = truncate_to_width(&subtitle_styled, inner_width, "…", false);
        let footer_line = truncate_to_width(&footer_styled, inner_width, "…", false);
        let input_line = self.input.render(inner_width);

        let content_lines: Vec<String> = vec![
            title_line,
            String::new(),
            subtitle_line,
            String::new(),
            input_line,
            String::new(),
            footer_line,
        ];

        if safe_width < 4 {
            let mut out = vec![String::new()];
            for line in content_lines {
                out.push(truncate_to_width(&line, safe_width, "…", false));
            }
            return out;
        }

        let mut lines: Vec<String> = vec![
            String::new(),
            border(format!("╭{}╮", "─".repeat(safe_width - 2))),
            border(format!("│{}│", " ".repeat(safe_width - 2))),
        ];
        for content in content_lines {
            let vis = visible_width(&content);
            let right_pad = inner_width.saturating_sub(vis);
            lines.push(border(format!("│{pad}{content}{}│", " ".repeat(right_pad))));
        }
        lines.push(border(format!("│{}│", " ".repeat(safe_width - 2))));
        lines.push(border(format!("╰{}╯", "─".repeat(safe_width - 2))));
        lines.push(String::new());
        lines
            .iter()
            .map(|line| truncate_to_width(line, safe_width, "…", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if self.done {
            return;
        }
        if matches_key(data, "escape") || matches_key(data, "ctrl+c") || matches_key(data, "ctrl+d")
        {
            self.cancel();
            return;
        }
        if self.empty_hinted {
            self.empty_hinted = false;
        }
        if self.input.handle_input(data) == InputEvent::Submit {
            let value = self.input.get_value().to_owned();
            self.submit(&value);
        }
    }

    fn invalidate(&mut self) {}

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl Focusable for FeedbackInputDialogComponent {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    fn plain(joined: &str) -> String {
        crate::ansi::strip_ansi(joined)
    }

    #[test]
    fn renders_rounded_box() {
        set_palette(DARK_COLORS);
        let mut c = FeedbackInputDialogComponent::new();
        let lines = c.render(60);
        assert!(lines[0].is_empty());
        assert!(plain(&lines[1]).starts_with('╭'), "{}", lines[1]);
        assert!(
            plain(&lines[lines.len() - 2]).starts_with("╰"),
            "{}",
            lines[lines.len() - 2]
        );
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains("Send feedback to Dimi"), "{joined}");
        assert!(
            joined.contains("Enter to submit  ·  Esc to cancel"),
            "{joined}"
        );
    }

    #[test]
    fn submit_nonempty() {
        set_palette(DARK_COLORS);
        let mut c = FeedbackInputDialogComponent::new();
        for ch in "hello".chars() {
            c.handle_input(&ch.to_string());
        }
        c.handle_input("\r");
        assert_eq!(
            c.take_action(),
            Some(FeedbackInputDialogResult::Ok("hello".to_owned()))
        );
    }

    #[test]
    fn empty_submit_hints_then_cancel() {
        set_palette(DARK_COLORS);
        let mut c = FeedbackInputDialogComponent::new();
        c.handle_input("\r");
        assert_eq!(c.take_action(), None);
        assert!(plain(&c.render(60).join("\n")).contains("Feedback cannot be empty."));
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(FeedbackInputDialogResult::Cancel));
    }
}
