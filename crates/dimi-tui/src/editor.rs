//! `Editor` — multi-line text editor component with word wrapping, cursor,
//! undo, kill-ring, and keybindings (port of `@dimi-agent/pi-tui`
//! `src/components/editor.ts`, slice 4 scope: text editing + rendering +
//! key handling; autocomplete / paste-burst / mentions land with the
//! dialogs slice).

use crate::component::{CURSOR_MARKER, Component};
use crate::keys::{decode_printable_key, matches_key};
use crate::width::visible_width;
use unicode_segmentation::UnicodeSegmentation;

/// One word-wrapped chunk with its span in the original line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub text: String,
    pub start_index: usize,
    pub end_index: usize,
}

/// A laid-out visual line.
#[derive(Debug, Clone)]
pub struct LayoutLine {
    pub text: String,
    pub has_cursor: bool,
    pub cursor_pos: Option<usize>,
}

/// Editor buffer state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    pub lines: Vec<String>,
    pub cursor_line: usize,
    pub cursor_col: usize,
}

impl EditorState {
    pub fn new() -> Self {
        EditorState {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
        }
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

/// Undo stack — snapshots of the editor state.
#[derive(Debug, Default)]
pub struct UndoStack {
    stack: Vec<EditorState>,
}

impl UndoStack {
    pub fn push(&mut self, state: EditorState) {
        self.stack.push(state);
        if self.stack.len() > 100 {
            self.stack.remove(0);
        }
    }
    pub fn pop(&mut self) -> Option<EditorState> {
        self.stack.pop()
    }
    pub fn clear(&mut self) {
        self.stack.clear();
    }
    pub fn len(&self) -> usize {
        self.stack.len()
    }
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

/// Kill ring for Emacs-style kill/yank.
#[derive(Debug, Default)]
pub struct KillRing {
    entries: Vec<String>,
    index: usize,
}

impl KillRing {
    pub fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.entries.push(text.to_owned());
        if self.entries.len() > 60 {
            self.entries.remove(0);
        }
        self.index = self.entries.len();
    }
    /// Most recent kill; `rotate` moves to older entries (yank-pop).
    pub fn pop(&mut self, rotate: bool) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        if rotate {
            self.index = self.index.saturating_sub(1);
            if self.index == 0 {
                self.index = self.entries.len();
            }
        }
        let idx = (self.index.saturating_sub(1)) % self.entries.len();
        self.entries.get(idx).cloned()
    }
}

/// Check whether a grapheme is CJK (breaks between adjacent CJK chars).
fn is_cjk_break_char(segment: &str) -> bool {
    let Some(cp) = segment.chars().next().map(|c| c as u32) else {
        return false;
    };
    (0x3400..=0x4dbf).contains(&cp)
        || (0x4e00..=0x9fff).contains(&cp)
        || (0x3040..=0x309f).contains(&cp)
        || (0x30a0..=0x30ff).contains(&cp)
        || (0xac00..=0xd7af).contains(&cp)
        || (0x3100..=0x312f).contains(&cp)
}

fn is_whitespace_char(s: &str) -> bool {
    s.chars().all(|c| c.is_whitespace())
}

/// Split a line into word-wrapped chunks (pi-tui `wordWrapLine`).
pub fn word_wrap_line(line: &str, max_width: usize) -> Vec<TextChunk> {
    if line.is_empty() || max_width == 0 {
        return vec![TextChunk {
            text: String::new(),
            start_index: 0,
            end_index: 0,
        }];
    }
    let line_width = visible_width(line);
    if line_width <= max_width {
        return vec![TextChunk {
            text: line.to_owned(),
            start_index: 0,
            end_index: line.len(),
        }];
    }

    let mut chunks: Vec<TextChunk> = Vec::new();
    let segments: Vec<(usize, String)> = line
        .grapheme_indices(true)
        .map(|(i, g)| (i, g.to_owned()))
        .collect();

    let mut current_width = 0usize;
    let mut chunk_start = 0usize;
    let mut wrap_opp_index: isize = -1;
    let mut wrap_opp_width = 0usize;

    for (idx, (char_index, grapheme)) in segments.iter().enumerate() {
        let g_width = visible_width(grapheme);
        let is_ws = is_whitespace_char(grapheme);

        // Overflow check before advancing.
        if current_width + g_width > max_width {
            if wrap_opp_index >= 0 && current_width - wrap_opp_width + g_width <= max_width {
                let woi = wrap_opp_index as usize;
                chunks.push(TextChunk {
                    text: line[chunk_start..woi].to_owned(),
                    start_index: chunk_start,
                    end_index: woi,
                });
                chunk_start = woi;
                current_width -= wrap_opp_width;
            } else if chunk_start < *char_index {
                chunks.push(TextChunk {
                    text: line[chunk_start..*char_index].to_owned(),
                    start_index: chunk_start,
                    end_index: *char_index,
                });
                chunk_start = *char_index;
                current_width = 0;
            }
            wrap_opp_index = -1;
        }

        if g_width > max_width {
            // Indivisible grapheme wider than maxWidth: keep as open chunk
            // (the paint layer truncates overwide lines).
            current_width = g_width;
            wrap_opp_index = -1;
            continue;
        }

        // Advance.
        current_width += g_width;

        // Record wrap opportunity.
        let next = segments.get(idx + 1);
        if is_ws {
            if let Some((next_idx, next_g)) = next {
                if !is_whitespace_char(next_g) {
                    wrap_opp_index = *next_idx as isize;
                    wrap_opp_width = current_width;
                }
            }
        } else if let Some((next_idx, next_g)) = next {
            if !is_whitespace_char(next_g) {
                let is_cjk = is_cjk_break_char(grapheme);
                let next_is_cjk = is_cjk_break_char(next_g);
                if is_cjk || next_is_cjk {
                    wrap_opp_index = *next_idx as isize;
                    wrap_opp_width = current_width;
                }
            }
        }
    }

    chunks.push(TextChunk {
        text: line[chunk_start..].to_owned(),
        start_index: chunk_start,
        end_index: line.len(),
    });
    chunks
}

/// Text callback type.
pub type Callback = dyn FnMut(&str);

/// Editor options.
#[derive(Debug, Clone, Default)]
pub struct EditorOptions {
    pub padding_x: usize,
}

/// Theme hooks for the editor border.
pub trait EditorTheme {
    fn border_color(&self, text: &str) -> String;
}

/// The editor component.
pub struct Editor {
    state: EditorState,
    focused: bool,
    padding_x: usize,
    last_width: usize,
    scroll_offset: usize,
    undo_stack: UndoStack,
    kill_ring: KillRing,
    last_action: Option<&'static str>, // "kill" | "yank" | "type-word"
    history: Vec<String>,
    history_index: isize,
    history_draft: Option<EditorState>,
    /// Submitted text callback.
    pub on_submit: Option<Box<Callback>>,
    /// Text change callback.
    pub on_change: Option<Box<Callback>>,
}

impl Editor {
    pub fn new(options: EditorOptions) -> Self {
        let padding_x = options.padding_x;
        Editor {
            state: EditorState::new(),
            focused: false,
            padding_x,
            last_width: 80,
            scroll_offset: 0,
            undo_stack: UndoStack::default(),
            kill_ring: KillRing::default(),
            last_action: None,
            history: Vec::new(),
            history_index: -1,
            history_draft: None,
            on_submit: None,
            on_change: None,
        }
    }

