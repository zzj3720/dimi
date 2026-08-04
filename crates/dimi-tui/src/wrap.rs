//! ANSI-aware text wrapping and truncation, ported from
//! `@dimi-agent/pi-tui` `src/utils.ts` (AnsiCodeTracker, wrapTextWithAnsi,
//! splitIntoTokensWithAnsi, truncateToWidth, applyBackgroundToLine).
//!
//! The semantics are kept byte-aligned with the TS reference so wrapped and
//! truncated lines can be diffed against the TS TUI.

use unicode_segmentation::UnicodeSegmentation;

use crate::ansi::extract_ansi_code;
use crate::width::{grapheme_width, visible_width};

/// Active OSC 8 hyperlink state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveHyperlink {
    params: String,
    url: String,
    terminator: Osc8Terminator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Osc8Terminator {
    Bel,
    St,
}

impl ActiveHyperlink {
    fn format(&self) -> String {
        format!(
            "\x1b]8;{};{}{}",
            self.params,
            self.url,
            self.terminator.bytes()
        )
    }
    fn close(&self) -> String {
        format!("\x1b]8;;{}", self.terminator.bytes())
    }
}

impl Osc8Terminator {
    fn bytes(self) -> &'static str {
        match self {
            Osc8Terminator::Bel => "\x07",
            Osc8Terminator::St => "\x1b\\",
        }
    }
}

/// Parse an OSC 8 hyperlink sequence: `ESC]8;params;url TERM`.
/// Returns `Some(hyperlink)` when the code is a hyperlink open,
/// `Some(None)` is not needed — see parseOsc8Hyperlink: returns
/// `undefined` (not a hyperlink) / `null` (empty URL) / `ActiveHyperlink`.
fn parse_osc8_hyperlink(ansi_code: &str) -> Option<Option<ActiveHyperlink>> {
    if !ansi_code.starts_with("\x1b]8;") {
        return None;
    }
    let (terminator, body_len) = if ansi_code.ends_with('\x07') {
        (Osc8Terminator::Bel, ansi_code.len() - 1)
    } else if ansi_code.ends_with("\x1b\\") {
        (Osc8Terminator::St, ansi_code.len() - 2)
    } else {
        return None;
    };
    let body = &ansi_code[4..body_len];
    let separator_index = body.find(';')?;
    let params = body[..separator_index].to_owned();
    let url = body[separator_index + 1..].to_owned();
    if url.is_empty() {
        Some(None)
    } else {
        Some(Some(ActiveHyperlink {
            params,
            url,
            terminator,
        }))
    }
}

/// Track active ANSI SGR codes so styling can be preserved across line breaks
/// (mirrors pi-tui's `AnsiCodeTracker`).
#[derive(Debug, Clone, Default)]
pub struct AnsiCodeTracker {
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    blink: bool,
    inverse: bool,
    hidden: bool,
    strikethrough: bool,
    fg_color: Option<String>,
    bg_color: Option<String>,
    active_hyperlink: Option<ActiveHyperlink>,
}

