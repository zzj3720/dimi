//! Differential rendering core — ported from `@dimi-agent/pi-tui` `src/tui.ts`
//! (`TUI` class, `doRender`).
//!
//! The output sequence semantics are kept byte-aligned with the TS reference:
//! synchronized output (`ESC[?2026h/l`), `ESC[2K` line clears, trailing
//! segment reset (`ESC[0m ESC]8;; BEL`), cursor-marker extraction, viewport
//! tracking, and full redraw triggers (first frame / width change / height
//! change / clear-on-shrink / changed-above-viewport). Kitty image handling
//! and overlay compositing land in later slices and are deliberately absent
//! here.

use crate::ansi::{SEGMENT_RESET, normalize_terminal_output};
use crate::component::{CURSOR_MARKER, Component};
use crate::terminal::Terminal;
use crate::width::{ascii_visible_width, slice_by_column, visible_width};

/// Minimum render interval (ms) — reserved for the async scheduler slice.
pub const MIN_RENDER_INTERVAL_MS: u64 = 16;

/// Cursor position extracted from rendered lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub col: usize,
}

/// The differential rendering engine. Owns the component tree and the frame
/// state; writes output through a [`Terminal`].
pub struct Tui {
    children: Vec<Box<dyn Component>>,
    focused: Option<usize>,
    terminal: Box<dyn Terminal>,

    previous_lines: Vec<String>,
    previous_raw_lines: Vec<String>,
    previous_width: usize,
    previous_height: usize,
    cursor_row: isize,
    hardware_cursor_row: isize,
    show_hardware_cursor: bool,
    clear_on_shrink: bool,
    max_lines_rendered: usize,
    previous_viewport_top: isize,
    full_redraw_count: usize,
    stopped: bool,
}

impl Tui {
    pub fn new(terminal: Box<dyn Terminal>) -> Self {
        Tui {
            children: Vec::new(),
            focused: None,
            terminal,
            previous_lines: Vec::new(),
            previous_raw_lines: Vec::new(),
            previous_width: 0,
            previous_height: 0,
            cursor_row: 0,
            hardware_cursor_row: 0,
            show_hardware_cursor: false,
            clear_on_shrink: false,
            max_lines_rendered: 0,
            previous_viewport_top: 0,
            full_redraw_count: 0,
            stopped: false,
        }
    }

    pub fn add_child(&mut self, component: Box<dyn Component>) {
        self.children.push(component);
    }

    /// Set the focused child (by index). Focus drives hardware-cursor marker
    /// emission and input routing.
    pub fn set_focus(&mut self, index: Option<usize>) {
        // Unfocus the previous child.
        if let Some(prev) = self.focused {
            if let Some(child) = self.children.get_mut(prev) {
                if let Some(f) = child.as_focusable_mut() {
                    f.set_focused(false);
                }
            }
        }
        self.focused = index;
        if let Some(idx) = index {
            if let Some(child) = self.children.get_mut(idx) {
                if let Some(f) = child.as_focusable_mut() {
                    f.set_focused(true);
                }
            }
        }
        self.request_render();
    }

    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// Route raw input to the focused child's `handle_input`.
    pub fn handle_input(&mut self, data: &str) {
        if let Some(idx) = self.focused {
            if let Some(child) = self.children.get_mut(idx) {
                child.handle_input(data);
            }
        }
    }

    pub fn clear(&mut self) {
        self.children.clear();
    }

    pub fn full_redraws(&self) -> usize {
        self.full_redraw_count
    }

    pub fn set_clear_on_shrink(&mut self, enabled: bool) {
        self.clear_on_shrink = enabled;
    }

    pub fn clear_on_shrink(&self) -> bool {
        self.clear_on_shrink
    }

    pub fn set_show_hardware_cursor(&mut self, enabled: bool) {
        if self.show_hardware_cursor == enabled {
            return;
        }
        self.show_hardware_cursor = enabled;
        if !enabled {
            self.terminal.hide_cursor();
        }
        self.request_render();
    }

    pub fn get_show_hardware_cursor(&self) -> bool {
        self.show_hardware_cursor
    }

    pub fn start(&mut self) {
        self.stopped = false;
        // Input/resize wiring lands with the controllers slice; the demo
        // drives rendering externally.
        let mut on_input = |_data: &str| {};
        let mut on_resize = || {};
        self.terminal.start(&mut on_input, &mut on_resize);
        self.terminal.hide_cursor();
        self.request_render();
    }

