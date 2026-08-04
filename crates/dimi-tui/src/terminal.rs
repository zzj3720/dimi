//! Terminal abstraction — the pi-tui `Terminal` interface, ported.
//!
//! This first slice ships a recording terminal for tests and a minimal
//! stdout terminal for the demo. Full raw-mode input handling (Kitty
//! keyboard protocol negotiation, stdin buffer splitting, bracketed paste)
//! lands with the editor slice.

use std::io::Write;

/// Terminal interface — the TUI writes frames through this and reads size.
pub trait Terminal {
    /// Start the terminal with input and resize handlers.
    fn start(&mut self, on_input: &mut dyn FnMut(&str), on_resize: &mut dyn FnMut());

    /// Stop the terminal and restore state.
    fn stop(&mut self);

    /// Write output to the terminal.
    fn write(&mut self, data: &str);

    /// Terminal dimensions.
    fn columns(&self) -> usize;
    fn rows(&self) -> usize;

    /// Cursor visibility.
    fn hide_cursor(&mut self);
    fn show_cursor(&mut self);
}

/// Terminal that records every write for assertions. Mirrors pi-tui's
/// `LoggingVirtualTerminal` used across its test suite.
#[derive(Default)]
pub struct RecordingTerminal {
    pub writes: Vec<String>,
    pub columns_value: usize,
    pub rows_value: usize,
}

impl RecordingTerminal {
    pub fn new(columns: usize, rows: usize) -> Self {
        RecordingTerminal {
            writes: Vec::new(),
            columns_value: columns,
            rows_value: rows,
        }
    }

    /// Concatenated output of every write so far.
    pub fn output(&self) -> String {
        self.writes.concat()
    }

    pub fn clear_writes(&mut self) {
        self.writes.clear();
    }
}

impl Terminal for RecordingTerminal {
    fn start(&mut self, _on_input: &mut dyn FnMut(&str), _on_resize: &mut dyn FnMut()) {}

    fn stop(&mut self) {}

    fn write(&mut self, data: &str) {
        self.writes.push(data.to_owned());
    }

    fn columns(&self) -> usize {
        self.columns_value
    }

    fn rows(&self) -> usize {
        self.rows_value
    }

    fn hide_cursor(&mut self) {}

    fn show_cursor(&mut self) {}
}

/// Minimal stdout terminal. `start`/`stop` switch raw mode via termios
/// (stdin), writes go to stdout. Keyboard input reading is not wired yet.
pub struct StdoutTerminal {
    raw_mode_enabled: bool,
}

impl StdoutTerminal {
    pub fn new() -> Self {
        StdoutTerminal {
            raw_mode_enabled: false,
        }
    }
}

impl Default for StdoutTerminal {
    fn default() -> Self {
        Self::new()
    }
}

impl StdoutTerminal {
    fn set_raw_mode(&mut self, raw: bool) {
        use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};
        use std::io::IsTerminal;

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
        // Best-effort: raw mode failure must not crash the renderer.
        if result.is_ok() {
            self.raw_mode_enabled = raw;
        }
    }
}

impl Terminal for StdoutTerminal {
    fn start(&mut self, _on_input: &mut dyn FnMut(&str), _on_resize: &mut dyn FnMut()) {
        self.set_raw_mode(true);
    }

    fn stop(&mut self) {
        if self.raw_mode_enabled {
            self.set_raw_mode(false);
        }
    }

    fn write(&mut self, data: &str) {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(data.as_bytes());
        let _ = stdout.flush();
    }

    fn columns(&self) -> usize {
        terminal_size().0
    }

    fn rows(&self) -> usize {
        terminal_size().1
    }

    fn hide_cursor(&mut self) {
        self.write("\x1b[?25l");
    }

    fn show_cursor(&mut self) {
        self.write("\x1b[?25h");
    }
}

/// Read the terminal size: ioctl TIOCGWINSZ via the `terminal_size` crate
/// (safe API), then `COLUMNS`/`LINES`, then 80x24.
fn terminal_size() -> (usize, usize) {
    if let Some((cols, rows)) = terminal_size::terminal_size() {
        let (cols, rows) = (cols.0 as usize, rows.0 as usize);
        if cols > 0 && rows > 0 {
            return (cols, rows);
        }
    }
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(80);
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(24);
    (cols, rows)
}