impl AnsiCodeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process one ANSI code, updating tracked style state.
    pub fn process(&mut self, ansi_code: &str) {
        if let Some(hyperlink) = parse_osc8_hyperlink(ansi_code) {
            self.active_hyperlink = hyperlink;
            return;
        }
        if !ansi_code.ends_with('m') {
            return;
        }
        // Extract the parameters between ESC[ and m.
        let Some(inner) = ansi_code
            .strip_prefix("\x1b[")
            .and_then(|s| s.strip_suffix('m'))
        else {
            return;
        };
        if inner.is_empty() || inner == "0" {
            self.reset();
            return;
        }
        let parts: Vec<&str> = inner.split(';').collect();
        let mut i = 0;
        while i < parts.len() {
            let code = parts[i].parse::<u32>().unwrap_or(0);
            if code == 38 || code == 48 {
                if parts.get(i + 1) == Some(&"5") && parts.get(i + 2).is_some() {
                    let color_code = format!("{};{};{}", parts[i], parts[i + 1], parts[i + 2]);
                    if code == 38 {
                        self.fg_color = Some(color_code);
                    } else {
                        self.bg_color = Some(color_code);
                    }
                    i += 3;
                    continue;
                } else if parts.get(i + 1) == Some(&"2") && parts.get(i + 4).is_some() {
                    let color_code = format!(
                        "{};{};{};{};{}",
                        parts[i],
                        parts[i + 1],
                        parts[i + 2],
                        parts[i + 3],
                        parts[i + 4]
                    );
                    if code == 38 {
                        self.fg_color = Some(color_code);
                    } else {
                        self.bg_color = Some(color_code);
                    }
                    i += 5;
                    continue;
                }
            }
            match code {
                0 => self.reset(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                5 => self.blink = true,
                7 => self.inverse = true,
                8 => self.hidden = true,
                9 => self.strikethrough = true,
                21 => self.bold = false,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                25 => self.blink = false,
                27 => self.inverse = false,
                28 => self.hidden = false,
                29 => self.strikethrough = false,
                39 => self.fg_color = None,
                49 => self.bg_color = None,
                _ => {
                    if (30..=37).contains(&code) || (90..=97).contains(&code) {
                        self.fg_color = Some(code.to_string());
                    } else if (40..=47).contains(&code) || (100..=107).contains(&code) {
                        self.bg_color = Some(code.to_string());
                    }
                }
            }
            i += 1;
        }
    }

    fn reset(&mut self) {
        self.bold = false;
        self.dim = false;
        self.italic = false;
        self.underline = false;
        self.blink = false;
        self.inverse = false;
        self.hidden = false;
        self.strikethrough = false;
        self.fg_color = None;
        self.bg_color = None;
        // SGR reset does not affect OSC 8 hyperlink state.
    }

    /// Clear all state for reuse.
    pub fn clear(&mut self) {
        self.reset();
        self.active_hyperlink = None;
    }

    /// Active SGR codes + hyperlink re-open, in pi-tui's order.
    pub fn get_active_codes(&self) -> String {
        let mut codes: Vec<&str> = Vec::new();
        if self.bold {
            codes.push("1");
        }
        if self.dim {
            codes.push("2");
        }
        if self.italic {
            codes.push("3");
        }
        if self.underline {
            codes.push("4");
        }
        if self.blink {
            codes.push("5");
        }
        if self.inverse {
            codes.push("7");
        }
        if self.hidden {
            codes.push("8");
        }
        if self.strikethrough {
            codes.push("9");
        }
        if let Some(fg) = &self.fg_color {
            codes.push(fg);
        }
        if let Some(bg) = &self.bg_color {
            codes.push(bg);
        }
        let mut result = if codes.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", codes.join(";"))
        };
        if let Some(h) = &self.active_hyperlink {
            result.push_str(&h.format());
        }
        result
    }

    pub fn has_active_codes(&self) -> bool {
        self.bold
            || self.dim
            || self.italic
            || self.underline
            || self.blink
            || self.inverse
            || self.hidden
            || self.strikethrough
            || self.fg_color.is_some()
            || self.bg_color.is_some()
            || self.active_hyperlink.is_some()
    }

    /// Reset codes for attributes that need to be turned off at line end
    /// (underline to prevent bleeding into padding; OSC 8 to close and
    /// re-open on the next line).
    pub fn get_line_end_reset(&self) -> String {
        let mut result = String::new();
        if self.underline {
            result.push_str("\x1b[24m");
        }
        if let Some(h) = &self.active_hyperlink {
            result.push_str(&h.close());
        }
        result
    }
}

fn update_tracker_from_text(text: &str, tracker: &mut AnsiCodeTracker) {
    let mut i = 0;
    while i < text.len() {
        if let Some((code, len)) = extract_ansi_code(text, i) {
            tracker.process(&code);
            i += len;
        } else {
            i += 1;
        }
    }
}