    // ── public accessors ─────────────────────────────────────────────────

    pub fn get_state(&self) -> &EditorState {
        &self.state
    }

    pub fn get_text(&self) -> String {
        self.state.lines.join("\n")
    }

    pub fn set_text(&mut self, text: &str) {
        self.set_text_internal(text, true);
    }

    /// Set text without resetting history browsing state
    /// (TS `setTextInternal`, used by navigateHistory).
    fn set_text_internal(&mut self, text: &str, reset_history: bool) {
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        self.state.lines = lines;
        self.state.cursor_line = self.state.lines.len() - 1;
        self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
        self.scroll_offset = 0;
        if reset_history {
            self.history_index = -1;
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn add_to_history(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.history.first().is_some_and(|h| h == trimmed) {
            return;
        }
        self.history.insert(0, trimmed.to_owned());
        if self.history.len() > 100 {
            self.history.pop();
        }
    }

    // ── text operations ───────────────────────────────────────────────────

    fn push_undo_snapshot(&mut self) {
        self.undo_stack.push(self.state.clone());
    }

    fn insert_character(&mut self, ch: &str) {
        self.push_undo_snapshot();
        let line = &mut self.state.lines[self.state.cursor_line];
        let col = self.state.cursor_col.min(line.len());
        // Insert at a char boundary.
        let byte_idx = char_boundary(line, col);
        line.insert_str(byte_idx, ch);
        self.state.cursor_col = col + ch.len();
        self.on_change_after_edit();
    }

    fn handle_backspace(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        if self.state.cursor_col > 0 {
            self.push_undo_snapshot();
            let col = self.state.cursor_col;
            let byte_idx = char_boundary(&line, col);
            let prev_idx = prev_char_boundary(&line, byte_idx);
            self.state.lines[self.state.cursor_line].replace_range(prev_idx..byte_idx, "");
            self.state.cursor_col =
                prev_char_boundary(&self.state.lines[self.state.cursor_line], prev_idx);
            // Recompute as count of chars before prev_idx.
            self.state.cursor_col = self.state.lines[self.state.cursor_line][..prev_idx]
                .graphemes(true)
                .count();
            self.on_change_after_edit();
            return;
        }
        // At start of line: merge with previous line.
        if self.state.cursor_line > 0 {
            self.push_undo_snapshot();
            let prev_len = self.state.lines[self.state.cursor_line - 1].len();
            let current = self.state.lines.remove(self.state.cursor_line);
            self.state.lines[self.state.cursor_line - 1].push_str(&current);
            self.state.cursor_line -= 1;
            self.state.cursor_col = prev_len;
            self.on_change_after_edit();
        }
    }

    fn handle_forward_delete(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        if self.state.cursor_col < line.len() {
            self.push_undo_snapshot();
            let byte_idx = char_boundary(&line, self.state.cursor_col);
            let next_idx = next_char_boundary(&line, byte_idx);
            self.state.lines[self.state.cursor_line].replace_range(byte_idx..next_idx, "");
            self.on_change_after_edit();
            return;
        }
        if self.state.cursor_line + 1 < self.state.lines.len() {
            self.push_undo_snapshot();
            let next = self.state.lines.remove(self.state.cursor_line + 1);
            self.state.lines[self.state.cursor_line].push_str(&next);
            self.on_change_after_edit();
        }
    }

    fn add_new_line(&mut self) {
        self.push_undo_snapshot();
        let line = self.state.lines[self.state.cursor_line].clone();
        let byte_idx = char_boundary(&line, self.state.cursor_col);
        let tail = line[byte_idx..].to_owned();
        self.state.lines[self.state.cursor_line].truncate(byte_idx);
        self.state.cursor_line += 1;
        self.state.lines.insert(self.state.cursor_line, tail);
        self.state.cursor_col = 0;
        self.on_change_after_edit();
    }

    fn delete_to_end_of_line(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        if self.state.cursor_col < line.len() {
            self.push_undo_snapshot();
            let byte_idx = char_boundary(&line, self.state.cursor_col);
            self.state.lines[self.state.cursor_line].truncate(byte_idx);
            self.on_change_after_edit();
        }
    }

    fn delete_to_start_of_line(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        if self.state.cursor_col > 0 {
            self.push_undo_snapshot();
            let byte_idx = char_boundary(&line, self.state.cursor_col);
            self.state.lines[self.state.cursor_line].replace_range(..byte_idx, "");
            self.state.cursor_col = 0;
            self.on_change_after_edit();
        }
    }

    fn delete_word_backwards(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        let byte_idx = char_boundary(&line, self.state.cursor_col);
        let start = find_word_backward(&line, byte_idx);
        if start < byte_idx {
            self.push_undo_snapshot();
            self.kill_ring.push(&line[start..byte_idx]);
            self.state.lines[self.state.cursor_line].replace_range(start..byte_idx, "");
            self.state.cursor_col = self.state.lines[self.state.cursor_line][..start]
                .graphemes(true)
                .count();
            self.last_action = Some("kill");
            self.on_change_after_edit();
        }
    }

    fn delete_word_forward(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        let byte_idx = char_boundary(&line, self.state.cursor_col);
        let end = find_word_forward(&line, byte_idx);
        if end > byte_idx {
            self.push_undo_snapshot();
            self.kill_ring.push(&line[byte_idx..end]);
            self.state.lines[self.state.cursor_line].replace_range(byte_idx..end, "");
            self.last_action = Some("kill");
            self.on_change_after_edit();
        }
    }

    fn yank(&mut self) {
        if let Some(text) = self.kill_ring.pop(false) {
            self.push_undo_snapshot();
            self.insert_character(&text);
            self.last_action = Some("yank");
        }
    }

    fn yank_pop(&mut self) {
        if self.last_action != Some("yank") {
            return;
        }
        if let Some(text) = self.kill_ring.pop(true) {
            // Remove the previously yanked text (approximate: delete the last
            // insertion of the same length from the cursor).
            let line = &self.state.lines[self.state.cursor_line];
            let col = self.state.cursor_col;
            let byte_idx = char_boundary(line, col);
            let prev_idx = prev_char_boundary(line, byte_idx);
            self.state.lines[self.state.cursor_line].replace_range(prev_idx..byte_idx, "");
            self.state.cursor_col = self.state.lines[self.state.cursor_line][..prev_idx]
                .graphemes(true)
                .count();
            self.insert_character(&text);
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.state = prev;
            self.scroll_offset = 0;
            self.on_change_after_edit();
        }
    }

    // ── cursor movement ───────────────────────────────────────────────────

    fn move_cursor(&mut self, delta_line: isize, delta_col: isize) {
        let line_count = self.state.lines.len() as isize;
        let mut new_line = self.state.cursor_line as isize + delta_line;
        new_line = new_line.max(0).min(line_count - 1);
        self.state.cursor_line = new_line as usize;

        let line = &self.state.lines[self.state.cursor_line];
        let line_len = line.graphemes(true).count() as isize;
        let mut new_col = self.state.cursor_col as isize + delta_col;
        new_col = new_col.max(0).min(line_len);
        self.state.cursor_col = new_col as usize;
    }

    fn move_to_line_start(&mut self) {
        self.state.cursor_col = 0;
    }

    fn move_to_line_end(&mut self) {
        self.state.cursor_col = self.state.lines[self.state.cursor_line]
            .graphemes(true)
            .count();
    }

    fn move_word_backwards(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        let byte_idx = char_boundary(&line, self.state.cursor_col);
        let start = find_word_backward(&line, byte_idx);
        self.state.cursor_col = line[..start].graphemes(true).count();
    }

    fn move_word_forwards(&mut self) {
        let line = self.state.lines[self.state.cursor_line].clone();
        let byte_idx = char_boundary(&line, self.state.cursor_col);
        let end = find_word_forward(&line, byte_idx);
        self.state.cursor_col = line[..end].graphemes(true).count();
    }

    fn navigate_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index == -1 {
            // Save the current draft before browsing.
            self.history_draft = Some(self.state.clone());
        }
        // TS navigateHistory: `newIndex = newIndex - direction` — up (-1)
        // increases the index (older entries), down (1) decreases it.
        let mut new_index = self.history_index - direction;
        new_index = new_index.max(-1).min(self.history.len() as isize - 1);
        self.history_index = new_index;
        if new_index < 0 {
            // Exit history browsing and restore the saved draft.
            if let Some(draft) = self.history_draft.take() {
                self.state = draft;
                self.scroll_offset = 0;
            }
            return;
        }
        let entry = self.history[new_index as usize].clone();
        self.set_text_internal(&entry, false);
    }

    // ── layout & render ───────────────────────────────────────────────────

    fn layout_text(&self, width: usize) -> Vec<LayoutLine> {
        let mut layout: Vec<LayoutLine> = Vec::new();
        if self.state.lines.is_empty()
            || (self.state.lines.len() == 1 && self.state.lines[0].is_empty())
        {
            layout.push(LayoutLine {
                text: String::new(),
                has_cursor: self.state.cursor_line == 0,
                cursor_pos: Some(0),
            });
            return layout;
        }
        let mut found_cursor = false;
        for (li, line) in self.state.lines.iter().enumerate() {
            let chunks = word_wrap_line(line, width);
            for (ci, chunk) in chunks.iter().enumerate() {
                let has_cursor = !found_cursor && li == self.state.cursor_line;
                let mut cursor_pos = None;
                if has_cursor {
                    let col_chars = self.state.cursor_col.min(chunk.end_index);
                    // Cursor column within this chunk (in chars of the chunk).
                    let within = if col_chars >= chunk.start_index {
                        col_chars.saturating_sub(chunk.start_index)
                    } else {
                        0
                    };
                    cursor_pos = Some(within);
                    found_cursor = true;
                }
                layout.push(LayoutLine {
                    text: chunk.text.clone(),
                    has_cursor,
                    cursor_pos,
                });
                let _ = ci;
            }
        }
        if !found_cursor {
            layout.push(LayoutLine {
                text: String::new(),
                has_cursor: true,
                cursor_pos: Some(0),
            });
        }
        layout
    }

    fn render_inner(&mut self, width: usize) -> Vec<String> {
        let max_padding = width.saturating_sub(1) / 2;
        let padding_x = self.padding_x.min(max_padding);
        let content_width = (width.saturating_sub(padding_x * 2)).max(1);
        let layout_width = (content_width.saturating_sub(if padding_x > 0 { 0 } else { 1 })).max(1);
        self.last_width = layout_width;

        let horizontal = "─".repeat(width);
        let layout_lines = self.layout_text(layout_width);

        // Max visible lines: 30% of terminal height, min 5.
        let terminal_rows = 24usize; // TUI provides this; overridden by render(width, rows).
        let max_visible_lines = 5.max(terminal_rows * 3 / 10);

        // Cursor line index in layout.
        let cursor_line_index = layout_lines.iter().position(|l| l.has_cursor).unwrap_or(0);

        // Adjust scroll offset.
        if cursor_line_index < self.scroll_offset {
            self.scroll_offset = cursor_line_index;
        } else if cursor_line_index >= self.scroll_offset + max_visible_lines {
            self.scroll_offset = cursor_line_index - max_visible_lines + 1;
        }
        let max_scroll = layout_lines.len().saturating_sub(max_visible_lines);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        let visible_lines: Vec<LayoutLine> = layout_lines
            .iter()
            .skip(self.scroll_offset)
            .take(max_visible_lines)
            .cloned()
            .collect();

        let mut result: Vec<String> = Vec::new();
        let left_padding = " ".repeat(padding_x);
        let right_padding = left_padding.clone();

        // Top border with scroll indicator.
        if self.scroll_offset > 0 {
            let indicator = format!("─── ↑ {} more ", self.scroll_offset);
            let indicator_width = visible_width(&indicator);
            let remaining = width as isize - indicator_width as isize;
            if remaining >= 0 {
                result.push(format!("{indicator}{}", "─".repeat(remaining as usize)));
            } else {
                result.push(crate::wrap::truncate_to_width(
                    &indicator, width, "…", false,
                ));
            }
        } else {
            result.push(horizontal.clone());
        }

        let emit_cursor_marker = self.focused;

        for layout_line in &visible_lines {
            let mut display_text = layout_line.text.clone();
            let mut line_visible_width = visible_width(&layout_line.text);
            let mut cursor_in_padding = false;

            if layout_line.has_cursor {
                if let Some(cursor_pos) = layout_line.cursor_pos {
                    let before =
                        display_text[..char_boundary(&display_text, cursor_pos)].to_owned();
                    let after = display_text[char_boundary(&display_text, cursor_pos)..].to_owned();
                    let marker = if emit_cursor_marker {
                        CURSOR_MARKER
                    } else {
                        ""
                    };

                    if !after.is_empty() {
                        let first_grapheme: String =
                            after.graphemes(true).next().unwrap_or_default().to_owned();
                        let rest_after = &after[first_grapheme.len()..];
                        display_text =
                            format!("{before}{marker}\x1b[7m{first_grapheme}\x1b[0m{rest_after}");
                    } else {
                        display_text = format!("{before}{marker}\x1b[7m \x1b[0m");
                        line_visible_width += 1;
                        if line_visible_width > content_width && padding_x > 0 {
                            cursor_in_padding = true;
                        }
                    }
                }
            }

            let padding = " ".repeat(content_width.saturating_sub(line_visible_width));
            let line_right_padding = if cursor_in_padding {
                right_padding[1..].to_owned()
            } else {
                right_padding.clone()
            };
            result.push(format!(
                "{left_padding}{display_text}{padding}{line_right_padding}"
            ));
        }

        // Bottom border with scroll indicator.
        let lines_below = layout_lines
            .len()
            .saturating_sub(self.scroll_offset + visible_lines.len());
        if lines_below > 0 {
            let indicator = format!("─── ↓ {lines_below} more ");
            let indicator_width = visible_width(&indicator);
            let remaining = width as isize - indicator_width as isize;
            if remaining >= 0 {
                result.push(format!(
                    "{indicator}{}",
                    "─".repeat(remaining.max(0) as usize)
                ));
            } else {
                result.push(crate::wrap::truncate_to_width(
                    &indicator, width, "…", false,
                ));
            }
        } else {
            result.push(horizontal);
        }

        result
    }

    fn on_change_after_edit(&mut self) {
        let text = self.get_text();
        if let Some(cb) = &mut self.on_change {
            cb(&text);
        }
    }

    fn submit_value(&mut self) {
        let text = self.get_text();
        self.add_to_history(&text);
        if let Some(cb) = &mut self.on_submit {
            cb(&text);
        }
    }
}

impl Component for Editor {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.render_inner(width)
    }

