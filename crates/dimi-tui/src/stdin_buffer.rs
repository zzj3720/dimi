//! `StdinBuffer` — buffers stdin input and emits complete sequences
//! (port of `@dimi-agent/pi-tui` `src/stdin-buffer.ts`).
//!
//! stdin data events can arrive in partial chunks, especially for escape
//! sequences; without buffering, partial sequences would be misinterpreted as
//! regular keypresses. The buffer accumulates input until a complete sequence
//! is detected, splits bracketed paste content, dedupes Kitty CSI-u printable
//! sequences, and flushes incomplete tails after a timeout.

const ESC: char = '\x1b';
const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceStatus {
    Complete,
    Incomplete,
    NotEscape,
}

/// Whether `data` is a complete escape sequence or needs more data.
fn is_complete_sequence(data: &str) -> SequenceStatus {
    if !data.starts_with(ESC) {
        return SequenceStatus::NotEscape;
    }
    if data.chars().count() == 1 {
        return SequenceStatus::Incomplete;
    }
    let after_esc = &data[1..];

    // CSI sequences: ESC [
    if after_esc.starts_with('[') {
        // Old-style mouse: ESC[M + 3 bytes = 6 total.
        if after_esc.starts_with("[M") {
            return if data.chars().count() >= 6 {
                SequenceStatus::Complete
            } else {
                SequenceStatus::Incomplete
            };
        }
        return is_complete_csi_sequence(data);
    }
    // OSC sequences: ESC ]
    if after_esc.starts_with(']') {
        return is_complete_osc_sequence(data);
    }
    // DCS sequences: ESC P ... ST (XTVersion responses).
    if after_esc.starts_with('P') {
        return is_complete_dcs_sequence(data);
    }
    // APC sequences: ESC _ ... ST (Kitty graphics responses).
    if after_esc.starts_with('_') {
        return is_complete_apc_sequence(data);
    }
    // SS3 sequences: ESC O followed by a single character.
    if after_esc.starts_with('O') {
        return if after_esc.chars().count() >= 2 {
            SequenceStatus::Complete
        } else {
            SequenceStatus::Incomplete
        };
    }
    // Meta key sequences: ESC followed by a single character.
    if after_esc.chars().count() == 1 {
        return SequenceStatus::Complete;
    }
    // Unknown escape sequence — treat as complete.
    SequenceStatus::Complete
}

/// CSI sequences end with a final byte in 0x40..=0x7E (@-~).
fn is_complete_csi_sequence(data: &str) -> SequenceStatus {
    if !data.starts_with('\x1b') || !data[1..].starts_with('[') {
        return SequenceStatus::Complete;
    }
    if data.chars().count() < 3 {
        return SequenceStatus::Incomplete;
    }
    let payload = &data[2..];
    let last_char = payload.chars().last().unwrap_or_default();
    let last_code = last_char as u32;

    if (0x40..=0x7e).contains(&last_code) {
        // SGR mouse: ESC[<B;X;Ym or ESC[<B;X;YM — needs 3 numeric parts.
        if payload.starts_with('<') {
            let body = &payload[1..payload.chars().count() - 1];
            let parts: Vec<&str> = body.split(';').collect();
            let all_digits = parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
            if all_digits && parts.len() == 3 {
                return SequenceStatus::Complete;
            }
            return SequenceStatus::Incomplete;
        }
        return SequenceStatus::Complete;
    }
    SequenceStatus::Incomplete
}

/// OSC sequences end with ST (ESC \) or BEL (\x07).
fn is_complete_osc_sequence(data: &str) -> SequenceStatus {
    if !data.starts_with('\x1b') || !data[1..].starts_with(']') {
        return SequenceStatus::Complete;
    }
    if data.ends_with("\x1b\\") || data.ends_with('\x07') {
        SequenceStatus::Complete
    } else {
        SequenceStatus::Incomplete
    }
}

