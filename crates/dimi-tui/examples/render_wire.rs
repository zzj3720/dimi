//! Render a real wire.jsonl to transcript lines (dev tool for the slice 2
//! frame harness). Usage: cargo run -p dimi-tui --example render_wire -- <path> [width]
use std::path::Path;

use dimi_tui::theme::{DARK_COLORS, set_palette};
use dimi_tui::wire_transcript::{render_transcript, transcript_from_wire};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: render_wire <wire.jsonl> [width]");
    let width: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);
    set_palette(DARK_COLORS);
    let entries = transcript_from_wire(Path::new(&path)).expect("transcript");
    eprintln!("entries: {}", entries.len());
    let lines = render_transcript(&entries, width);
    for line in lines {
        println!("{line}");
    }
}