    pub fn stop(&mut self) {
        self.stopped = true;
        // Move the cursor to the end of the content to prevent overwriting
        // artifacts on exit.
        if !self.previous_lines.is_empty() {
            let target_row = self.previous_lines.len() as isize; // line after content
            let line_diff = target_row - self.hardware_cursor_row;
            if line_diff > 0 {
                self.terminal.write(&format!("\x1b[{line_diff}B"));
            } else if line_diff < 0 {
                self.terminal.write(&format!("\x1b[{}A", -line_diff));
            }
            self.terminal.write("\r\n");
        }
        self.terminal.show_cursor();
        self.terminal.stop();
    }

    /// Request a render. This first slice renders synchronously; the
    /// 16 ms coalescing scheduler lands with the controllers slice.
    pub fn request_render(&mut self) {
        if self.stopped {
            return;
        }
        self.do_render();
    }

    /// Render all children to lines for a viewport width.
    fn render_children(&mut self, width: usize) -> Vec<String> {
        let width = width.max(1);
        let mut lines = Vec::new();
        for child in &mut self.children {
            lines.extend(child.render(width));
        }
        lines
    }

    /// Invalidate all children (mirrors pi-tui's `TUI.invalidate`). Called
    /// when the theme changes or the tree needs a from-scratch render.
    pub fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }

    /// Find and extract the cursor position from rendered lines, searching the
    /// bottom `height` lines (visible viewport) from bottom to top. Strips the
    /// marker from the line and returns the visual column.
    fn extract_cursor_position(lines: &mut [String], height: usize) -> Option<CursorPosition> {
        if lines.is_empty() || height == 0 {
            return None;
        }
        let viewport_top = lines.len().saturating_sub(height);
        for row in (viewport_top..lines.len()).rev() {
            let line = &lines[row];
            if let Some(marker_index) = line.find(CURSOR_MARKER) {
                let before = &line[..marker_index];
                let col = visible_width(before);
                lines[row] = format!("{}{}", before, &line[marker_index + CURSOR_MARKER.len()..]);
                return Some(CursorPosition { row, col });
            }
        }
        None
    }

    /// Position the hardware cursor for IME candidate window.
    fn position_cursor(&mut self, cursor_pos: Option<CursorPosition>, total_lines: usize) {
        let Some(cursor_pos) = cursor_pos else {
            self.terminal.hide_cursor();
            return;
        };
        if total_lines == 0 {
            self.terminal.hide_cursor();
            return;
        }
        let target_row = cursor_pos.row.min(total_lines - 1) as isize;
        let target_col = cursor_pos.col;

        let row_delta = target_row - self.hardware_cursor_row;
        let mut buffer = String::new();
        if row_delta > 0 {
            buffer.push_str(&format!("\x1b[{row_delta}B"));
        } else if row_delta < 0 {
            buffer.push_str(&format!("\x1b[{}A", -row_delta));
        }
        // Move to absolute column (1-indexed).
        buffer.push_str(&format!("\x1b[{}G", target_col + 1));

        if !buffer.is_empty() {
            self.terminal.write(&buffer);
        }

        self.hardware_cursor_row = target_row;
        if self.show_hardware_cursor {
            self.terminal.show_cursor();
        } else {
            self.terminal.hide_cursor();
        }
    }

    /// Clear the screen + scrollback and render all new lines (optionally
    /// clearing first).
    #[allow(clippy::too_many_arguments)]
    fn full_render(
        &mut self,
        clear: bool,
        new_lines: &[String],
        raw_lines: &[String],
        cursor_pos: Option<CursorPosition>,
        height: usize,
        width: usize,
    ) {
        self.full_redraw_count += 1;
        let mut buffer = String::from("\x1b[?2026h"); // Begin synchronized output
        if clear {
            buffer.push_str("\x1b[2J\x1b[H\x1b[3J"); // Clear screen, home, clear scrollback
        }
        for (i, line) in new_lines.iter().enumerate() {
            if i > 0 {
                buffer.push_str("\r\n");
            }
            buffer.push_str(line);
        }
        buffer.push_str("\x1b[?2026l"); // End synchronized output
        self.terminal.write(&buffer);

        self.cursor_row = new_lines.len().saturating_sub(1) as isize;
        self.hardware_cursor_row = self.cursor_row;
        if clear {
            self.max_lines_rendered = new_lines.len();
        } else {
            self.max_lines_rendered = self.max_lines_rendered.max(new_lines.len());
        }
        let buffer_length = height.max(new_lines.len());
        self.previous_viewport_top = (buffer_length as isize - height as isize).max(0);
        self.position_cursor(cursor_pos, new_lines.len());
        self.previous_lines = new_lines.to_vec();
        self.previous_raw_lines = raw_lines.to_vec();
        self.previous_width = width;
        self.previous_height = height;
    }

    /// The main render loop. Mirrors `TUI.doRender`.
    pub fn do_render(&mut self) {
        if self.stopped {
            return;
        }
        let width = self.terminal.columns();
        let height = self.terminal.rows();
        let width_changed = self.previous_width != 0 && self.previous_width != width;
        let height_changed = self.previous_height != 0 && self.previous_height != height;
        let previous_buffer_length = if self.previous_height > 0 {
            self.previous_viewport_top + self.previous_height as isize
        } else {
            height as isize
        };
        let mut prev_viewport_top = if height_changed {
            (previous_buffer_length - height as isize).max(0)
        } else {
            self.previous_viewport_top
        };
        let mut viewport_top = prev_viewport_top;

        // Render all components to get new lines.
        let raw_lines = self.render_children(width);

        // Extract cursor position before applying line resets.
        let mut new_lines = raw_lines.clone();
        let cursor_pos = Self::extract_cursor_position(&mut new_lines, height);

        // Process raw lines for output: never write a line wider than the
        // terminal; append the trailing segment reset after truncation so
        // styles cannot leak.
        let reuse_processed = !width_changed && !self.previous_raw_lines.is_empty();
        let mut processed_lines: Vec<String> = Vec::with_capacity(new_lines.len());
        for (i, raw_line) in new_lines.iter().enumerate() {
            if reuse_processed
                && i < self.previous_raw_lines.len()
                && *raw_line == self.previous_raw_lines[i]
            {
                processed_lines.push(self.previous_lines[i].clone());
                continue;
            }
            let line_width =
                ascii_visible_width(raw_line, width).unwrap_or_else(|| visible_width(raw_line));
            let mut line = raw_line.clone();
            if line_width > width {
                line = slice_by_column(&line, 0, width, true);
            }
            line = format!("{}{}", normalize_terminal_output(&line), SEGMENT_RESET);
            processed_lines.push(line);
        }
        new_lines = processed_lines;

        // First render — just output everything without clearing (assumes a
        // clean screen).
        if self.previous_lines.is_empty() && !width_changed && !height_changed {
            self.full_render(false, &new_lines, &raw_lines, cursor_pos, height, width);
            return;
        }

        // Width changes always need a full re-render because wrapping changes.
        if width_changed {
            self.full_render(true, &new_lines, &raw_lines, cursor_pos, height, width);
            return;
        }

        // Height changes normally need a full re-render to keep the visible
        // viewport aligned. (Termux software-keyboard special case deferred.)
        if height_changed {
            self.full_render(true, &new_lines, &raw_lines, cursor_pos, height, width);
            return;
        }

        // Content shrunk below the working area — re-render to clear empty
        // rows (overlay handling deferred).
        if self.clear_on_shrink && new_lines.len() < self.max_lines_rendered {
            self.full_render(true, &new_lines, &raw_lines, cursor_pos, height, width);
            return;
        }

        // Find first and last changed lines.
        let max_lines = new_lines.len().max(self.previous_lines.len());
        let mut first_changed: isize = -1;
        let mut last_changed: isize = -1;
        for i in 0..max_lines {
            let old_line = self.previous_lines.get(i).map(String::as_str).unwrap_or("");
            let new_line = new_lines.get(i).map(String::as_str).unwrap_or("");
            if old_line != new_line {
                if first_changed == -1 {
                    first_changed = i as isize;
                }
                last_changed = i as isize;
            }
        }
        let appended_lines = new_lines.len() > self.previous_lines.len();
        if appended_lines {
            if first_changed == -1 {
                first_changed = self.previous_lines.len() as isize;
            }
            last_changed = new_lines.len() as isize - 1;
        }

        // No changes — still update the hardware cursor if it moved.
        if first_changed == -1 {
            self.position_cursor(cursor_pos, new_lines.len());
            self.previous_viewport_top = prev_viewport_top;
            self.previous_height = height;
            self.previous_raw_lines = raw_lines;
            return;
        }

        // All changes are in deleted lines (nothing to render, just clear).
        if first_changed >= new_lines.len() as isize {
            if self.previous_lines.len() > new_lines.len() {
                let mut buffer = String::from("\x1b[?2026h");
                let target_row = new_lines.len().saturating_sub(1) as isize;
                if target_row < prev_viewport_top {
                    self.full_render(true, &new_lines, &raw_lines, cursor_pos, height, width);
                    return;
                }
                let line_diff = target_row - (self.hardware_cursor_row - prev_viewport_top);
                if line_diff > 0 {
                    buffer.push_str(&format!("\x1b[{line_diff}B"));
                } else if line_diff < 0 {
                    buffer.push_str(&format!("\x1b[{}A", -line_diff));
                }
                buffer.push('\r');
                let extra_lines = self.previous_lines.len() - new_lines.len();
                if extra_lines > height {
                    self.full_render(true, &new_lines, &raw_lines, cursor_pos, height, width);
                    return;
                }
                let clear_start_offset = if new_lines.is_empty() { 0 } else { 1 };
                if extra_lines > 0 && clear_start_offset > 0 {
                    buffer.push_str(&format!("\x1b[{clear_start_offset}B"));
                }
                for i in 0..extra_lines {
                    buffer.push_str("\r\x1b[2K");
                    if i < extra_lines - 1 {
                        buffer.push_str("\x1b[1B");
                    }
                }
                let move_back = (extra_lines.saturating_sub(1) + clear_start_offset) as isize;
                if move_back > 0 {
                    buffer.push_str(&format!("\x1b[{move_back}A"));
                }
                buffer.push_str("\x1b[?2026l");
                self.terminal.write(&buffer);
                self.cursor_row = target_row;
                self.hardware_cursor_row = target_row;
            }
            self.position_cursor(cursor_pos, new_lines.len());
            self.previous_lines = new_lines;
            self.previous_raw_lines = raw_lines;
            self.previous_width = width;
            self.previous_height = height;
            self.previous_viewport_top = prev_viewport_top;
            return;
        }

        // Differential rendering can only touch what was actually visible.
        if first_changed < prev_viewport_top {
            self.full_render(true, &new_lines, &raw_lines, cursor_pos, height, width);
            return;
        }

        // Render from first changed line to end.
        let mut buffer = String::from("\x1b[?2026h"); // Begin synchronized output
        let prev_viewport_bottom = prev_viewport_top + height as isize - 1;
        let append_start = appended_lines
            && first_changed == self.previous_lines.len() as isize
            && first_changed > 0;
        let move_target_row = if append_start {
            first_changed - 1
        } else {
            first_changed
        };
        if move_target_row > prev_viewport_bottom {
            let current_screen_row =
                (self.hardware_cursor_row - prev_viewport_top).clamp(0, height as isize - 1);
            let move_to_bottom = height as isize - 1 - current_screen_row;
            if move_to_bottom > 0 {
                buffer.push_str(&format!("\x1b[{move_to_bottom}B"));
            }
            let scroll = move_target_row - prev_viewport_bottom;
            for _ in 0..scroll {
                buffer.push_str("\r\n");
            }
            prev_viewport_top += scroll;
            viewport_top += scroll;
            self.hardware_cursor_row = move_target_row;
        }

        // Move the cursor to the first changed line.
        let line_diff =
            (move_target_row - viewport_top) - (self.hardware_cursor_row - prev_viewport_top);
        if line_diff > 0 {
            buffer.push_str(&format!("\x1b[{line_diff}B"));
        } else if line_diff < 0 {
            buffer.push_str(&format!("\x1b[{}A", -line_diff));
        }
        buffer.push_str(if append_start { "\r\n" } else { "\r" });

        // Only render changed lines (firstChanged to lastChanged).
        let render_end = (last_changed as usize).min(new_lines.len().saturating_sub(1));
        let first = first_changed as usize;
        for (offset, line) in new_lines[first..=render_end].iter().enumerate() {
            if offset > 0 {
                buffer.push_str("\r\n");
            }
            buffer.push_str("\x1b[2K"); // Clear current line
            buffer.push_str(line);
        }

        // Track where the cursor ended up after rendering.
        let mut final_cursor_row = render_end as isize;

        // If we had more lines before, clear them and move the cursor back.
        if self.previous_lines.len() > new_lines.len() {
            if render_end < new_lines.len().saturating_sub(1) {
                let move_down = new_lines.len() - 1 - render_end;
                buffer.push_str(&format!("\x1b[{move_down}B"));
                final_cursor_row = new_lines.len() as isize - 1;
            }
            let extra_lines = self.previous_lines.len() - new_lines.len();
            for _ in 0..extra_lines {
                buffer.push_str("\r\n\x1b[2K");
            }
            buffer.push_str(&format!("\x1b[{extra_lines}A"));
        }

        buffer.push_str("\x1b[?2026l"); // End synchronized output
        self.terminal.write(&buffer);

        // Track cursor position for the next render.
        self.cursor_row = new_lines.len().saturating_sub(1) as isize;
        self.hardware_cursor_row = final_cursor_row;
        self.max_lines_rendered = self.max_lines_rendered.max(new_lines.len());
        self.previous_viewport_top = prev_viewport_top.max(final_cursor_row - height as isize + 1);

        self.position_cursor(cursor_pos, new_lines.len());

        self.previous_lines = new_lines;
        self.previous_raw_lines = raw_lines;
        self.previous_width = width;
        self.previous_height = height;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::terminal::RecordingTerminal;

    struct FixedComponent {
        lines: Vec<String>,
    }

    impl FixedComponent {
        fn new(lines: Vec<String>) -> Self {
            FixedComponent { lines }
        }
    }

    impl Component for FixedComponent {
        fn render(&mut self, _width: usize) -> Vec<String> {
            self.lines.clone()
        }
    }

    /// Wraps a shared [`RecordingTerminal`] so the test can inspect writes
    /// after the TUI has taken ownership of the boxed trait object.
    struct RecordingShared(Rc<RefCell<RecordingTerminal>>);

    impl Terminal for RecordingShared {
        fn start(&mut self, _on_input: &mut dyn FnMut(&str), _on_resize: &mut dyn FnMut()) {}
        fn stop(&mut self) {}
        fn write(&mut self, data: &str) {
            self.0.borrow_mut().write(data);
        }
        fn columns(&self) -> usize {
            self.0.borrow().columns()
        }
        fn rows(&self) -> usize {
            self.0.borrow().rows()
        }
        fn hide_cursor(&mut self) {
            self.0.borrow_mut().hide_cursor();
        }
        fn show_cursor(&mut self) {
            self.0.borrow_mut().show_cursor();
        }
    }

    struct Harness {
        tui: Tui,
        term: Rc<RefCell<RecordingTerminal>>,
    }

    impl Harness {
        fn new(width: usize, height: usize) -> Self {
            let term = Rc::new(RefCell::new(RecordingTerminal::new(width, height)));
            let tui = Tui::new(Box::new(RecordingShared(term.clone())));
            Harness { tui, term }
        }

        fn set_lines(&mut self, lines: Vec<String>) {
            self.tui.children.clear();
            self.tui.children.push(Box::new(FixedComponent::new(lines)));
        }

        fn render(&mut self) -> String {
            self.term.borrow_mut().clear_writes();
            self.tui.do_render();
            self.term.borrow().output()
        }
    }

    #[test]
    fn first_render_writes_all_lines_with_sync_output() {
        let mut h = Harness::new(80, 24);
        h.set_lines(vec!["line one".to_owned(), "line two".to_owned()]);
        let out = h.render();
        assert_eq!(
            out,
            "\x1b[?2026hline one\x1b[0m\x1b]8;;\x07\r\nline two\x1b[0m\x1b]8;;\x07\x1b[?2026l"
        );
    }

    #[test]
    fn appending_lines_renders_only_new_region() {
        let mut h = Harness::new(80, 24);
        h.set_lines(vec!["a".to_owned()]);
        h.render();
        h.set_lines(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let out = h.render();
        // append start = 1: `\r\n` to line 1, render lines 1..=2.
        assert_eq!(
            out,
            "\x1b[?2026h\r\n\x1b[2Kb\x1b[0m\x1b]8;;\x07\r\n\x1b[2Kc\x1b[0m\x1b]8;;\x07\x1b[?2026l"
        );
    }

    #[test]
    fn single_line_change_renders_only_that_line() {
        let mut h = Harness::new(80, 24);
        h.set_lines(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        h.render();
        h.set_lines(vec!["a".to_owned(), "X".to_owned(), "c".to_owned()]);
        let out = h.render();
        // Cursor sat at line 2 after the first frame; move up 1, render line 1.
        assert_eq!(
            out,
            "\x1b[?2026h\x1b[1A\r\x1b[2KX\x1b[0m\x1b]8;;\x07\x1b[?2026l"
        );
    }

    #[test]
    fn deleting_lines_clears_them() {
        let mut h = Harness::new(80, 24);
        h.set_lines(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        h.render();
        h.set_lines(vec!["a".to_owned()]);
        let out = h.render();
        // Deleted-lines path (TS doRender): cursor at line 2 → up 2, \r,
        // move down 1, clear 2 extra lines, move back 2.
        assert_eq!(
            out,
            "\x1b[?2026h\x1b[2A\r\x1b[1B\r\x1b[2K\x1b[1B\r\x1b[2K\x1b[2A\x1b[?2026l"
        );
    }

    #[test]
    fn width_change_forces_full_clear_redraw() {
        let mut h = Harness::new(80, 24);
        h.set_lines(vec!["hello".to_owned()]);
        h.render();
        h.term.borrow_mut().columns_value = 100;
        h.set_lines(vec!["hello world".to_owned()]);
        let out = h.render();
        assert!(out.starts_with("\x1b[?2026h\x1b[2J\x1b[H\x1b[3J"));
        assert!(out.contains("hello world"));
        assert!(out.ends_with("\x1b[?2026l"));
    }

    #[test]
    fn no_change_produces_no_write() {
        let mut h = Harness::new(80, 24);
        h.set_lines(vec!["a".to_owned(), "b".to_owned()]);
        h.render();
        let out = h.render();
        assert_eq!(out, "");
    }

    #[test]
    fn wide_line_is_truncated_to_terminal_width() {
        let mut h = Harness::new(5, 24);
        h.set_lines(vec!["hello world".to_owned()]);
        let out = h.render();
        assert!(out.contains("hello\x1b[0m\x1b]8;;\x07"));
        assert!(!out.contains("hello worl"));
    }

    #[test]
    fn cursor_marker_is_extracted_and_stripped() {
        let mut h = Harness::new(80, 24);
        h.set_lines(vec![format!("ab{}cd", CURSOR_MARKER)]);
        let out = h.render();
        // Marker stripped from the line.
        assert!(!out.contains(CURSOR_MARKER));
        // Hardware cursor positioned at column 2 (1-indexed 3).
        assert!(out.contains("\x1b[3G"));
    }
    #[test]
    fn focus_routes_input_to_child() {
        let term = Rc::new(RefCell::new(RecordingTerminal::new(80, 24)));
        let mut tui = Tui::new(Box::new(RecordingShared(term.clone())));
        // Editor child at index 0.
        tui.add_child(Box::new(crate::editor::Editor::new(
            crate::editor::EditorOptions { padding_x: 0 },
        )));
        tui.set_focus(Some(0));
        tui.handle_input("h");
        tui.handle_input("i");
        // The focused editor received the input; render shows it.
        let out = tui.render_children(80);
        let joined = out.join("\n");
        assert!(
            joined.contains("hi"),
            "editor should contain typed text: {joined}"
        );
        // Unfocus → input no longer routed (no crash).
        tui.set_focus(None);
        tui.handle_input("x");
    }

    #[test]
    fn focus_sets_marker_emission() {
        let term = Rc::new(RefCell::new(RecordingTerminal::new(80, 24)));
        let mut tui = Tui::new(Box::new(RecordingShared(term.clone())));
        tui.add_child(Box::new(crate::editor::Editor::new(
            crate::editor::EditorOptions { padding_x: 0 },
        )));
        tui.set_focus(Some(0));
        let out = tui.render_children(80);
        assert!(
            out.iter()
                .any(|l| l.contains(crate::component::CURSOR_MARKER))
        );
        tui.set_focus(None);
        let out = tui.render_children(80);
        assert!(
            !out.iter()
                .any(|l| l.contains(crate::component::CURSOR_MARKER))
        );
    }
}