/// DCS sequences end with ST (ESC \).
fn is_complete_dcs_sequence(data: &str) -> SequenceStatus {
    if !data.starts_with('\x1b') || !data[1..].starts_with('P') {
        return SequenceStatus::Complete;
    }
    if data.ends_with("\x1b\\") {
        SequenceStatus::Complete
    } else {
        SequenceStatus::Incomplete
    }
}

/// APC sequences end with ST (ESC \).
fn is_complete_apc_sequence(data: &str) -> SequenceStatus {
    if !data.starts_with('\x1b') || !data[1..].starts_with('_') {
        return SequenceStatus::Complete;
    }
    if data.ends_with("\x1b\\") {
        SequenceStatus::Complete
    } else {
        SequenceStatus::Incomplete
    }
}

/// Parse an unmodified Kitty printable codepoint: `ESC[<codepoint>u` with no
/// modifier (or empty modifier colons). Returns the codepoint when ≥ 32.
fn parse_unmodified_kitty_printable_codepoint(sequence: &str) -> Option<u32> {
    // ^\x1b\[(\d+)(?::\d*)?(?::\d+)?u$
    let rest = sequence.strip_prefix("\x1b[")?;
    let rest = rest.strip_suffix('u')?;
    let mut parts = rest.split(':');
    let codepoint = parts.next()?.parse::<u32>().ok()?;
    if codepoint < 32 {
        return None;
    }
    Some(codepoint)
}

