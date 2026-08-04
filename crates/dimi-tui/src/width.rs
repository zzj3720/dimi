//! Terminal-visible width calculation, ported from `@dimi-agent/pi-tui`
//! `src/utils.ts`. The semantics are deliberately kept byte-aligned with the
//! TS reference (grapheme cluster widths, tab = 3, RGI emoji = 2, regional
//! indicators = 2, ANSI escape stripping, slice-by-column) so the Rust TUI's
//! output can be diffed against the TS TUI frame-by-frame.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use crate::ansi::{extract_ansi_code, strip_ansi};

/// Maximum number of entries kept in the [`visible_width`] cache. Mirrors
/// `WIDTH_CACHE_SIZE` in the TS reference.
const WIDTH_CACHE_SIZE: usize = 4096;

static WIDTH_CACHE: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// True when every char of `segment` is zero-width: Default_Ignorable,
/// Control, Mark, or Surrogate (mirrors the TS `zeroWidthRegex`).
fn is_zero_width_segment(segment: &str) -> bool {
    segment.chars().all(|c| {
        let cp = c as u32;
        // Control chars include C0/C1 (0x00-0x1f, 0x7f-0x9f).
        if c.is_control() {
            return true;
        }
        // Surrogates are unpaired here (Rust `char` cannot hold them), so
        // this is unreachable, kept for parity.
        if (0xd800..=0xdfff).contains(&cp) {
            return true;
        }
        // Mark
        if unicode_width::UnicodeWidthChar::width(c) == Some(0) {
            return true;
        }
        // Default_Ignorable_Code_Point (ZWJ, variation selectors, …).
        // Approximation: unicode-width already reports 0 for most; treat the
        // well-known ignorables explicitly for parity with the TS regex.
        matches!(cp, 0x00ad | 0x034f | 0x061c | 0x115f | 0x1160 | 0x17b4 | 0x17b5 | 0x180b..=0x180f | 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x206f | 0x3164 | 0xfe00..=0xfe0f | 0xfeff | 0xffa0 | 0xfff0..=0xfff8 | 0x1bca0..=0x1bca3 | 0x1d173..=0x1d17a | 0xe0000..=0xe0fff)
    })
}

/// Fast heuristic for "could be an RGI emoji" — mirrors the TS pre-filter that
/// avoids running the expensive RGI_Emoji regex on every grapheme.
fn could_be_emoji(segment: &str) -> bool {
    let Some(cp) = segment.chars().next().map(|c| c as u32) else {
        return false;
    };
    (0x1f000..=0x1fbff).contains(&cp) // Emoji and Pictograph
        || (0x2300..=0x23ff).contains(&cp) // Misc technical
        || (0x2600..=0x27bf).contains(&cp) // Misc symbols, dingbats
        || (0x2b50..=0x2b55).contains(&cp) // Specific stars/circles
        || segment.contains('\u{fe0f}') // VS16 (emoji presentation selector)
        || segment.chars().count() > 2 // Multi-codepoint sequences (ZWJ, skin tones, …)
}

/// Terminal width of one grapheme cluster. Mirrors the TS `graphemeWidth`.
///
/// The TS reference runs an RGI_Emoji regex to decide emoji presentation
/// width. That regex needs a `unicode-emoji`-style table; we approximate it
/// with the same `couldBeEmoji` pre-filter plus: VS16 present, or the base
/// already being East-Asian-wide (emoji blocks are W in the EAW table). This
/// covers the observable parity surface (©=1, ©️=2, ☀=2, ZWJ/skin-tone
/// sequences=2); edge cases are guarded by differential tests against the TS
/// implementation.
pub fn grapheme_width(segment: &str) -> usize {
    if segment == "\t" {
        return 3;
    }
    if is_zero_width_segment(segment) {
        return 0;
    }
    let base = segment.chars().next().unwrap_or_default();
    if could_be_emoji(segment) && (segment.contains('\u{fe0f}') || base.width() == Some(2)) {
        return 2;
    }
    let mut width = base.width().unwrap_or(0);

    // Regional indicator symbols (U+1F1E6..U+1F1FF) render full-width in most
    // terminals even when isolated during streaming; keep width conservative
    // (2) to avoid auto-wrap drift artifacts.
    let cp = base as u32;
    if (0x1f1e6..=0x1f1ff).contains(&cp) {
        width = 2;
    }

    // Trailing halfwidth/fullwidth forms and Thai/Lao AM vowels that segment
    // with a base.
    for c in segment.chars().skip(1) {
        let ccp = c as u32;
        if (0xff00..=0xffef).contains(&ccp) {
            width += c.width().unwrap_or(0);
        } else if ccp == 0x0e33 || ccp == 0x0eb3 {
            width += 1;
        }
    }

    width
}