    fn handle_input(&mut self, data: &str) {
        // Bracketed paste.
        if data.contains("\x1b[200~") {
            let content = data.replace("\x1b[200~", "").replace("\x1b[201~", "");
            if !content.is_empty() {
                self.push_undo_snapshot();
                self.insert_character(&content);
            }
            return;
        }

        // Undo.
        if matches_key(data, "ctrl+-") {
            self.undo();
            return;
        }

        // Copy (Ctrl+C) — let the parent handle exit/clear.
        if matches_key(data, "ctrl+c") {
            return;
        }

        // Deletions.
        if matches_key(data, "ctrl+k") {
            self.delete_to_end_of_line();
            return;
        }
        if matches_key(data, "ctrl+u") {
            self.delete_to_start_of_line();
            return;
        }
        if matches_key(data, "ctrl+w") || matches_key(data, "alt+backspace") {
            self.delete_word_backwards();
            return;
        }
        if matches_key(data, "alt+d") || matches_key(data, "alt+delete") {
            self.delete_word_forward();
            return;
        }
        if matches_key(data, "backspace") || matches_key(data, "shift+backspace") {
            self.handle_backspace();
            return;
        }
        if matches_key(data, "delete")
            || matches_key(data, "shift+delete")
            || matches_key(data, "ctrl+d")
        {
            self.handle_forward_delete();
            return;
        }

        // Yank.
        if matches_key(data, "ctrl+y") {
            self.yank();
            return;
        }
        if matches_key(data, "alt+y") {
            self.yank_pop();
            return;
        }

        // Cursor movement.
        if matches_key(data, "ctrl+a") || matches_key(data, "home") {
            self.move_to_line_start();
            return;
        }
        if matches_key(data, "ctrl+e") || matches_key(data, "end") {
            self.move_to_line_end();
            return;
        }
        if matches_key(data, "alt+b")
            || matches_key(data, "alt+left")
            || matches_key(data, "ctrl+left")
        {
            self.move_word_backwards();
            return;
        }
        if matches_key(data, "alt+f")
            || matches_key(data, "alt+right")
            || matches_key(data, "ctrl+right")
        {
            self.move_word_forwards();
            return;
        }

        // New line (shift+enter / ctrl+j).
        if matches_key(data, "shift+enter") || matches_key(data, "ctrl+j") || data == "\n" {
            self.add_new_line();
            return;
        }

        // Submit (enter).
        if matches_key(data, "enter") || data == "\r" {
            self.submit_value();
            return;
        }

        // Arrow keys with history.
        if matches_key(data, "up") {
            if self.state.cursor_line == 0
                && (self.history_index > -1
                    || self.state.cursor_col == 0
                    || self.get_text().is_empty())
            {
                self.navigate_history(-1);
            } else if self.state.cursor_line == 0 {
                self.move_to_line_start();
            } else {
                self.move_cursor(-1, 0);
            }
            return;
        }
        if matches_key(data, "down") {
            if self.history_index > -1 && self.state.cursor_line + 1 >= self.state.lines.len() {
                self.navigate_history(1);
            } else if self.state.cursor_line + 1 >= self.state.lines.len() {
                self.move_to_line_end();
            } else {
                self.move_cursor(1, 0);
            }
            return;
        }
        if matches_key(data, "right") || matches_key(data, "ctrl+f") {
            self.move_cursor(0, 1);
            return;
        }
        if matches_key(data, "left") || matches_key(data, "ctrl+b") {
            self.move_cursor(0, -1);
            return;
        }

        // Printable characters.
        if let Some(printable) = decode_printable_key(data) {
            self.insert_character(&printable);
            return;
        }
        if !data.is_empty() && data.chars().next().is_some_and(|c| c as u32 >= 32) {
            self.insert_character(data);
        }
    }

