//! Minimal interactive shell for the Rust TUI — wires `ProcessTerminal` +
//! `Tui` + `CustomEditor` into a running event loop.
//!
//! This is the application-shell prototype (slice 6): raw mode, Kitty
//! negotiation, an editor with the dimi prompt/box, and Enter-submit that
//! prints the submitted line. It runs on a real TTY; run with:
//! `cargo run -p dimi-tui --example shell`

use std::cell::RefCell;
use std::io::{IsTerminal, Read};
use std::rc::Rc;

use dimi_tui::custom_editor::{CustomEditor, CustomEditorCallbacks};
use dimi_tui::editor::EditorOptions;
use dimi_tui::process_terminal::ProcessTerminal;
use dimi_tui::terminal::Terminal;
use dimi_tui::theme::{DARK_COLORS, set_palette};
use dimi_tui::tui::Tui;

/// Shares the ProcessTerminal between the Tui (as its Terminal) and the demo
/// event loop (which drives stdin through the buffer).
struct SharedTerminal {
    inner: Rc<RefCell<ProcessTerminal>>,
}

impl Terminal for SharedTerminal {
    fn start(&mut self, on_input: &mut dyn FnMut(&str), on_resize: &mut dyn FnMut()) {
        self.inner.borrow_mut().start(on_input, on_resize);
    }
    fn stop(&mut self) {
        self.inner.borrow_mut().stop();
    }
    fn write(&mut self, data: &str) {
        self.inner.borrow_mut().write(data);
    }
    fn columns(&self) -> usize {
        self.inner.borrow().columns()
    }
    fn rows(&self) -> usize {
        self.inner.borrow().rows()
    }
    fn hide_cursor(&mut self) {
        self.inner.borrow_mut().hide_cursor();
    }
    fn show_cursor(&mut self) {
        self.inner.borrow_mut().show_cursor();
    }
    fn kitty_protocol_active(&self) -> bool {
        self.inner.borrow().kitty_protocol_active()
    }
    fn move_by(&mut self, lines: isize) {
        self.inner.borrow_mut().move_by(lines);
    }
    fn clear_line(&mut self) {
        self.inner.borrow_mut().clear_line();
    }
    fn clear_from_cursor(&mut self) {
        self.inner.borrow_mut().clear_from_cursor();
    }
    fn clear_screen(&mut self) {
        self.inner.borrow_mut().clear_screen();
    }
    fn set_title(&mut self, title: &str) {
        self.inner.borrow_mut().set_title(title);
    }
    fn set_progress(&mut self, active: bool) {
        self.inner.borrow_mut().set_progress(active);
    }
    fn drain_input(&mut self, max_ms: u64, idle_ms: u64) {
        self.inner.borrow_mut().drain_input(max_ms, idle_ms);
    }
}

fn main() {
    if !std::io::stdin().is_terminal() {
        eprintln!("example `shell` requires a TTY");
        std::process::exit(1);
    }
    set_palette(DARK_COLORS);

    let term = Rc::new(RefCell::new(ProcessTerminal::new()));
    let mut tui = Tui::new(Box::new(SharedTerminal {
        inner: term.clone(),
    }));
    tui.set_clear_on_shrink(true);

    // Build the editor with app callbacks.
    let editor = CustomEditor::new(
        EditorOptions { padding_x: 4 },
        CustomEditorCallbacks {
            on_escape: Some(Box::new(|| {
                std::process::exit(0);
            })),
            on_ctrl_c: Some(Box::new(|| {
                std::process::exit(130);
            })),
            ..Default::default()
        },
    );

    tui.add_child(Box::new(editor));
    tui.set_focus(Some(0));

    tui.start();

    // Event loop: read stdin chunks, route through the terminal's stdin
    // buffer (Kitty negotiation + sequence splitting) to the TUI, re-render
    // on resize, exit on EOF.
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 4096];
    loop {
        if term.borrow_mut().take_resize_pending() {
            tui.request_render();
        }
        match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                // Route complete sequences (after Kitty negotiation + buffer
                // splitting) into the TUI input handler.
                let mut on_input = |data: &str| {
                    tui.handle_input(data);
                };
                term.borrow_mut().process_stdin_chunk(&chunk, &mut on_input);
                tui.request_render();
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }

    tui.stop();
    eprintln!("\n[shell exited]");
}
