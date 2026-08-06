//! Minimal single-line input used for the inline feedback / "Other" answer
//! rows in the approval and question dialogs — a scoped port of
//! `@dimi-agent/pi-tui` `src/components/input.ts` (`Input`).
//!
//! Slice-9 scope: only the editing keys the dialogs route into it
//! (printable characters, backspace, ←/→) plus enter/escape which the parent
//! interprets; the full undo / kill-ring / bracketed-paste machinery is left
//! to the editor slice. The `render` output is byte-aligned with pi-tui's
//! `Input.render` (the `> ` prompt, the inverse-video fake cursor, and the
//! `CURSOR_MARKER` when focused).

use unicode_segmentation::UnicodeSegmentation;

use crate::component::CURSOR_MARKER;
use crate::keys::{decode_kitty_printable, matches_key};
use crate::width::{slice_by_column, visible_width};

/// Event returned by [`InputLine::handle_input`] — the parent decides what
/// submit/escape mean (mirrors pi-tui's `onSubmit` / `onEscape` callbacks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    None,
    Submit,
    Escape,
}

/// Single-line input with horizontal scrolling (port of pi-tui `Input`).
#[derive(Debug, Clone)]
pub struct InputLine {
    value: String,
    /// Cursor position in the value (byte index, kept on char boundaries).
    cursor: usize,
    focused: bool,
}

impl Default for InputLine {
    fn default() -> Self {
        Self::new()
    }
}

impl InputLine {
    pub fn new() -> Self {
        InputLine {
            value: String::new(),
            cursor: 0,
            focused: false,
        }
    }

    pub fn get_value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_owned();
        self.cursor = self.cursor.min(self.value.len());
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Basic editing for the dialog inline inputs. Returns the event the
    /// parent should react to (enter / escape); printable + navigation keys
    /// edit the buffer and return [`InputEvent::None`].
    pub fn handle_input(&mut self, data: &str) -> InputEvent {
        if matches_key(data, "enter") || data == "\n" {
            return InputEvent::Submit;
        }
        if matches_key(data, "escape") {
            return InputEvent::Escape;
        }
        if matches_key(data, "backspace") {
            if self.cursor > 0 {
                let before = &self.value[..self.cursor];
                let last = before
                    .graphemes(true)
                    .next_back()
                    .map(str::len)
                    .unwrap_or(1);
                self.value = format!(
                    "{}{}",
                    &self.value[..self.cursor - last],
                    &self.value[self.cursor..]
                );
                self.cursor -= last;
            }
            return InputEvent::None;
        }
        if matches_key(data, "left") {
            if self.cursor > 0 {
                let before = &self.value[..self.cursor];
                let last = before
                    .graphemes(true)
                    .next_back()
                    .map(str::len)
                    .unwrap_or(1);
                self.cursor -= last;
            }
            return InputEvent::None;
        }
        if matches_key(data, "right") {
            if self.cursor < self.value.len() {
                let after = &self.value[self.cursor..];
                let first = after.graphemes(true).next().map(str::len).unwrap_or(1);
                self.cursor += first;
            }
            return InputEvent::None;
        }
        if matches_key(data, "home") {
            self.cursor = 0;
            return InputEvent::None;
        }
        if matches_key(data, "end") {
            self.cursor = self.value.len();
            return InputEvent::None;
        }
        // Kitty CSI-u printable character.
        if let Some(ch) = decode_kitty_printable(data) {
            self.insert(&ch);
            return InputEvent::None;
        }
        // Regular printable characters (reject C0/C1 control chars, mirroring
        // the TS hasControlChars check).
        let has_control = data.chars().any(|c| {
            let code = c as u32;
            code < 32 || code == 0x7f || (0x80..=0x9f).contains(&code)
        });
        if !has_control {
            self.insert(data);
        }
        InputEvent::None
    }

    fn insert(&mut self, text: &str) {
        self.value = format!(
            "{}{}{}",
            &self.value[..self.cursor],
            text,
            &self.value[self.cursor..]
        );
        self.cursor += text.len();
    }