    fn invalidate(&mut self) {}
}

impl crate::component::Focusable for Editor {
    fn focused(&self) -> bool {
        self.focused
    }
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

/// Find the byte index of the previous word boundary (pi-tui findWordBackward).
fn find_word_backward(line: &str, from: usize) -> usize {
    let before = &line[..from];
    let chars: Vec<(usize, &str)> = before.grapheme_indices(true).collect();
    if chars.is_empty() {
        return 0;
    }
    let mut i = chars.len();
    // Skip trailing whitespace.
    while i > 0 && is_whitespace_char(chars[i - 1].1) {
        i -= 1;
    }
    // Skip word chars.
    while i > 0 && !is_whitespace_char(chars[i - 1].1) {
        i -= 1;
    }
    if i == 0 {
        0
    } else {
        chars[i - 1].0 + chars[i - 1].1.len()
    }
}

/// Find the byte index of the next word boundary (pi-tui findWordForward).
fn find_word_forward(line: &str, from: usize) -> usize {
    let after = &line[from..];
    let chars: Vec<(usize, &str)> = after.grapheme_indices(true).collect();
    if chars.is_empty() {
        return from;
    }
    let mut i = 0;
    // Skip word chars.
    while i < chars.len() && !is_whitespace_char(chars[i].1) {
        i += 1;
    }
    // Skip whitespace.
    while i < chars.len() && is_whitespace_char(chars[i].1) {
        i += 1;
    }
    from + chars[i - 1].0 + chars[i - 1].1.len()
}

/// Byte index of the char boundary at (or before) `col` graphemes.
fn char_boundary(line: &str, col: usize) -> usize {
    let graphemes: Vec<usize> = line.grapheme_indices(true).map(|(i, _)| i).collect();
    if col >= graphemes.len() {
        return line.len();
    }
    graphemes[col]
}

fn prev_char_boundary(line: &str, byte_idx: usize) -> usize {
    char_boundary(line, byte_idx.saturating_sub(1))
}

fn next_char_boundary(line: &str, byte_idx: usize) -> usize {
    if byte_idx >= line.len() {
        return line.len();
    }
    let rest = &line[byte_idx..];
    byte_idx + rest.graphemes(true).next().map(|g| g.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;

    fn ed() -> Editor {
        Editor::new(EditorOptions { padding_x: 0 })
    }

    fn type_text(e: &mut Editor, text: &str) {
        for ch in text.chars() {
            e.handle_input(&ch.to_string());
        }
    }

    #[test]
    fn insert_and_text() {
        let mut e = ed();
        type_text(&mut e, "hello");
        assert_eq!(e.get_text(), "hello");
        assert_eq!(e.state.cursor_col, 5);
    }

    #[test]
    fn backspace_removes_char() {
        let mut e = ed();
        type_text(&mut e, "hello");
        e.handle_input("\x7f");
        assert_eq!(e.get_text(), "hell");
        assert_eq!(e.state.cursor_col, 4);
    }

    #[test]
    fn ctrl_a_e_move_cursor() {
        let mut e = ed();
        type_text(&mut e, "hello");
        e.handle_input("\x01");
        assert_eq!(e.state.cursor_col, 0);
        e.handle_input("\x05");
        assert_eq!(e.state.cursor_col, 5);
    }

    #[test]
    fn ctrl_k_delete_to_line_end() {
        let mut e = ed();
        type_text(&mut e, "hello world");
        e.handle_input("\x01");
        e.handle_input("\x0b");
        assert_eq!(e.get_text(), "");
    }

    #[test]
    fn ctrl_u_delete_to_line_start() {
        let mut e = ed();
        type_text(&mut e, "hello world");
        e.handle_input("\x15");
        assert_eq!(e.get_text(), "");
        assert_eq!(e.state.cursor_col, 0);
    }

    #[test]
    fn ctrl_w_delete_word_backward() {
        let mut e = ed();
        type_text(&mut e, "hello world");
        e.handle_input("\x17");
        assert_eq!(e.get_text(), "hello ");
    }

    #[test]
    fn undo_restores() {
        let mut e = ed();
        type_text(&mut e, "hello");
        e.handle_input("\x1f");
        assert_eq!(e.get_text(), "hell");
        e.handle_input("\x1f");
        assert_eq!(e.get_text(), "hel");
    }

    #[test]
    fn arrow_left_right() {
        let mut e = ed();
        type_text(&mut e, "abc");
        e.handle_input("\x1b[D");
        assert_eq!(e.state.cursor_col, 2);
        e.handle_input("\x1b[C");
        assert_eq!(e.state.cursor_col, 3);
        e.handle_input("\x02");
        assert_eq!(e.state.cursor_col, 2);
        e.handle_input("\x06");
        assert_eq!(e.state.cursor_col, 3);
    }

    #[test]
    fn newline_splits() {
        let mut e = ed();
        type_text(&mut e, "ab");
        e.handle_input("\x01");
        e.handle_input("\r"); // submit; then type more
        // After submit the text is unchanged (submit fires callback).
        e.handle_input("\n");
        assert_eq!(e.get_text(), "\nab");
        assert_eq!(e.state.lines.len(), 2);
    }

    #[test]
    fn enter_submits() {
        let mut e = ed();
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        {
            let submitted_ref = submitted.clone();
            e.on_submit = Some(Box::new(move |t| {
                submitted_ref.lock().unwrap().push_str(t);
            }));
        }
        type_text(&mut e, "hi");
        e.handle_input("\r");
        assert_eq!(submitted.lock().unwrap().as_str(), "hi");
        // Enter adds to history.
        assert_eq!(e.history.first().map(|s| s.as_str()), Some("hi"));
    }

    #[test]
    fn history_navigation() {
        let mut e = ed();
        e.add_to_history("first");
        e.add_to_history("second");
        e.handle_input("\x1b[A");
        assert_eq!(e.get_text(), "second");
        e.handle_input("\x1b[A");
        assert_eq!(e.get_text(), "first");
    }

    #[test]
    fn render_single_line_with_cursor() {
        let mut e = ed();
        e.set_focused(true);
        type_text(&mut e, "hi");
        let lines = Component::render(&mut e, 20);
        assert_eq!(lines.len(), 3); // top border + content + bottom border
        assert!(lines[1].contains("hi"));
        // Focused editor emits the cursor marker.
        assert!(lines[1].contains(CURSOR_MARKER));
    }

    #[test]
    fn render_unfocused_no_marker() {
        let mut e = ed();
        type_text(&mut e, "hi");
        let lines = Component::render(&mut e, 20);
        assert!(!lines[1].contains(CURSOR_MARKER));
    }

    #[test]
    fn render_wraps_long_line() {
        let mut e = ed();
        e.set_focused(true);
        type_text(&mut e, "the quick brown fox jumps over the lazy dog");
        let lines = Component::render(&mut e, 20);
        // Content wraps to multiple rows.
        assert!(
            lines.len() >= 4,
            "expected wrapped content, got {}",
            lines.len()
        );
    }

    #[test]
    fn render_scroll_indicator() {
        let mut e = ed();
        e.set_focused(true);
        type_text(
            &mut e,
            "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10",
        );
        e.handle_input("\x01");
        let lines = Component::render(&mut e, 20);
        let joined = lines.join("\n");
        assert!(
            joined.contains("↑") || joined.contains("↓"),
            "expected scroll indicator: {joined}"
        );
    }

    #[test]
    fn paste_inserts() {
        let mut e = ed();
        e.handle_input("\x1b[200~pasted text\x1b[201~");
        assert_eq!(e.get_text(), "pasted text");
    }

    #[test]
    fn word_wrap_basic() {
        let chunks = word_wrap_line("hello world foo", 10);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "hello ");
        assert_eq!(chunks[1].text, "world foo");
    }

    #[test]
    fn word_wrap_cjk() {
        let chunks = word_wrap_line("你好世界", 2);
        // Each CJK char is exactly maxWidth, so each becomes its own chunk.
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].text, "你");
        assert_eq!(chunks[1].text, "好");
        assert_eq!(chunks[2].text, "世");
        assert_eq!(chunks[3].text, "界");
    }

    #[test]
    fn delete_char_forward() {
        let mut e = ed();
        type_text(&mut e, "abc");
        e.handle_input("\x01");
        e.handle_input("\x1b[3~");
        assert_eq!(e.get_text(), "bc");
        assert_eq!(e.state.cursor_col, 0);
    }

    #[test]
    fn ctrl_d_at_start_deletes_forward() {
        let mut e = ed();
        type_text(&mut e, "abc");
        e.handle_input("\x01");
        e.handle_input("\x04");
        assert_eq!(e.get_text(), "bc");
    }
}
