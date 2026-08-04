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
        // Hangul jamo are NOT zero-width — they are width-1/2 characters
        // (TS `\p{Mark}` does not include them; eastAsianWidth reports W).
        // unicode-width reports 0 for some, so exclude the jamo ranges first.
        if is_hangul_jamo(cp) {
            return false;
        }
        // Mark (Mn/Mc): unicode-width reports 0 for most combining marks;
        // some Mc spacing marks report 1 and need an explicit table.
        if unicode_width::UnicodeWidthChar::width(c) == Some(0) {
            return true;
        }
        if is_spacing_mark_width_one(cp) {
            return true;
        }
        // Default_Ignorable_Code_Point (ZWJ, variation selectors, …).
        // Approximation: unicode-width already reports 0 for most; treat the
        // well-known ignorables explicitly for parity with the TS regex.
        matches!(cp, 0x00ad | 0x034f | 0x061c | 0x115f | 0x1160 | 0x17b4 | 0x17b5 | 0x180b..=0x180f | 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x206f | 0x3164 | 0xfe00..=0xfe0f | 0xfeff | 0xffa0 | 0xfff0..=0xfff8 | 0x1bca0..=0x1bca3 | 0x1d173..=0x1d17a | 0xe0000..=0xe0fff)
    })
}

/// Hangul jamo ranges (leading/trailing consonants + vowels) — width 1-2.
fn is_hangul_jamo(cp: u32) -> bool {
    (0x1100..=0x11ff).contains(&cp)
        || (0xa960..=0xa97f).contains(&cp)
        || (0xd7b0..=0xd7ff).contains(&cp)
}