    /// Port of `Input.render`: a single line `> {value-with-cursor}` padded to
    /// `width`.
    pub fn render(&self, width: usize) -> String {
        let prompt = "> ";
        let prompt_len = prompt.len();
        let available = width.saturating_sub(prompt_len);
        if available == 0 {
            return prompt.to_owned();
        }

        let visible_text: String;
        let mut cursor_display = self.cursor;
        let total_width = visible_width(&self.value);

        if total_width < available {
            visible_text = self.value.clone();
        } else {
            // Reserve one column for cursor if it's at the end.
            let scroll_width = if self.cursor == self.value.len() {
                available - 1
            } else {
                available
            };
            let cursor_col = visible_width(&self.value[..self.cursor]);
            if scroll_width > 0 {
                let half_width = scroll_width / 2;
                let start_col = if cursor_col < half_width {
                    0
                } else if cursor_col > total_width.saturating_sub(half_width) {
                    total_width.saturating_sub(scroll_width)
                } else {
                    cursor_col.saturating_sub(half_width)
                };
                visible_text = slice_by_column(&self.value, start_col, scroll_width, true);
                let before_cursor = slice_by_column(
                    &self.value,
                    start_col,
                    cursor_col.saturating_sub(start_col),
                    true,
                );
                cursor_display = before_cursor.len();
            } else {
                visible_text = String::new();
                cursor_display = 0;
            }
        }

        // Build line with fake cursor.
        let after = &visible_text[cursor_display..];
        let cursor_grapheme = after.graphemes(true).next().unwrap_or(" ");
        let before_cursor = &visible_text[..cursor_display];
        let cursor_end = (cursor_display + cursor_grapheme.len()).min(visible_text.len());
        let after_cursor = &visible_text[cursor_end..];

        // Hardware cursor marker (zero-width) before the fake cursor for IME
        // positioning.
        let marker = if self.focused { CURSOR_MARKER } else { "" };
        let cursor_char = format!("\x1b[7m{cursor_grapheme}\x1b[27m");
        let text_with_cursor = format!("{before_cursor}{marker}{cursor_char}{after_cursor}");

        let visual_length = visible_width(&text_with_cursor);
        let padding = " ".repeat(available.saturating_sub(visual_length));
        format!("{prompt}{text_with_cursor}{padding}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_render() {
        let input = InputLine::new();
        // Always shows the inverse-video fake cursor, even when empty.
        assert_eq!(input.render(10), "> \x1b[7m \x1b[27m       ");
    }

    #[test]
    fn renders_value_with_inverse_cursor() {
        let mut input = InputLine::new();
        input.set_value("abc");
        input.cursor = input.value.len();
        let line = input.render(10);
        assert!(line.starts_with("> "), "{line}");
        assert!(line.contains("\x1b[7m \x1b[27m"), "{line}"); // cursor at end
        let stripped = crate::ansi::strip_ansi(&line);
        assert!(stripped.starts_with("> abc"), "{stripped}");
    }

    #[test]
    fn typing_and_backspace() {
        let mut input = InputLine::new();
        input.handle_input("h");
        input.handle_input("i");
        assert_eq!(input.get_value(), "hi");
        input.handle_input("\x7f"); // backspace
        assert_eq!(input.get_value(), "h");
    }

    #[test]
    fn enter_and_escape_events() {
        let mut input = InputLine::new();
        assert_eq!(input.handle_input("\r"), InputEvent::Submit);
        assert_eq!(input.handle_input("\x1b"), InputEvent::Escape);
    }

    #[test]
    fn long_value_scrolls() {
        let mut input = InputLine::new();
        input.set_value("abcdefghijklmnopqrstuvwxyz");
        input.cursor = input.value.len();
        let line = input.render(12);
        // Visible window keeps the tail + cursor.
        let stripped = crate::ansi::strip_ansi(&line);
        assert!(stripped.contains("z"), "{stripped}");
    }
}
