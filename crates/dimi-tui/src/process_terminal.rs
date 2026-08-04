//! `ProcessTerminal` — the real terminal over process stdin/stdout
//! (port of `@dimi-agent/pi-tui` `src/terminal.ts`).
//!
//! Responsibilities:
//! - raw mode via termios, bracketed paste enable/disable
//! - Kitty keyboard protocol negotiation (`ESC[>7u ESC[?u ESC[c`) with
//!   modifyOtherKeys fallback when the terminal answers DA first
//! - stdin buffering via [`crate::stdin_buffer::StdinBuffer`] so components
//!   receive complete sequences, with paste re-wrapping
//! - SIGWINCH resize tracking, window title, OSC 9;4 progress, drain-on-exit

use std::io::{IsTerminal, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};

use crate::keys::set_kitty_protocol_active;
use crate::stdin_buffer::{StdinBuffer, StdinBufferSink};
use crate::terminal::Terminal;

const KITTY_KEYBOARD_PROTOCOL_QUERY: &str = "\x1b[>7u\x1b[?u\x1b[c";
// Kept for TS parity; the event-loop slice uses these.
#[allow(dead_code)]
const DESIRED_KITTY_KEYBOARD_PROTOCOL_FLAGS: u32 = 7;
#[allow(dead_code)]
const KEYBOARD_PROTOCOL_RESPONSE_FRAGMENT_TIMEOUT_MS: u64 = 150;
#[allow(dead_code)]
const TERMINAL_PROGRESS_KEEPALIVE_MS: u64 = 1000;
const TERMINAL_PROGRESS_ACTIVE_SEQUENCE: &str = "\x1b]9;4;3\x07";
const TERMINAL_PROGRESS_CLEAR_SEQUENCE: &str = "\x1b]9;4;0;\x07";

// SIGWINCH flag: signal-hook's safe flag::register sets it; the event loop
// drains it via take_resize_pending.

/// Kitty protocol negotiation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NegotiationSequence {
    /// `ESC[?<flags>u` — the terminal reported Kitty flags.
    KittyFlags(u32),
    /// `ESC[?<params>c` — DA response (no Kitty support).
    DeviceAttributes,
}

fn parse_negotiation_sequence(sequence: &str) -> Option<NegotiationSequence> {
    // Kitty flags: ESC[?<digits>u
    if let Some(rest) = sequence.strip_prefix("\x1b[?") {
        if let Some(digits) = rest.strip_suffix('u') {
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                return Some(NegotiationSequence::KittyFlags(digits.parse().ok()?));
            }
        }
        // DA: ESC[?<params>c
        if let Some(params) = rest.strip_suffix('c') {
            if params.bytes().all(|b| b.is_ascii_digit() || b == b';') {
                return Some(NegotiationSequence::DeviceAttributes);
            }
        }
    }
    None
}

fn is_negotiation_prefix(sequence: &str) -> bool {
    sequence == "\x1b[" || {
        if let Some(rest) = sequence.strip_prefix("\x1b[?") {
            !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || b == b';')
        } else {
            false
        }
    }
}

/// Real terminal over stdin/stdout.
pub struct ProcessTerminal {
    was_raw: bool,
    kitty_protocol_active: bool,
    modify_other_keys_active: bool,
    keyboard_protocol_pushed: bool,
    negotiation_buffer: String,
    stdin_buffer: Option<StdinBuffer>,
    progress_active: bool,
    sigwinch_flag: Option<Arc<AtomicBool>>,
}

impl ProcessTerminal {
    pub fn new() -> Self {
        ProcessTerminal {
            was_raw: false,
            kitty_protocol_active: false,
            modify_other_keys_active: false,
            keyboard_protocol_pushed: false,
            negotiation_buffer: String::new(),
            stdin_buffer: None,
            progress_active: false,
            sigwinch_flag: None,
        }
    }