/// Split `buffer` into complete sequences + remainder.
fn extract_complete_sequences(buffer: &str) -> (Vec<String>, String) {
    let mut sequences: Vec<String> = Vec::new();
    let mut pos = 0usize;

    while pos < buffer.len() {
        let remaining = &buffer[pos..];
        if remaining.starts_with(ESC) {
            let mut seq_end = 1usize;
            let mut found = false;
            while seq_end <= remaining.len() {
                let candidate = &remaining[..seq_end];
                let status = is_complete_sequence(candidate);

                match status {
                    SequenceStatus::Complete => {
                        // WezTerm sends Escape key press as raw '\x1b' and the
                        // release as a full CSI-u, arriving as '\x1b\x1b[27;...u'.
                        // If the char after '\x1b\x1b' starts a new escape
                        // sequence, emit only the first ESC and restart.
                        if candidate == "\x1b\x1b" {
                            let next_char = remaining[seq_end..].chars().next();
                            if matches!(
                                next_char,
                                Some('[') | Some(']') | Some('O') | Some('P') | Some('_')
                            ) {
                                sequences.push(ESC.to_string());
                                pos += 1;
                                found = true;
                                break;
                            }
                        }
                        sequences.push(candidate.to_owned());
                        pos += seq_end;
                        found = true;
                        break;
                    }
                    SequenceStatus::Incomplete => {
                        seq_end += 1;
                    }
                    SequenceStatus::NotEscape => {
                        sequences.push(candidate.to_owned());
                        pos += seq_end;
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                // Ran past the end — the whole remainder is incomplete.
                return (sequences, remaining.to_owned());
            }
        } else {
            // Not an escape sequence — take a single character.
            let c = remaining.chars().next().unwrap_or_default();
            sequences.push(c.to_string());
            pos += c.len_utf8();
        }
    }

    (sequences, String::new())
}

/// Callbacks for the stdin buffer.
pub trait StdinBufferSink {
    /// A complete input sequence.
    fn on_data(&mut self, sequence: &str);
    /// A bracketed-paste payload.
    fn on_paste(&mut self, content: &str);
}

/// Buffers stdin input and emits complete sequences.
pub struct StdinBuffer {
    buffer: String,
    timeout_ms: u64,
    paste_mode: bool,
    paste_buffer: String,
    pending_kitty_printable_codepoint: Option<u32>,
}

impl StdinBuffer {
    pub fn new(timeout_ms: u64) -> Self {
        StdinBuffer {
            buffer: String::new(),
            timeout_ms,
            paste_mode: false,
            paste_buffer: String::new(),
            pending_kitty_printable_codepoint: None,
        }
    }

    /// Feed stdin data; emits sequences via `sink` (paste payloads via
    /// `on_paste`, everything else via `on_data`).
    ///
    /// `now` is the caller's monotonic clock (ms) used for the flush timeout;
    /// returns the next deadline (ms) when an incomplete tail is buffered.
    pub fn process<S: StdinBufferSink>(
        &mut self,
        data: &str,
        sink: &mut S,
        now: u64,
    ) -> Option<u64> {
        if data.is_empty() && self.buffer.is_empty() {
            sink.on_data("");
            return None;
        }

        self.buffer.push_str(data);

        if self.paste_mode {
            self.paste_buffer.push_str(&self.buffer);
            self.buffer.clear();
            if let Some(end_index) = self.paste_buffer.find(BRACKETED_PASTE_END) {
                let pasted_content = self.paste_buffer[..end_index].to_owned();
                let remaining =
                    self.paste_buffer[end_index + BRACKETED_PASTE_END.len()..].to_owned();
                self.paste_mode = false;
                self.paste_buffer.clear();
                self.pending_kitty_printable_codepoint = None;
                sink.on_paste(&pasted_content);
                if !remaining.is_empty() {
                    return self.process(&remaining, sink, now);
                }
            }
            return None;
        }

        if let Some(start_index) = self.buffer.find(BRACKETED_PASTE_START) {
            if start_index > 0 {
                let before_paste = self.buffer[..start_index].to_owned();
                let (seqs, _) = extract_complete_sequences(&before_paste);
                for seq in seqs {
                    self.emit_data_sequence(&seq, sink);
                }
            }
            self.pending_kitty_printable_codepoint = None;
            self.buffer = self.buffer[start_index + BRACKETED_PASTE_START.len()..].to_owned();
            self.paste_mode = true;
            self.paste_buffer = self.buffer.clone();
            self.buffer.clear();

            if let Some(end_index) = self.paste_buffer.find(BRACKETED_PASTE_END) {
                let pasted_content = self.paste_buffer[..end_index].to_owned();
                let remaining =
                    self.paste_buffer[end_index + BRACKETED_PASTE_END.len()..].to_owned();
                self.paste_mode = false;
                self.paste_buffer.clear();
                self.pending_kitty_printable_codepoint = None;
                sink.on_paste(&pasted_content);
                if !remaining.is_empty() {
                    return self.process(&remaining, sink, now);
                }
            }
            return None;
        }

        let (seqs, remainder) = extract_complete_sequences(&self.buffer);
        self.buffer = remainder;
        for seq in &seqs {
            self.emit_data_sequence(seq, sink);
        }

        if !self.buffer.is_empty() {
            Some(now + self.timeout_ms)
        } else {
            None
        }
    }

    /// Flush an incomplete tail (called on the flush deadline).
    pub fn flush<S: StdinBufferSink>(&mut self, sink: &mut S) -> Vec<String> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let sequences = vec![self.buffer.clone()];
        self.buffer.clear();
        self.pending_kitty_printable_codepoint = None;
        for seq in &sequences {
            sink.on_data(seq);
        }
        sequences
    }

    fn emit_data_sequence<S: StdinBufferSink>(&mut self, sequence: &str, sink: &mut S) {
        let raw_codepoint = if sequence.chars().count() == 1 {
            sequence.chars().next().map(|c| c as u32)
        } else {
            None
        };
        if let Some(cp) = raw_codepoint {
            if self.pending_kitty_printable_codepoint == Some(cp) {
                self.pending_kitty_printable_codepoint = None;
                return;
            }
        }
        self.pending_kitty_printable_codepoint =
            parse_unmodified_kitty_printable_codepoint(sequence);
        sink.on_data(sequence);
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.paste_mode = false;
        self.paste_buffer.clear();
        self.pending_kitty_printable_codepoint = None;
    }

    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingSink {
        data: Vec<String>,
        pastes: Vec<String>,
    }
    impl RecordingSink {
        fn new() -> Self {
            RecordingSink {
                data: Vec::new(),
                pastes: Vec::new(),
            }
        }
    }
    impl StdinBufferSink for RecordingSink {
        fn on_data(&mut self, sequence: &str) {
            self.data.push(sequence.to_owned());
        }
        fn on_paste(&mut self, content: &str) {
            self.pastes.push(content.to_owned());
        }
    }

    #[test]
    fn plain_chars_emit_individually() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        b.process("abc", &mut s, 0);
        assert_eq!(s.data, vec!["a", "b", "c"]);
        assert!(b.get_buffer().is_empty());
    }

    #[test]
    fn partial_csi_buffered_then_flushed() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        // '\x1b' alone is incomplete.
        let deadline = b.process("\x1b", &mut s, 100);
        assert!(s.data.is_empty());
        assert!(deadline.is_some());
        assert_eq!(b.get_buffer(), "\x1b");
        // Complete the CSI sequence.
        b.process("[A", &mut s, 101);
        assert_eq!(s.data, vec!["\x1b[A"]);
        assert!(b.get_buffer().is_empty());
    }

    #[test]
    fn complete_csi_emits_immediately() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        b.process("\x1b[A", &mut s, 0);
        assert_eq!(s.data, vec!["\x1b[A"]);
    }

    #[test]
    fn bracketed_paste_extracts_payload() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        b.process("\x1b[200~pasted \x1b[201~", &mut s, 0);
        assert_eq!(s.pastes, vec!["pasted "]);
        assert!(s.data.is_empty());
    }

    #[test]
    fn paste_content_not_parsed_as_keys() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        // The paste payload contains escape-looking text; it must come through
        // as a single paste, not as parsed sequences.
        b.process("\x1b[200~line1\nline2\x1b[201~", &mut s, 0);
        assert_eq!(s.pastes, vec!["line1\nline2"]);
        assert!(s.data.is_empty());
    }

    #[test]
    fn paste_split_across_chunks() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        b.process("\x1b[200~hel", &mut s, 0);
        assert!(s.pastes.is_empty());
        b.process("lo\x1b[201~", &mut s, 1);
        assert_eq!(s.pastes, vec!["hello"]);
    }

    #[test]
    fn kitty_printable_dedup() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        // Some terminals send the Kitty CSI-u for a key, then a raw duplicate
        // character; the duplicate printable must be suppressed.
        b.process("\x1b[97ua", &mut s, 0);
        assert_eq!(s.data, vec!["\x1b[97u"]);
    }

    #[test]
    fn double_esc_followed_by_csi_splits() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        // '\x1b\x1b[27;...u' — the first ESC is a meta prefix, the second
        // starts a CSI sequence.
        b.process("\x1b\x1b[27;1u", &mut s, 0);
        assert_eq!(s.data, vec!["\x1b", "\x1b[27;1u"]);
    }

    #[test]
    fn incomplete_tail_flushes_on_deadline() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        b.process("\x1b[1;5", &mut s, 0);
        assert!(s.data.is_empty());
        let flushed = b.flush(&mut s);
        assert_eq!(flushed, vec!["\x1b[1;5"]);
        assert_eq!(s.data, vec!["\x1b[1;5"]);
    }

    #[test]
    fn mouse_sgr_sequence() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        // Split mouse SGR sequence arrives in pieces.
        b.process("\x1b[<35;20;5", &mut s, 0);
        assert!(s.data.is_empty());
        b.process("m", &mut s, 1);
        assert_eq!(s.data, vec!["\x1b[<35;20;5m"]);
    }

    #[test]
    fn clear_resets_state() {
        let mut b = StdinBuffer::new(10);
        let mut s = RecordingSink::new();
        b.process("\x1b[200~x", &mut s, 0);
        b.clear();
        assert!(b.get_buffer().is_empty());
        assert!(!b.paste_mode);
    }
}
