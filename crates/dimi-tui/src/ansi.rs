//! ANSI escape sequence handling — ported from `@dimi-agent/pi-tui`
//! `src/utils.ts` (sequence extraction, stripping, terminal-output
//! normalization) plus shared output constants.

/// Trailing reset appended to every processed line. SGR reset plus an OSC 8
/// hyperlink close so styles and links cannot leak past the line.
pub const SEGMENT_RESET: &str = "\x1b[0m\x1b]8;;\x07";

/// Extract one ANSI/OSC/APC escape sequence starting at `pos`, or `None` when
/// `str[pos]` is not an escape start or the sequence is unterminated.
///
/// Mirrors `extractAnsiCode`:
/// - CSI: `ESC [ ... m/G/K/H/J`
/// - OSC: `ESC ] ... BEL` or `ESC ] ... ST`
/// - APC: `ESC _ ... BEL` or `ESC _ ... ST`
pub fn extract_ansi_code(s: &str, pos: usize) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    if pos >= bytes.len() || bytes[pos] != 0x1b {
        return None;
    }
    let next = *bytes.get(pos + 1)?;
    let mut j = pos + 2;
    match next {
        // CSI sequence
        b'[' => {
            while j < bytes.len() && !matches!(bytes[j], b'm' | b'G' | b'K' | b'H' | b'J') {
                j += 1;
            }
            if j < bytes.len() {
                return Some((s[pos..=j].to_owned(), j + 1 - pos));
            }
            None
        }
        // OSC sequence (hyperlinks, titles, …)
        b']' => {
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    return Some((s[pos..=j].to_owned(), j + 1 - pos));
                }
                if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                    return Some((s[pos..=j + 1].to_owned(), j + 2 - pos));
                }
                j += 1;
            }
            None
        }
        // APC sequence (CURSOR_MARKER, application commands)
        b'_' => {
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    return Some((s[pos..=j].to_owned(), j + 1 - pos));
                }
                if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                    return Some((s[pos..=j + 1].to_owned(), j + 2 - pos));
                }
                j += 1;
            }
            None
        }
        _ => None,
    }
}

/// Strip all ANSI/OSC/APC escape sequences from `s`.
pub fn strip_ansi(s: &str) -> String {
    let mut stripped = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if let Some((_, len)) = extract_ansi_code(s, i) {
            i += len;
            continue;
        }
        let c = s[i..].chars().next().unwrap_or_default();
        stripped.push(c);
        i += c.len_utf8();
    }
    stripped
}

/// Normalize text for terminal output without changing logical editor content.
/// Some terminals render precomposed Thai/Lao AM vowels inconsistently during
/// differential repaint; their compatibility decompositions have the same cell
/// width but avoid stale-cell artifacts. Mirrors `normalizeTerminalOutput`.
pub fn normalize_terminal_output(s: &str) -> String {
    if !s.contains('\u{0e33}') && !s.contains('\u{0eb3}') {
        return s.to_owned();
    }
    s.chars()
        .map(|c| match c {
            '\u{0e33}' => "\u{0e4d}\u{0e32}".to_owned(),
            '\u{0eb3}' => "\u{0ecd}\u{0eb2}".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_csi_osc_apc() {
        assert_eq!(
            extract_ansi_code("\x1b[31m", 0),
            Some(("\x1b[31m".to_owned(), 5))
        );
        assert_eq!(
            extract_ansi_code("a\x1b[2Kb", 1),
            Some(("\x1b[2K".to_owned(), 4))
        );
        assert_eq!(
            extract_ansi_code("\x1b]8;;https://example.com\x07", 0),
            Some(("\x1b]8;;https://example.com\x07".to_owned(), 25))
        );
        assert_eq!(
            extract_ansi_code("\x1b]8;;x\x1b\\", 0),
            Some(("\x1b]8;;x\x1b\\".to_owned(), 8))
        );
        assert_eq!(
            extract_ansi_code("\x1b_pi:c\x07", 0),
            Some(("\x1b_pi:c\x07".to_owned(), 7))
        );
        assert_eq!(extract_ansi_code("plain", 0), None);
        // Unterminated CSI is not a complete sequence.
        assert_eq!(extract_ansi_code("\x1b[31", 0), None);
    }

    #[test]
    fn strip_ansi_removes_sequences() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(
            strip_ansi("\x1b]8;;https://example.com\x07link\x1b]8;;\x07"),
            "link"
        );
        assert_eq!(strip_ansi("\x1b_pi:c\x07x"), "x");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn normalize_thai_lao_am() {
        assert_eq!(normalize_terminal_output("plain"), "plain");
        assert_eq!(normalize_terminal_output("\u{0e33}"), "\u{0e4d}\u{0e32}");
        assert_eq!(normalize_terminal_output("\u{0eb3}"), "\u{0ecd}\u{0eb2}");
    }
}