/// True when every byte is a printable ASCII char (0x20..=0x7e).
fn is_printable_ascii(s: &str) -> bool {
    s.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// Visible width of `str` in terminal columns. Tabs count as 3; ANSI/OSC/APC
/// escape sequences are stripped. Mirrors the TS `visibleWidth`.
pub fn visible_width(str: &str) -> usize {
    if str.is_empty() {
        return 0;
    }
    // Fast path: pure ASCII printable.
    if is_printable_ascii(str) {
        return str.len();
    }
    // Check cache.
    if let Ok(cache) = WIDTH_CACHE.lock() {
        if let Some(w) = cache.get(str) {
            return *w;
        }
    }

    let mut clean = str;
    let mut owned;
    if str.contains('\t') {
        owned = str.replace('\t', "   ");
        clean = &owned;
    }
    if clean.contains('\x1b') {
        owned = strip_ansi(clean);
        clean = &owned;
    }

    let mut width = 0;
    for segment in clean.graphemes(true) {
        width += grapheme_width(segment);
    }

    // Cache result, evicting the oldest entry when full.
    if let Ok(mut cache) = WIDTH_CACHE.lock() {
        if cache.len() >= WIDTH_CACHE_SIZE {
            if let Some(oldest) = cache.keys().next().cloned() {
                cache.remove(&oldest);
            }
        }
        cache.insert(str.to_owned(), width);
    }
    width
}

/// Strip all ANSI/OSC/APC escape sequences from `s`.
///
/// Fast visible-width scan for lines whose printable content is plain ASCII,
/// skipping over ANSI escape sequences. Returns the visible width, or `None`
/// when the line contains control characters or non-ASCII content (caller
/// should fall back to [`visible_width`]). Early-exits as soon as the width
/// exceeds `limit`, returning the partial count.
pub fn ascii_visible_width(line: &str, limit: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut width = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            let ansi = extract_ansi_code(line, i)?;
            i += ansi.1;
            continue;
        }
        if !(0x20..=0x7e).contains(&b) {
            return None;
        }
        width += 1;
        if width > limit {
            return Some(width);
        }
        i += 1;
    }
    Some(width)
}

/// Normalize text for terminal output without changing logical editor content.
///
/// Slice a range of visible columns from a line, handling ANSI codes and wide
/// chars. `strict` excludes wide chars at the boundary that would extend past
/// the range. Mirrors `sliceWithWidth`.
pub fn slice_with_width(
    line: &str,
    start_col: usize,
    length: usize,
    strict: bool,
) -> (String, usize) {
    if length == 0 {
        return (String::new(), 0);
    }
    let end_col = start_col + length;
    let mut result = String::new();
    let mut result_width = 0;
    let mut current_col = 0;
    let mut i = 0;
    let mut pending_ansi = String::new();

    while i < line.len() {
        if let Some((code, len)) = extract_ansi_code(line, i) {
            if current_col >= start_col && current_col < end_col {
                result.push_str(&code);
            } else if current_col < start_col {
                pending_ansi.push_str(&code);
            }
            i += len;
            continue;
        }

        let text_end = next_text_end(line, i);
        for segment in line[i..text_end].graphemes(true) {
            let w = grapheme_width(segment);
            let in_range = current_col >= start_col && current_col < end_col;
            let fits = !strict || current_col + w <= end_col;
            if in_range && fits {
                if !pending_ansi.is_empty() {
                    result.push_str(&pending_ansi);
                    pending_ansi.clear();
                }
                result.push_str(segment);
                result_width += w;
            }
            current_col += w;
            if current_col >= end_col {
                break;
            }
        }
        i = text_end;
        if current_col >= end_col {
            break;
        }
    }
    (result, result_width)
}

