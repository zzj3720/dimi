//! Start-permission prompt — an option list with a notice, port of
//! `apps/dimi/src/tui/components/dialogs/start-permission-prompt.ts`
//! (`StartPermissionPromptComponent`).
//!
//! The swarm variant (`swarm_start_permission_prompt.rs`) configures this
//! component with its own title/notice/options.

use crate::component::{Component, Focusable};
use crate::keys::matches_key;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

/// `StartPermissionChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPermissionChoice {
    Auto,
    Yolo,
    Manual,
    Cancel,
}

impl StartPermissionChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            StartPermissionChoice::Auto => "auto",
            StartPermissionChoice::Yolo => "yolo",
            StartPermissionChoice::Manual => "manual",
            StartPermissionChoice::Cancel => "cancel",
        }
    }
}

/// `StartPermissionOption<T>`.
#[derive(Debug, Clone)]
pub struct StartPermissionOption<T = StartPermissionChoice> {
    pub value: T,
    pub label: String,
    pub description: String,
}

/// `StartPermissionPromptOptions<T>`.
#[derive(Debug, Clone)]
pub struct StartPermissionPromptOptions<T = StartPermissionChoice> {
    pub title: String,
    pub notice_lines: Vec<String>,
    pub options: Vec<StartPermissionOption<T>>,
}

/// `StartPermissionPromptComponent<T>`.
pub struct StartPermissionPromptComponent<T = StartPermissionChoice> {
    opts: StartPermissionPromptOptions<T>,
    selected_index: usize,
    focused: bool,
    /// Host polls this after `handle_input` (mirrors `onSelect`/`onCancel`).
    pub action: Option<StartPermissionAction<T>>,
}

/// Action a host reacts to (mirrors the TS `onSelect`/`onCancel` callbacks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPermissionAction<T> {
    Select(T),
    Cancel,
}

impl<T: Clone + PartialEq> StartPermissionPromptComponent<T> {
    pub fn new(opts: StartPermissionPromptOptions<T>) -> Self {
        StartPermissionPromptComponent {
            opts,
            selected_index: 0,
            focused: false,
            action: None,
        }
    }

    pub fn take_action(&mut self) -> Option<StartPermissionAction<T>> {
        self.action.take()
    }
}

/// `is_word_char` — `\w` in JS regex (ASCII letters, digits, underscore).
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Port of the TS `styleModeNames` split: split on whole-word boundaries of
/// `Manual`/`Auto`/`YOLO`, keeping the separator words (like a JS regex
/// `split` with a capture group). Empty leading/trailing segments are kept
/// so the styled spans line up byte-for-byte.
fn split_mode_names(text: &str) -> Vec<String> {
    const WORDS: [&str; 3] = ["Manual", "Auto", "YOLO"];
    let bytes = text.as_bytes();
    let mut boundaries: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    boundaries.push(text.len());

    let mut parts: Vec<String> = Vec::new();
    let mut last = 0usize;
    let mut idx = 0usize;
    while idx < boundaries.len() - 1 {
        let i = boundaries[idx];
        let before_ok = i == 0 || !is_word_char(bytes[i - 1]);
        let mut matched = false;
        if before_ok {
            for w in &WORDS {
                if text[i..].starts_with(w) {
                    let after_idx = i + w.len();
                    let after_ok = after_idx == text.len() || !is_word_char(bytes[after_idx]);
                    if after_ok {
                        parts.push(text[last..i].to_owned());
                        parts.push((*w).to_owned());
                        last = after_idx;
                        matched = true;
                        break;
                    }
                }
            }
        }
        if matched {
            // Jump to the boundary at `last`.
            while idx < boundaries.len() && boundaries[idx] < last {
                idx += 1;
            }
        } else {
            idx += 1;
        }
    }
    parts.push(text[last..].to_owned());
    parts
}

/// `wrapPlain` — word-wrap without ANSI awareness on the input (plain text).
fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if visible_width(&candidate) <= width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(current);
        }
        current = if visible_width(word) <= width {
            word.to_owned()
        } else {
            truncate_to_width(word, width, "…", false)
        };
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// `styleModeNames(text, baseToken)` — bold the mode names, dim/faint the rest.
fn style_mode_names(text: &str, base_token: ColorToken) -> String {
    let theme = current_theme();
    split_mode_names(text)
        .into_iter()
        .map(|part| {
            if part == "Manual" || part == "Auto" || part == "YOLO" {
                theme.bold_fg(ColorToken::TextStrong, &part)
            } else {
                theme.fg(base_token, &part)
            }
        })
        .collect()
}

impl<T: Clone> Component for StartPermissionPromptComponent<T> {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let rule = theme.fg(ColorToken::Primary, &"─".repeat(width));
        let mut lines: Vec<String> = vec![
            rule.clone(),
            theme.bold_fg(ColorToken::Primary, &format!(" {}", self.opts.title)),
            theme.fg(
                ColorToken::TextMuted,
                " ↑↓ navigate · Enter select · Esc cancel",
            ),
            String::new(),
        ];