/// Advance `end` by one full char, stopping at tab or ESC (keeps `end` on a
/// char boundary so later `text[..end]` slices are safe).
fn next_text_run_end(text: &str, mut end: usize) -> usize {
    while end < text.len() {
        let b = text.as_bytes()[end];
        if b == b'\t' || b == 0x1b {
            break;
        }
        end += text[end..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
    }
    end
}

/// True when every char is a printable ASCII char (0x20..=0x7e).
pub fn is_printable_ascii(s: &str) -> bool {
    s.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// Split text into word/space tokens while keeping ANSI codes attached to the
/// following visible character (pi-tui `splitIntoTokensWithAnsi`).
pub fn split_into_tokens_with_ansi(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut pending_ansi = String::new();
    let mut current_kind: Option<&'static str> = None; // "space" | "word"

    let mut i = 0;
    while i < text.len() {
        if let Some((code, len)) = extract_ansi_code(text, i) {
            pending_ansi.push_str(&code);
            i += len;
            continue;
        }

        let mut end = i;
        while end < text.len() && extract_ansi_code(text, end).is_none() {
            end += 1;
        }

        for segment in text[i..end].graphemes(true) {
            let segment_is_space = segment == " ";
            let is_cjk = !segment_is_space && is_cjk_break_char(segment);
            if is_cjk {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                let token = format!("{pending_ansi}{segment}");
                pending_ansi.clear();
                tokens.push(token);
                continue;
            }
            let segment_kind: &'static str = if segment_is_space { "space" } else { "word" };
            if !current.is_empty() && current_kind != Some(segment_kind) {
                tokens.push(std::mem::take(&mut current));
            }
            if !pending_ansi.is_empty() {
                current.push_str(&pending_ansi);
                pending_ansi.clear();
            }
            current_kind = Some(segment_kind);
            current.push_str(segment);
        }
        i = end;
    }

    // Handle remaining pending ANSI codes (attach to last token).
    if !pending_ansi.is_empty() {
        if !current.is_empty() {
            current.push_str(&pending_ansi);
        } else if !tokens.is_empty() {
            tokens.last_mut().unwrap().push_str(&pending_ansi);
        } else {
            current = pending_ansi;
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Whether the grapheme starts with a CJK character that pi-tui's
/// `cjkBreakRegex` treats as a break opportunity. The TS regex uses
/// `Script_Extensions`; we approximate with the main CJK script ranges.
fn is_cjk_break_char(segment: &str) -> bool {
    let Some(cp) = segment.chars().next().map(|c| c as u32) else {
        return false;
    };
    (0x3400..=0x4dbf).contains(&cp) // CJK Ext A
        || (0x4e00..=0x9fff).contains(&cp) // Han
        || (0x3040..=0x309f).contains(&cp) // Hiragana
        || (0x30a0..=0x30ff).contains(&cp) // Katakana
        || (0xac00..=0xd7af).contains(&cp) // Hangul
        || (0x3100..=0x312f).contains(&cp) // Bopomofo
}

/// Break a long word into grapheme-sized chunks with ANSI handling
/// (pi-tui `breakLongWord`).
fn break_long_word(word: &str, width: usize, tracker: &mut AnsiCodeTracker) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current_line = tracker.get_active_codes();
    let mut current_width = 0usize;

    // First separate ANSI codes from visible content.
    #[derive(Debug)]
    enum Seg {
        Ansi(String),
        Grapheme(String),
    }
    let mut segments: Vec<Seg> = Vec::new();
    let mut i = 0;
    while i < word.len() {
        if let Some((code, len)) = extract_ansi_code(word, i) {
            segments.push(Seg::Ansi(code));
            i += len;
        } else {
            let end = next_text_run_end(word, i);
            for seg in word[i..end].graphemes(true) {
                segments.push(Seg::Grapheme(seg.to_owned()));
            }
            i = end;
        }
    }

    for seg in segments {
        match seg {
            Seg::Ansi(code) => {
                current_line.push_str(&code);
                tracker.process(&code);
                continue;
            }
            Seg::Grapheme(grapheme) => {
                if grapheme.is_empty() {
                    continue;
                }
                let grapheme_width = grapheme_width(&grapheme);
                if current_width + grapheme_width > width {
                    let line_end_reset = tracker.get_line_end_reset();
                    if !line_end_reset.is_empty() {
                        current_line.push_str(&line_end_reset);
                    }
                    lines.push(std::mem::take(&mut current_line));
                    current_line = tracker.get_active_codes();
                    current_width = 0;
                }
                current_line.push_str(&grapheme);
                current_width += grapheme_width;
            }
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Wrap text with ANSI codes preserved. Word-wrapping only — no padding, no
/// backgrounds. Each returned line is ≤ `width` visible columns.
pub fn wrap_text_with_ansi(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let input_lines: Vec<&str> = text.split('\n').collect();
    let mut result: Vec<String> = Vec::new();
    let mut tracker = AnsiCodeTracker::new();
    for input_line in input_lines {
        // Prepend active ANSI codes from previous lines (except the first).
        let prefix = if result.is_empty() {
            String::new()
        } else {
            tracker.get_active_codes()
        };
        let wrapped = wrap_single_line(&format!("{prefix}{input_line}"), width);
        for wrapped_line in wrapped {
            result.push(wrapped_line);
        }
        update_tracker_from_text(input_line, &mut tracker);
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

fn wrap_single_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    let visible_length = visible_width(line);
    if visible_length <= width {
        return vec![line.to_owned()];
    }

    let mut wrapped: Vec<String> = Vec::new();
    let mut tracker = AnsiCodeTracker::new();
    let tokens = split_into_tokens_with_ansi(line);

    let mut current_line = String::new();
    let mut current_visible_length = 0usize;

    for token in tokens {
        let token_visible_length = visible_width(&token);
        let is_whitespace = token.trim().is_empty();

        // Token itself is too long — break it character by character.
        if token_visible_length > width && !is_whitespace {
            if !current_line.is_empty() {
                let line_end_reset = tracker.get_line_end_reset();
                if !line_end_reset.is_empty() {
                    current_line.push_str(&line_end_reset);
                }
                wrapped.push(std::mem::take(&mut current_line));
            }
            let broken = break_long_word(&token, width, &mut tracker);
            for line in broken.iter().take(broken.len().saturating_sub(1)) {
                wrapped.push(line.clone());
            }
            current_line = broken.last().cloned().unwrap_or_default();
            current_visible_length = visible_width(&current_line);
            continue;
        }

        // Check if adding this token would exceed width.
        let total_needed = current_visible_length + token_visible_length;
        if total_needed > width && current_visible_length > 0 {
            // Trim trailing whitespace, then add underline reset.
            let mut line_to_wrap = current_line.trim_end().to_owned();
            let line_end_reset = tracker.get_line_end_reset();
            if !line_end_reset.is_empty() {
                line_to_wrap.push_str(&line_end_reset);
            }
            wrapped.push(line_to_wrap);
            if is_whitespace {
                current_line = tracker.get_active_codes();
                current_visible_length = 0;
            } else {
                current_line = format!("{}{}", tracker.get_active_codes(), token);
                current_visible_length = token_visible_length;
            }
        } else {
            current_line.push_str(&token);
            current_visible_length += token_visible_length;
        }
        update_tracker_from_text(&token, &mut tracker);
    }

    if !current_line.is_empty() {
        wrapped.push(current_line);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    // Trailing whitespace can cause lines to exceed the requested width.
    wrapped.iter_mut().for_each(|l| {
        let t = l.trim_end();
        *l = t.to_owned();
    });
    wrapped
}

/// Truncate a fragment (no ellipsis) to a visible width.
pub fn truncate_fragment_to_width(text: &str, max_width: usize) -> (String, usize) {
    if max_width == 0 || text.is_empty() {
        return (String::new(), 0);
    }
    if is_printable_ascii(text) {
        let clipped: String = text.chars().take(max_width).collect();
        let w = clipped.len();
        return (clipped, w);
    }
    let has_ansi = text.contains('\x1b');
    let has_tabs = text.contains('\t');
    if !has_ansi && !has_tabs {
        let mut result = String::new();
        let mut width = 0usize;
        for segment in text.graphemes(true) {
            let w = grapheme_width(segment);
            if width + w > max_width {
                break;
            }
            result.push_str(segment);
            width += w;
        }
        return (result, width);
    }

    let mut result = String::new();
    let mut width = 0usize;
    let mut i = 0;
    let mut pending_ansi = String::new();
    while i < text.len() {
        if let Some((code, len)) = extract_ansi_code(text, i) {
            pending_ansi.push_str(&code);
            i += len;
            continue;
        }
        if text[i..].starts_with('\t') {
            if width + 3 > max_width {
                break;
            }
            if !pending_ansi.is_empty() {
                result.push_str(&pending_ansi);
                pending_ansi.clear();
            }
            result.push('\t');
            width += 3;
            i += 1;
            continue;
        }
        let end = next_text_run_end(text, i);
        for segment in text[i..end].graphemes(true) {
            let w = grapheme_width(segment);
            if width + w > max_width {
                return (result, width);
            }
            if !pending_ansi.is_empty() {
                result.push_str(&pending_ansi);
                pending_ansi.clear();
            }
            result.push_str(segment);
            width += w;
        }
        i = end;
    }
    (result, width)
}

fn finalize_truncated_result(
    prefix: &str,
    prefix_width: usize,
    ellipsis: &str,
    ellipsis_width: usize,
    max_width: usize,
    pad: bool,
) -> String {
    const RESET: &str = "\x1b[0m";
    let visible = prefix_width + ellipsis_width;
    let mut result = String::new();
    if ellipsis.is_empty() {
        result.push_str(prefix);
        result.push_str(RESET);
    } else {
        result.push_str(prefix);
        result.push_str(RESET);
        result.push_str(ellipsis);
        result.push_str(RESET);
    }
    if pad {
        let pad_count = max_width.saturating_sub(visible);
        result.push_str(&" ".repeat(pad_count));
    }
    result
}

/// Truncate text to a maximum visible width, adding ellipsis if needed.
/// Optionally pads with spaces to exactly maxWidth.
pub fn truncate_to_width(text: &str, max_width: usize, ellipsis: &str, pad: bool) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text.is_empty() {
        return if pad {
            " ".repeat(max_width)
        } else {
            String::new()
        };
    }
    let ellipsis_width = visible_width(ellipsis);
    if ellipsis_width >= max_width {
        let text_width = visible_width(text);
        if text_width <= max_width {
            return if pad {
                format!("{text}{}", " ".repeat(max_width - text_width))
            } else {
                text.to_owned()
            };
        }
        let clipped = truncate_fragment_to_width(ellipsis, max_width);
        if clipped.1 == 0 {
            return if pad {
                " ".repeat(max_width)
            } else {
                String::new()
            };
        }
        return finalize_truncated_result("", 0, &clipped.0, clipped.1, max_width, pad);
    }

    if is_printable_ascii(text) {
        if text.len() <= max_width {
            return if pad {
                format!("{text}{}", " ".repeat(max_width - text.len()))
            } else {
                text.to_owned()
            };
        }
        let target_width = max_width - ellipsis_width;
        return finalize_truncated_result(
            &text[..target_width],
            target_width,
            ellipsis,
            ellipsis_width,
            max_width,
            pad,
        );
    }

    let target_width = max_width - ellipsis_width;
    let mut result = String::new();
    let mut pending_ansi = String::new();
    let mut visible_so_far = 0usize;
    let mut kept_width = 0usize;
    let mut keep_contiguous_prefix = true;
    let mut overflowed = false;
    let has_ansi = text.contains('\x1b');
    let has_tabs = text.contains('\t');

    let exhausted_input = if !has_ansi && !has_tabs {
        for segment in text.graphemes(true) {
            let width = grapheme_width(segment);
            if keep_contiguous_prefix && kept_width + width <= target_width {
                result.push_str(segment);
                kept_width += width;
            } else {
                keep_contiguous_prefix = false;
            }
            visible_so_far += width;
            if visible_so_far > max_width {
                overflowed = true;
                break;
            }
        }
        !overflowed
    } else {
        let mut i = 0;
        while i < text.len() {
            if let Some((code, len)) = extract_ansi_code(text, i) {
                pending_ansi.push_str(&code);
                i += len;
                continue;
            }
            if text[i..].starts_with('\t') {
                if keep_contiguous_prefix && kept_width + 3 <= target_width {
                    if !pending_ansi.is_empty() {
                        result.push_str(&pending_ansi);
                        pending_ansi.clear();
                    }
                    result.push('\t');
                    kept_width += 3;
                } else {
                    keep_contiguous_prefix = false;
                    pending_ansi.clear();
                }
                visible_so_far += 3;
                if visible_so_far > max_width {
                    overflowed = true;
                    break;
                }
                i += 1;
                continue;
            }
            let end = next_text_run_end(text, i);
            for segment in text[i..end].graphemes(true) {
                let width = grapheme_width(segment);
                if keep_contiguous_prefix && kept_width + width <= target_width {
                    if !pending_ansi.is_empty() {
                        result.push_str(&pending_ansi);
                        pending_ansi.clear();
                    }
                    result.push_str(segment);
                    kept_width += width;
                } else {
                    keep_contiguous_prefix = false;
                    pending_ansi.clear();
                }
                visible_so_far += width;
                if visible_so_far > max_width {
                    overflowed = true;
                    break;
                }
            }
            if overflowed {
                break;
            }
            i = end;
        }
        i >= text.len()
    };

    if !overflowed && exhausted_input {
        return if pad {
            format!(
                "{text}{}",
                " ".repeat(max_width.saturating_sub(visible_so_far))
            )
        } else {
            text.to_owned()
        };
    }
    finalize_truncated_result(
        &result,
        kept_width,
        ellipsis,
        ellipsis_width,
        max_width,
        pad,
    )
}

/// Apply a background color to a line, padding to full width.
pub fn apply_background_to_line(line: &str, width: usize, bg: impl Fn(&str) -> String) -> String {
    let visible_len = visible_width(line);
    let padding_needed = width.saturating_sub(visible_len);
    let with_padding = format!("{line}{}", " ".repeat(padding_needed));
    bg(&with_padding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_basic() {
        let lines = wrap_text_with_ansi(
            "The quick brown fox jumps over the lazy dog and keeps running",
            30,
        );
        assert_eq!(
            lines,
            vec![
                "The quick brown fox jumps over",
                "the lazy dog and keeps running"
            ]
        );
    }

    #[test]
    fn wrap_ansi_reopen() {
        let lines = wrap_text_with_ansi("\x1b[1maaa bbb ccc ddd eee fff\x1b[22m", 8);
        assert_eq!(lines[0], "\x1b[1maaa bbb");
        assert_eq!(lines[1], "\x1b[1mccc ddd");
        assert_eq!(lines[2], "\x1b[1meee fff\x1b[22m");
    }

    #[test]
    fn wrap_long_word() {
        let lines = wrap_text_with_ansi("supercalifragilisticexpialidocious is a long word", 20);
        assert_eq!(lines[0], "supercalifragilistic");
        // The wrapped line is trimmed at the wrap point (padding happens later
        // in the caller, e.g. the markdown renderer).
        assert_eq!(lines[1], "expialidocious is a");
        assert_eq!(lines[2], "long word");
    }

    #[test]
    fn wrap_newline_carries_style() {
        let lines = wrap_text_with_ansi("line1\nline2", 80);
        assert_eq!(lines, vec!["line1", "line2"]);
        // Styled first line carries the style into the second.
        let lines = wrap_text_with_ansi("\x1b[1mline1\nline2\x1b[22m", 80);
        assert_eq!(lines, vec!["\x1b[1mline1", "\x1b[1mline2\x1b[22m"]);
    }

    #[test]
    fn truncate_ascii() {
        // finalizeTruncatedResult always appends SGR resets around the ellipsis.
        assert_eq!(
            truncate_to_width("hello world", 5, "...", false),
            "he\x1b[0m...\x1b[0m"
        );
        assert_eq!(
            truncate_to_width("hello world", 11, "...", false),
            "hello world"
        );
        assert_eq!(truncate_to_width("hello", 5, "...", false), "hello");
        assert_eq!(truncate_to_width("hello", 5, "...", true), "hello");
        assert_eq!(
            truncate_to_width("hello world", 8, "...", true),
            format!("hello{}\x1b[0m", "\x1b[0m...")
        );
    }

    #[test]
    fn truncate_with_ansi() {
        let out = truncate_to_width("\x1b[31mred text here\x1b[0m", 6, "...", false);
        // Visible: "red te" (6) + "..." but truncated to maxWidth-3=3 visible
        assert_eq!(out, "\x1b[31mred\x1b[0m...\x1b[0m");
    }

    #[test]
    fn truncate_ellipsis_wider_than_max() {
        // "…" is width 1; a double ellipsis (width 2) fits maxWidth=2 fully.
        assert_eq!(truncate_to_width("abc", 2, "……", false), "\x1b[0m……\x1b[0m");
    }

    #[test]
    fn tracker_active_codes() {
        let mut t = AnsiCodeTracker::new();
        t.process("\x1b[1m");
        t.process("\x1b[38;2;79;168;255m");
        assert_eq!(t.get_active_codes(), "\x1b[1;38;2;79;168;255m");
        assert_eq!(t.get_line_end_reset(), "");
        t.process("\x1b[4m");
        assert_eq!(t.get_line_end_reset(), "\x1b[24m");
        t.process("\x1b[0m");
        assert_eq!(t.get_active_codes(), "");
    }

    #[test]
    fn tracker_hyperlink() {
        let mut t = AnsiCodeTracker::new();
        t.process("\x1b]8;;https://example.com\x1b\\");
        assert_eq!(t.get_active_codes(), "\x1b]8;;https://example.com\x1b\\");
        assert_eq!(t.get_line_end_reset(), "\x1b]8;;\x1b\\");
        t.process("\x1b]8;;\x07");
        assert_eq!(t.get_active_codes(), "");
    }
}
