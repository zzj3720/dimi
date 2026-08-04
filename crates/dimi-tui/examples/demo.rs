//! Minimal runnable rendering skeleton (slice 1).
//!
//! Renders a welcome panel + footer with an animated spinner and clock,
//! demonstrating the differential renderer: only the changed lines are
//! rewritten each frame. Press Ctrl+C to exit (raw-mode input wiring lands in
//! a later slice, so this demo renders frames in a loop until interrupted).
//!
//! Run: `cargo run -p dimi-tui --example demo`

use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use dimi_tui::component::{CURSOR_MARKER, Component, SharedComponent};
use dimi_tui::terminal::StdoutTerminal;
use dimi_tui::theme::DARK_COLORS;
use dimi_tui::tui::Tui;
use dimi_tui::width::visible_width;

const SPINNER_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];

fn style(text: &str, hex: &str) -> String {
    format!("\x1b[38;2;{}m{}\x1b[0m", hex_to_rgb(hex), text)
}

fn hex_to_rgb(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    format!("{r};{g};{b}")
}

/// Welcome panel — mirrors the TS `WelcomeComponent` shape (title, hint).
struct WelcomeComponent {
    version: String,
    work_dir: String,
}

impl WelcomeComponent {
    fn new(version: &str, work_dir: &str) -> Self {
        WelcomeComponent {
            version: version.to_owned(),
            work_dir: work_dir.to_owned(),
        }
    }
}

impl Component for WelcomeComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let _ = width;
        let title = style("Dimi", DARK_COLORS.primary);
        let sep = style("·", DARK_COLORS.text_muted);
        let version = style(&self.version, DARK_COLORS.text_dim);
        let dir = style(&self.work_dir, DARK_COLORS.text_dim);
        vec![
            format!("  {title} {sep} {version}"),
            String::new(),
            format!(
                "  {}",
                style(
                    "Type a message to start a session. /help for commands.",
                    DARK_COLORS.text
                )
            ),
            format!("  {} {dir}", style("cwd:", DARK_COLORS.text_muted)),
            String::new(),
        ]
    }
}

/// Footer — mirrors the TS `FooterComponent` shape: model · permission ·
/// mode, plus a cursor marker on the input line.
struct FooterComponent {
    model: String,
    permission: String,
    tick: usize,
}

impl FooterComponent {
    fn new(model: &str, permission: &str) -> Self {
        FooterComponent {
            model: model.to_owned(),
            permission: permission.to_owned(),
            tick: 0,
        }
    }

    fn advance(&mut self) {
        self.tick += 1;
    }
}

impl Component for FooterComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let spinner = SPINNER_FRAMES[self.tick % SPINNER_FRAMES.len()];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let clock = format!("{}s", now % 1000);
        let model = style(&self.model, DARK_COLORS.primary);
        let permission = style(&self.permission, DARK_COLORS.success);
        let spinner_text = style(spinner, DARK_COLORS.accent);
        let clock_text = style(&clock, DARK_COLORS.text_muted);
        let status = format!("{model} · {permission} · {spinner_text} · {clock_text}");
        vec![
            // Input line with a cursor marker at the end.
            format!("> {CURSOR_MARKER}"),
            pad_to_width(&status, width),
        ]
    }
}

fn pad_to_width(line: &str, width: usize) -> String {
    let w = visible_width(line);
    if w >= width {
        line.to_owned()
    } else {
        format!("{line}{}", " ".repeat(width - w))
    }
}

fn main() {
    let mut tui = Tui::new(Box::new(StdoutTerminal::new()));
    tui.add_child(Box::new(WelcomeComponent::new(
        "0.1.0-rust",
        "/Users/zuozijian/projects/k-3720",
    )));
    let footer = Rc::new(RefCell::new(FooterComponent::new("deepseek", "manual")));
    tui.add_child(Box::new(SharedComponent::new(footer.clone())));

    tui.start();

    // Render frames until interrupted. The footer's spinner/clock change each
    // frame, so the differential renderer only rewrites the footer lines.
    loop {
        footer.borrow_mut().advance();
        tui.request_render();
        thread::sleep(Duration::from_millis(120));
    }
}