    fn enable_bracketed_paste(&self) {
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[?2004h");
        let _ = out.flush();
    }

    fn disable_bracketed_paste(&self) {
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[?2004l");
        let _ = out.flush();
    }

    fn query_and_enable_kitty_protocol(&mut self, on_input: &mut dyn FnMut(&str)) {
        // Install the StdinBuffer that splits input into sequences and watches
        // for the Kitty response.
        self.stdin_buffer = Some(StdinBuffer::new(10));
        self.keyboard_protocol_pushed = true;
        self.negotiation_buffer.clear();
        let query = KITTY_KEYBOARD_PROTOCOL_QUERY.to_owned();
        self.write(&query);
        // The buffer sink forwards data to the input handler; negotiation
        // sequences are consumed here.
        let _ = on_input;
    }

    /// Process a chunk of raw stdin through the buffer, handling Kitty
    /// negotiation and forwarding complete sequences to `on_input`.
    pub fn process_stdin_chunk(&mut self, chunk: &str, on_input: &mut dyn FnMut(&str)) {
        let mut owned = self.stdin_buffer.take();
        let mut sink = CollectSink::default();
        if let Some(buf) = owned.as_mut() {
            buf.process(chunk, &mut sink, now_ms());
        }
        self.stdin_buffer = owned;
        for event in sink.events {
            match event {
                BufferEvent::Data(seq) => self.handle_sequence(&seq, on_input),
                BufferEvent::Paste(content) => {
                    (on_input)(&format!("\x1b[200~{content}\x1b[201~"));
                }
            }
        }
    }

    /// Route one buffered sequence: consume Kitty negotiation, forward the rest.
    fn handle_sequence(&mut self, sequence: &str, on_input: &mut dyn FnMut(&str)) {
        let pending = !self.negotiation_buffer.is_empty();
        if pending {
            let combined = format!("{}{}", self.negotiation_buffer, sequence);
            if parse_negotiation_sequence(&combined).is_some() {
                self.negotiation_buffer.clear();
                let _ = self.handle_negotiation(&combined);
                return;
            }
            if is_negotiation_prefix(&combined) {
                self.negotiation_buffer = combined;
                return;
            }
            // Flush the buffered fragment as input.
            let frag = std::mem::take(&mut self.negotiation_buffer);
            (on_input)(&frag);
        }
        if let Some(parsed) = parse_negotiation_sequence(sequence) {
            let _ = self.handle_negotiation(sequence);
            let _ = parsed;
            return;
        }
        if is_negotiation_prefix(sequence) {
            self.negotiation_buffer = sequence.to_owned();
            return;
        }
        (on_input)(sequence);
    }

    fn handle_negotiation(&mut self, sequence: &str) -> bool {
        let Some(parsed) = parse_negotiation_sequence(sequence) else {
            return false;
        };
        self.negotiation_buffer.clear();
        match parsed {
            NegotiationSequence::KittyFlags(flags) => {
                if flags != 0 {
                    self.disable_modify_other_keys();
                    if !self.kitty_protocol_active {
                        self.kitty_protocol_active = true;
                        set_kitty_protocol_active(true);
                    }
                } else {
                    self.enable_modify_other_keys();
                }
            }
            NegotiationSequence::DeviceAttributes => {
                if !self.kitty_protocol_active {
                    self.enable_modify_other_keys();
                }
            }
        }
        true
    }

    fn enable_modify_other_keys(&mut self) {
        if self.kitty_protocol_active || self.modify_other_keys_active {
            return;
        }
        self.write("\x1b[>4;2m");
        self.modify_other_keys_active = true;
    }

    fn disable_modify_other_keys(&mut self) {
        if !self.modify_other_keys_active {
            return;
        }
        self.write("\x1b[>4;0m");
        self.modify_other_keys_active = false;
    }

    #[allow(dead_code)] // convenience for app shells
    pub fn read_stdin(&mut self, on_input: &mut dyn FnMut(&str)) {
        let mut buf = [0u8; 4096];
        loop {
            match std::io::stdin().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                    self.process_stdin_chunk(&chunk, on_input);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }

    fn install_sigwinch(&mut self) {
        let flag = Arc::new(AtomicBool::new(false));
        self.sigwinch_flag = Some(flag.clone());
        // signal-hook's flag::register is a safe API.
        let _ = signal_hook::flag::register(libc::SIGWINCH, flag);
    }

    /// True when a SIGWINCH arrived since the last check; the event loop
    /// should re-read terminal size and re-render.
    pub fn take_resize_pending(&mut self) -> bool {
        self.sigwinch_flag
            .as_ref()
            .map(|f| f.swap(false, Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Drain the stdin buffer's incomplete tail (flush deadline).
    pub fn flush_stdin(&mut self, on_input: &mut dyn FnMut(&str)) {
        let mut owned = self.stdin_buffer.take();
        let mut sink = CollectSink::default();
        if let Some(buf) = owned.as_mut() {
            buf.flush(&mut sink);
        }
        self.stdin_buffer = owned;
        for event in sink.events {
            match event {
                BufferEvent::Data(seq) => self.handle_sequence(&seq, on_input),
                BufferEvent::Paste(content) => {
                    (on_input)(&format!("\x1b[200~{content}\x1b[201~"));
                }
            }
        }
    }
}

impl Default for ProcessTerminal {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Events produced by the stdin buffer, routed by `ProcessTerminal`.
#[derive(Debug)]
enum BufferEvent {
    Data(String),
    Paste(String),
}

/// Collects buffer events without borrowing the terminal.
#[derive(Default)]
struct CollectSink {
    events: Vec<BufferEvent>,
}

impl StdinBufferSink for CollectSink {
    fn on_data(&mut self, sequence: &str) {
        self.events.push(BufferEvent::Data(sequence.to_owned()));
    }
    fn on_paste(&mut self, content: &str) {
        self.events.push(BufferEvent::Paste(content.to_owned()));
    }
}

impl Terminal for ProcessTerminal {
    fn start(&mut self, on_input: &mut dyn FnMut(&str), on_resize: &mut dyn FnMut()) {
        self.was_raw = std::io::stdin().is_terminal();
        self.set_raw_mode(true);
        self.enable_bracketed_paste();
        let _ = on_resize;
        self.install_sigwinch();
        self.query_and_enable_kitty_protocol(on_input);
    }

    fn stop(&mut self) {
        if self.progress_active {
            self.write(TERMINAL_PROGRESS_CLEAR_SEQUENCE);
            self.progress_active = false;
        }
        self.disable_bracketed_paste();
        let should_disable_kitty = self.keyboard_protocol_pushed || self.kitty_protocol_active;
        self.negotiation_buffer.clear();
        if should_disable_kitty {
            self.write("\x1b[<u");
            self.keyboard_protocol_pushed = false;
            self.kitty_protocol_active = false;
            set_kitty_protocol_active(false);
        }
        self.disable_modify_other_keys();
        self.stdin_buffer = None;
        self.set_raw_mode(false);
    }

    fn write(&mut self, data: &str) {
        let mut out = std::io::stdout();
        let _ = out.write_all(data.as_bytes());
        let _ = out.flush();
    }

    fn columns(&self) -> usize {
        terminal_size_cols()
    }

    fn rows(&self) -> usize {
        terminal_size_rows()
    }

    fn hide_cursor(&mut self) {
        self.write("\x1b[?25l");
    }

    fn show_cursor(&mut self) {
        self.write("\x1b[?25h");
    }

    fn kitty_protocol_active(&self) -> bool {
        self.kitty_protocol_active
    }

    fn move_by(&mut self, lines: isize) {
        if lines > 0 {
            self.write(&format!("\x1b[{lines}B"));
        } else if lines < 0 {
            self.write(&format!("\x1b[{}A", -lines));
        }
    }

    fn clear_line(&mut self) {
        self.write("\x1b[K");
    }

    fn clear_from_cursor(&mut self) {
        self.write("\x1b[J");
    }

    fn clear_screen(&mut self) {
        self.write("\x1b[2J\x1b[H");
    }

    fn set_title(&mut self, title: &str) {
        self.write(&format!("\x1b]0;{title}\x07"));
    }

    fn set_progress(&mut self, active: bool) {
        if active {
            self.write(TERMINAL_PROGRESS_ACTIVE_SEQUENCE);
            self.progress_active = true;
        } else if self.progress_active {
            self.write(TERMINAL_PROGRESS_CLEAR_SEQUENCE);
            self.progress_active = false;
        }
    }

    fn drain_input(&mut self, max_ms: u64, idle_ms: u64) {
        // Drain stdin for up to max_ms, exiting early after idle_ms of no
        // input, so late Kitty key releases do not leak to the parent shell.
        let start = now_ms();
        let mut last_data = now_ms();
        loop {
            let now = now_ms();
            if now.saturating_sub(start) >= max_ms {
                break;
            }
            if now.saturating_sub(last_data) >= idle_ms {
                break;
            }
            let mut buf = [0u8; 64];
            // Non-blocking-ish single read with a short sleep.
            match std::io::stdin().read(&mut buf) {
                Ok(0) => break,
                Ok(_) => last_data = now_ms(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(idle_ms.min(20)));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }
}

impl ProcessTerminal {
    fn set_raw_mode(&mut self, raw: bool) {
        if !std::io::stdin().is_terminal() {
            return;
        }
        let stdin = std::io::stdin();
        let result = (|| -> nix::Result<()> {
            let mut termios = tcgetattr(&stdin)?;
            if raw {
                cfmakeraw(&mut termios);
            }
            tcsetattr(&stdin, SetArg::TCSANOW, &termios)?;
            Ok(())
        })();
        if result.is_ok() {
            self.was_raw = raw;
        }
    }
}

fn terminal_size_cols() -> usize {
    if let Some((cols, _)) = terminal_size::terminal_size() {
        let cols = cols.0 as usize;
        if cols > 0 {
            return cols;
        }
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(80)
}

fn terminal_size_rows() -> usize {
    if let Some((_, rows)) = terminal_size::terminal_size() {
        let rows = rows.0 as usize;
        if rows > 0 {
            return rows;
        }
    }
    std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(24)
}