/// [`slice_with_width`] returning only the text.
pub fn slice_by_column(line: &str, start_col: usize, length: usize, strict: bool) -> String {
    slice_with_width(line, start_col, length, strict).0
}

/// Index just past the next non-ANSI run starting at `i` (stops at an escape
/// start or end of string).
fn next_text_end(line: &str, i: usize) -> usize {
    let bytes = line.as_bytes();
    let mut end = i;
    while end < bytes.len() && bytes[end] != 0x1b {
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_ascii() {
        assert_eq!(visible_width(""), 0);
        assert_eq!(visible_width("hello"), 5);
        assert_eq!(visible_width("hello world"), 11);
    }

    #[test]
    fn visible_width_cjk() {
        // CJK ideographs are width 2.
        assert_eq!(visible_width("你好"), 4);
        assert_eq!(visible_width("a你b"), 4);
    }

    #[test]
    fn visible_width_tab_is_three() {
        assert_eq!(visible_width("a\tb"), 5);
        assert_eq!(grapheme_width("\t"), 3);
    }

    #[test]
    fn visible_width_strips_ansi() {
        assert_eq!(visible_width("\x1b[31mred\x1b[0m"), 3);
        assert_eq!(
            visible_width("\x1b]8;;https://example.com\x07link\x1b]8;;\x07"),
            4
        );
        // APC cursor marker is stripped too.
        assert_eq!(visible_width("\x1b_pi:c\x07x"), 1);
    }

    #[test]
    fn visible_width_emoji() {
        // U+1F600 😀 is a RGI emoji presentation → 2.
        assert_eq!(visible_width("😀"), 2);
        // Regional indicator is conservative 2 even isolated.
        assert_eq!(visible_width("🇺"), 2);
        // U+26A0 ⚠ without VS16 is text presentation (EAW ambiguous) → 1.
        assert_eq!(visible_width("⚠"), 1);
        // With VS16 it becomes emoji presentation → 2.
        assert_eq!(visible_width("⚠️"), 2);
    }

    #[test]
    fn visible_width_marks_are_zero_width() {
        // Combining acute accent attaches to 'e'.
        assert_eq!(visible_width("e\u{0301}"), 1);
        // ZWJ sequence: 👨👩👧 is one grapheme cluster, width 2.
        assert_eq!(visible_width("👨\u{200d}👩\u{200d}👧"), 2);
    }

    #[test]
    fn ascii_visible_width_skips_ansi() {
        assert_eq!(ascii_visible_width("\x1b[31mhello\x1b[0m", 100), Some(5));
        assert_eq!(ascii_visible_width("hello", 3), Some(4)); // early exit > limit
        assert_eq!(ascii_visible_width("héllo", 100), None); // non-ASCII
        assert_eq!(ascii_visible_width("he\tllo", 100), None); // control char
    }

    #[test]
    fn slice_by_column_plain() {
        assert_eq!(slice_by_column("hello", 0, 5, false), "hello");
        assert_eq!(slice_by_column("hello", 1, 3, false), "ell");
        assert_eq!(slice_by_column("hello", 0, 3, false), "hel");
    }

    #[test]
    fn slice_by_column_cjk() {
        assert_eq!(slice_by_column("你好", 0, 2, false), "你");
        // strict excludes the wide char that would overflow the boundary.
        assert_eq!(slice_by_column("你好", 0, 3, true), "你");
        assert_eq!(slice_by_column("你好", 2, 2, false), "好");
        // Half a wide char at the start is excluded.
        assert_eq!(slice_by_column("你好", 1, 2, true), "");
    }

    #[test]
    fn slice_by_column_ansi() {
        // TS sliceWithWidth keeps ANSI codes only while inside the slice
        // range; a trailing reset after the end column is dropped.
        assert_eq!(
            slice_by_column("\x1b[31mred\x1b[0m", 0, 3, false),
            "\x1b[31mred"
        );
        // ANSI codes before the slice are kept as prefix.
        assert_eq!(
            slice_by_column("\x1b[31mred\x1b[0m", 1, 2, false),
            "\x1b[31med"
        );
    }
}