/// Spacing marks (Mc) that unicode-width reports as width 1 but the TS
/// `\p{Mark}` classifies as zero-width. Only matters when the mark forms its
/// own grapheme cluster (line start / after ANSI stripping); inside a cluster
/// both references agree.
fn is_spacing_mark_width_one(cp: u32) -> bool {
    // Common Indic vowel signs (Devanagari, Bengali, Gurmukhi, Gujarati,
    // Oriya, Tamil, Telugu, Kannada, Malayalam, Sinhala, Thai/Lao spacing
    // marks, Myanmar, Khmer, Tibetan, Balinese, Javanese…). This is an
    // explicit subset of the Mc category that unicode-width scores 1; the
    // full 238-char table is tracked as a known-limitation in the review
    // record. The most common cases (e.g. U+093E) are covered here.
    matches!(cp,
        0x093e..=0x094f | 0x0955..=0x0957 | 0x0962 | 0x0963 |
        0x09be..=0x09c4 | 0x09c7 | 0x09c8 | 0x09cb..=0x09cd | 0x09d7 | 0x09e2 | 0x09e3 |
        0x0a3e..=0x0a42 | 0x0a47 | 0x0a48 | 0x0a4b..=0x0a4d | 0x0a51 | 0x0a70 | 0x0a71 |
        0x0abe..=0x0ac5 | 0x0ac7..=0x0ac9 | 0x0acb..=0x0acd |
        0x0b3e..=0x0b44 | 0x0b47 | 0x0b48 | 0x0b4b..=0x0b4d | 0x0b56 | 0x0b57 | 0x0b62 | 0x0b63 |
        0x0bbe..=0x0bc2 | 0x0bc6..=0x0bc8 | 0x0bca..=0x0bcd | 0x0bd7 |
        0x0c3e..=0x0c44 | 0x0c46..=0x0c48 | 0x0c4a..=0x0c4d | 0x0c55 | 0x0c56 | 0x0c62 | 0x0c63 |
        0x0cbe..=0x0cc4 | 0x0cc6..=0x0cc8 | 0x0cca..=0x0ccd | 0x0cd5 | 0x0cd6 | 0x0ce2 | 0x0ce3 |
        0x0d3e..=0x0d44 | 0x0d46..=0x0d48 | 0x0d4a..=0x0d4d | 0x0d57 | 0x0d62 | 0x0d63 |
        0x0dd2..=0x0dd6 | 0x0ddf |
        0x0e31 | 0x0e34..=0x0e3a | 0x0e47..=0x0e4e |
        0x0eb1 | 0x0eb4..=0x0ebc | 0x0ec8..=0x0ecd |
        0x102b..=0x103e | 0x1056 | 0x1057 | 0x1062..=0x1064 | 0x1067..=0x106d | 0x1083..=0x1086 |
        0x17b6..=0x17d3 | 0x17dd |
        0x1a55..=0x1a5e | 0x1a61..=0x1a74 |
        0x1b00..=0x1b04 | 0x1b34..=0x1b44 | 0x1b6b..=0x1b73 |
        0xa981..=0xa983 | 0xa9b4..=0xa9bf | 0xaa2f..=0xaa36 | 0xaa43 | 0xaa4c | 0xaa4d
    )
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
/// width plus `\p{Mark}` zero-width classification. We approximate with the
/// same `couldBeEmoji` pre-filter plus explicit rules for the observable
/// parity surface: VS16 → emoji presentation 2, regional indicators 2,
/// halfwidth voiced marks (FF9E/FF9F) and isolated Hangul jamo (1100/1161)
/// track the TS widths. Known residual divergences (all in rare isolated
/// clusters, not covered by the golden corpus) are listed in the review
/// record; the common surface (©=1, ©️=2, ☀=1, ⚙️=2, ZWJ/skin-tone=2,
/// e+combining=1) is byte-exact.
pub fn grapheme_width(segment: &str) -> usize {
    if segment == "\t" {
        return 3;
    }
    if is_zero_width_segment(segment) {
        return 0;
    }
    let base = segment.chars().next().unwrap_or_default();
    let cp = base as u32;

    // Emoji presentation via VS16, or an emoji-block char that is wide in
    // the EAW table (☀ U+2600 is EAW ambiguous → text presentation = 1,
    // matching the TS `eastAsianWidth` result; © same).
    if could_be_emoji(segment) && (segment.contains('\u{fe0f}') || base.width() == Some(2)) {
        return 2;
    }
    // Multi-codepoint emoji (ZWJ sequences, skin tones) are always 2.
    if could_be_emoji(segment) && segment.chars().count() > 1 && !segment.contains('\u{fe0f}') {
        let all_zero_width = segment
            .chars()
            .skip(1)
            .all(|c| is_zero_width_segment(&c.to_string()));
        if !all_zero_width {
            return 2;
        }
    }

    let mut width = base.width().unwrap_or(0);

    // Regional indicator symbols (U+1F1E6..U+1F1FF) render full-width in most
    // terminals even when isolated during streaming; keep width conservative
    // (2) to avoid auto-wrap drift artifacts.
    if (0x1f1e6..=0x1f1ff).contains(&cp) {
        width = 2;
    }
    // Isolated Hangul jamo: TS eastAsianWidth reports leading consonants
    // (U+1100-115F) as 2 and vowels (U+1161-11A7) as 1.
    if (0x1100..=0x115f).contains(&cp) {
        width = 2;
    } else if (0x1161..=0x11a7).contains(&cp) {
        width = 1;
    }

    // Trailing halfwidth/fullwidth forms and Thai/Lao AM vowels that segment
    // with a base. Halfwidth voiced marks (FF9E/FF9F) are checked BEFORE the
    // generic 0xFF00-0xFFEF range because unicode-width scores them 0 while
    // the TS reference reports 1 (a standalone ｶﾞ must be 2).
    for c in segment.chars().skip(1) {
        let ccp = c as u32;
        if ccp == 0xff9e || ccp == 0xff9f {
            width += 1;
        } else if (0xff00..=0xffef).contains(&ccp) {
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

#[cfg(test)]
mod parity_tests {
    use super::*;

    /// Regression tests for the review-verified parity surface — each case is
    /// pinned against the TS `visibleWidth` probe.
    #[test]
    fn halfwidth_voiced_katakana() {
        assert_eq!(visible_width("ｶﾞ"), 2); // U+FF76 + U+FF9E
    }

    #[test]
    fn thai_combining_is_zero() {
        assert_eq!(visible_width("ก่"), 1); // U+0E01 + U+0E48
    }

    #[test]
    fn emoji_zwj_family() {
        assert_eq!(visible_width("👨\u{200d}👩\u{200d}👧"), 2);
    }

    #[test]
    fn emoji_presentation_via_vs16() {
        assert_eq!(visible_width("©"), 1);
        assert_eq!(visible_width("©\u{fe0f}"), 2);
        assert_eq!(visible_width("☀"), 1); // EAW ambiguous → text presentation
        assert_eq!(visible_width("☀\u{fe0f}"), 2);
        assert_eq!(visible_width("⚙"), 1);
        assert_eq!(visible_width("⚙\u{fe0f}"), 2);
    }

    #[test]
    fn regional_indicator() {
        assert_eq!(visible_width("🇺"), 2);
    }

    #[test]
    fn combining_acute() {
        assert_eq!(visible_width("e\u{0301}"), 1);
    }

    #[test]
    fn devanagari_isolated_vowel_sign() {
        assert_eq!(visible_width("\u{093e}"), 0); // TS reports Mark → 0
    }

    #[test]
    fn hangul_jamo_isolated() {
        assert_eq!(visible_width("\u{1100}\u{1161}"), 2); // 가 syllable
        assert_eq!(visible_width("\u{1100}"), 2); // isolated leading consonant
        assert_eq!(visible_width("\u{1161}"), 1); // isolated vowel (TS: 1)
    }
}