        let text_width = width.saturating_sub(2).max(20);
        for paragraph in &self.opts.notice_lines {
            for line in wrap_plain(paragraph, text_width) {
                lines.push(format!(
                    " {}",
                    style_mode_names(&line, ColorToken::TextMuted)
                ));
            }
            lines.push(String::new());
        }

        for (i, option) in self.opts.options.iter().enumerate() {
            let selected = i == self.selected_index;
            let pointer = if selected {
                crate::dialogs::SELECT_POINTER
            } else {
                " "
            };
            let label = if selected {
                theme.bold_fg(ColorToken::Primary, &option.label)
            } else {
                style_mode_names(&option.label, ColorToken::Text)
            };
            lines.push(format!(
                "{}{}",
                theme.fg(
                    if selected {
                        ColorToken::Primary
                    } else {
                        ColorToken::TextDim
                    },
                    &format!("  {pointer} "),
                ),
                label
            ));
            for line in wrap_plain(&option.description, width.saturating_sub(4).max(20)) {
                lines.push(format!(
                    "    {}",
                    style_mode_names(&line, ColorToken::TextMuted)
                ));
            }
            lines.push(String::new());
        }

        lines.push(rule);
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "…", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") {
            self.action = Some(StartPermissionAction::Cancel);
            return;
        }
        if matches_key(data, "up") {
            self.selected_index = self.selected_index.saturating_sub(1);
            return;
        }
        if matches_key(data, "down") {
            self.selected_index =
                (self.selected_index + 1).min(self.opts.options.len().saturating_sub(1));
            return;
        }
        if matches_key(data, "enter") || matches_key(data, "space") {
            if let Some(option) = self.opts.options.get(self.selected_index) {
                self.action = Some(StartPermissionAction::Select(option.value.clone()));
            }
        }
    }

    fn invalidate(&mut self) {}

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl<T> Focusable for StartPermissionPromptComponent<T> {
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
    fn split_mode_names_keeps_words() {
        assert_eq!(
            split_mode_names("Manual mode asks you before Dimi runs"),
            vec!["", "Manual", " mode asks you before Dimi runs"]
        );
        assert_eq!(
            split_mode_names("Switch to YOLO and start"),
            vec!["Switch to ", "YOLO", " and start"]
        );
        assert_eq!(split_mode_names("no modes here"), vec!["no modes here"]);
        assert_eq!(split_mode_names(""), vec![""]);
        // Not a whole word: "Automation" must not match "Auto".
        assert_eq!(split_mode_names("Automation"), vec!["Automation"]);
        // Trailing match keeps empty tail.
        assert_eq!(split_mode_names("end Manual"), vec!["end ", "Manual", ""]);
    }

    #[test]
    fn renders_structure() {
        set_palette(DARK_COLORS);
        let mut c = StartPermissionPromptComponent::<StartPermissionChoice>::new(
            StartPermissionPromptOptions {
                title: "Start a swarm task with approvals on?".to_owned(),
                notice_lines: vec!["Manual mode asks you before Dimi runs commands.".to_owned()],
                options: vec![
                    StartPermissionOption {
                        value: StartPermissionChoice::Auto,
                        label: "Switch to Auto and start".to_owned(),
                        description: "Tools are approved automatically.".to_owned(),
                    },
                    StartPermissionOption {
                        value: StartPermissionChoice::Yolo,
                        label: "Switch to YOLO and start".to_owned(),
                        description: "Dimi may still ask you questions.".to_owned(),
                    },
                ],
            },
        );
        let lines = c.render(80);
        let joined = plain(&lines.join("\n"));
        assert!(
            joined.contains("Start a swarm task with approvals on?"),
            "{joined}"
        );
        assert!(
            joined.contains("↑↓ navigate · Enter select · Esc cancel"),
            "{joined}"
        );
        assert!(joined.contains("❯ Switch to Auto and start"), "{joined}");
        assert!(joined.contains("Switch to YOLO and start"), "{joined}");
    }

    #[test]
    fn navigation_and_select() {
        set_palette(DARK_COLORS);
        let mut c = StartPermissionPromptComponent::<StartPermissionChoice>::new(
            StartPermissionPromptOptions {
                title: "t".to_owned(),
                notice_lines: vec![],
                options: vec![
                    StartPermissionOption {
                        value: StartPermissionChoice::Auto,
                        label: "A".to_owned(),
                        description: String::new(),
                    },
                    StartPermissionOption {
                        value: StartPermissionChoice::Yolo,
                        label: "Y".to_owned(),
                        description: String::new(),
                    },
                ],
            },
        );
        c.handle_input("\x1b[B"); // down
        assert_eq!(c.selected_index, 1);
        c.handle_input("\r");
        assert_eq!(
            c.take_action(),
            Some(StartPermissionAction::Select(StartPermissionChoice::Yolo))
        );
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(StartPermissionAction::Cancel));
    }
}
