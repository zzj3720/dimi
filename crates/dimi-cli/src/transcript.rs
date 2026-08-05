//! `TranscriptContainer` — the app-level transcript component.
//!
//! Slice-6 coordinator piece (port of the transcript responsibilities the TS
//! `DimiTUI` coordinator owns in `apps/dimi/src/tui/dimi-tui.ts` —
//! `appendTranscriptEntry`, the `trimTranscriptWindow` windowing, and the
//! scroll-on-input affordance). Data + rendering reuse the library surface:
//! [`TranscriptEntry`] and [`render_transcript`] cold-rebuild and render the
//! wire transcript; the container adds the *window* (max visible lines) and
//! the *scroll* state (up/down pages back through the rendered buffer).
//!
//! When the container has no entries it renders the [`WelcomeComponent`]
//! (the TS welcome panel shown before any session content), so the app shell
//! can mount exactly three children — transcript / footer / editor — and the
//! welcome still appears on first run.

use std::path::Path;

use dimi_tui::chrome::{WelcomeComponent, WelcomeState};
use dimi_tui::component::Component;
use dimi_tui::components::messages::tool_renderers::ToolCallData;
use dimi_tui::keys::matches_key;
use dimi_tui::theme::ColorToken;
use dimi_tui::wire_transcript::{
    TranscriptEntry, TranscriptEntryKind, WireTranscriptError, render_transcript,
    transcript_from_wire,
};

/// Build a `Status` transcript entry with an optional tone (`None` = the
/// default dim status tone; `Some(Error)` renders the error color via
/// [`render_transcript`]).
pub fn status_entry(content: &str, color: Option<ColorToken>) -> TranscriptEntry {
    TranscriptEntry {
        kind: TranscriptEntryKind::Status,
        content: content.to_owned(),
        bullet: None,
        tool_call: None,
        tool_result: None,
        status_color: color,
    }
}

/// Build a `User` transcript entry. `bullet` is `None` for the default
/// `✨ ` bullet, `Some("")` to suppress the bullet entirely (the `!` shell
/// echo form).
pub fn user_entry(content: &str, bullet: Option<String>) -> TranscriptEntry {
    TranscriptEntry {
        kind: TranscriptEntryKind::User,
        content: content.to_owned(),
        bullet,
        tool_call: None,
        tool_result: None,
        status_color: None,
    }
}

/// Build an `Assistant` transcript entry from streaming assistant text.
pub fn assistant_entry(content: &str) -> TranscriptEntry {
    TranscriptEntry {
        kind: TranscriptEntryKind::Assistant,
        content: content.to_owned(),
        bullet: None,
        tool_call: None,
        tool_result: None,
        status_color: None,
    }
}

/// Build a `ToolCall` transcript entry (engine `tool.call.started` →
/// `ToolCallData`; `args` is parsed into the object map the renderer reads).
pub fn tool_call_entry(
    tool_call_id: &str,
    name: &str,
    args: Option<&serde_json::Value>,
) -> TranscriptEntry {
    let map = match args.and_then(|v| v.as_object()) {
        Some(map) => map.clone(),
        None => serde_json::Map::new(),
    };
    TranscriptEntry {
        kind: TranscriptEntryKind::ToolCall,
        content: String::new(),
        bullet: None,
        tool_call: Some(ToolCallData {
            id: tool_call_id.to_owned(),
            name: name.to_owned(),
            args: map,
            truncated: false,
        }),
        tool_result: None,
        status_color: None,
    }
}

/// Default visible-line budget before the app applies the real layout.
const DEFAULT_MAX_LINES: usize = 10;

/// The transcript component: holds entries, a visible-line window, and a
/// scroll offset (lines scrolled back from the bottom).
#[derive(Debug, Clone)]
pub struct TranscriptContainer {
    entries: Vec<TranscriptEntry>,
    max_lines: usize,
    /// Lines scrolled back from the bottom; `0` = showing the tail.
    scroll_offset: usize,
    /// Rendered when `entries` is empty (the pre-session welcome panel).
    welcome: Option<WelcomeState>,
}

impl TranscriptContainer {
    pub fn new() -> Self {
        TranscriptContainer {
            entries: Vec::new(),
            max_lines: DEFAULT_MAX_LINES,
            scroll_offset: 0,
            welcome: None,
        }
    }

    /// Append an entry and auto-scroll to the bottom.
    pub fn push(&mut self, entry: TranscriptEntry) {
        self.entries.push(entry);
        self.scroll_offset = 0;
    }

    /// Clear the transcript (port of `/new` → `clearTranscriptWindow`).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.scroll_offset = 0;
    }

    /// Truncate the transcript to the first `len` entries (port of the
    /// `/undo` structural removal in `undo.ts`: drop the last prompt and
    /// everything that followed it). Clamped to `[0, entries.len()]`.
    pub fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
        self.scroll_offset = 0;
    }

    // `max_lines` / `scroll_offset` / `entries` are the headless test + app
    // introspection surface of a binary crate; `cargo build` (without test
    // cfg) flags them as unused, hence `#[allow(dead_code)]`.

    /// Set the visible-line window. `n == 0` is clamped to 1 so the editor
    /// never loses every transcript line.
    pub fn set_max_lines(&mut self, n: usize) {
        self.max_lines = n.max(1);
    }

    #[allow(dead_code)]
    pub fn max_lines(&self) -> usize {
        self.max_lines
    }

    /// Current scroll offset (lines back from the bottom).
    #[allow(dead_code)]
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Cold-rebuild entries from a `wire.jsonl` file. Returns the entry count
    /// on success; leaves the container unchanged on error.
    pub fn load_wire(&mut self, path: &Path) -> Result<usize, WireTranscriptError> {
        let entries = transcript_from_wire(path)?;
        let count = entries.len();
        self.entries = entries;
        self.scroll_offset = 0;
        Ok(count)
    }

    /// The live entries (tests and the app shell inspect these).
    #[allow(dead_code)]
    pub fn entries(&self) -> &[TranscriptEntry] {
        &self.entries
    }

    /// Replace the welcome panel rendered while the transcript is empty.
    pub fn set_welcome_state(&mut self, state: Option<WelcomeState>) {
        self.welcome = state;
    }

    /// Render the transcript window: all entries are laid out with
    /// [`render_transcript`], then the `max_lines` tail (offset by the scroll
    /// position) is returned.
    pub fn render_transcript_lines(&mut self, width: usize) -> Vec<String> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let all = render_transcript(&self.entries, width);
        let max_scroll = all.len().saturating_sub(self.max_lines);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
        let end = all.len().saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(self.max_lines);
        all[start..end].to_vec()
    }
}

impl Default for TranscriptContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TranscriptContainer {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.entries.is_empty() {
            return match &self.welcome {
                Some(state) => WelcomeComponent::new(state.clone()).render(width),
                None => Vec::new(),
            };
        }
        self.render_transcript_lines(width)
    }

    /// Up/Down scroll the transcript window (`scroll_offset` is clamped to
    /// the rendered buffer on the next render).
    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "up") {
            self.scroll_offset = self.scroll_offset.saturating_add(1);
        } else if matches_key(data, "down") {
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_tui::theme::{DARK_COLORS, set_palette};
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../dimi-tui/testdata/sample-wire.jsonl")
    }

    fn push_statuses(t: &mut TranscriptContainer, n: usize) {
        for i in 0..n {
            t.push(status_entry(&format!("msg {i}"), None));
        }
    }

    #[test]
    fn empty_container_renders_nothing_without_welcome() {
        let mut t = TranscriptContainer::new();
        assert!(Component::render(&mut t, 80).is_empty());
    }

    #[test]
    fn empty_container_renders_welcome_when_set() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        t.set_welcome_state(Some(WelcomeState {
            model: "claude-sonnet-4-5".to_owned(),
            ..WelcomeState::new()
        }));
        let lines = Component::render(&mut t, 80);
        let joined = lines.join("\n");
        assert!(
            joined.contains("Welcome to Dimi!"),
            "welcome should render: {joined}"
        );
        assert!(joined.contains("claude-sonnet-4-5"));
    }

    #[test]
    fn push_limits_render_to_max_lines() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        push_statuses(&mut t, 20);
        t.set_max_lines(5);
        let lines = t.render_transcript_lines(80);
        assert!(
            lines.len() <= 5,
            "window should be capped by max_lines: {}",
            lines.len()
        );
        let joined = lines.join("\n");
        assert!(
            joined.contains("msg 19"),
            "tail should be visible: {joined}"
        );
        assert!(!joined.contains("msg 0"), "head should be scrolled out");
    }

    #[test]
    fn push_autoscrolls_to_bottom() {
        let mut t = TranscriptContainer::new();
        push_statuses(&mut t, 5);
        assert_eq!(t.scroll_offset(), 0);
    }

    #[test]
    fn clear_resets_entries_and_scroll() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        push_statuses(&mut t, 10);
        t.handle_input("\x1b[A"); // up
        assert_eq!(t.scroll_offset(), 1);
        t.clear();
        assert!(t.entries().is_empty());
        assert_eq!(t.scroll_offset(), 0);
        assert!(t.render_transcript_lines(80).is_empty());
    }

    #[test]
    fn truncate_keeps_prefix_and_resets_scroll() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        push_statuses(&mut t, 5);
        t.handle_input("\x1b[A"); // up
        assert_eq!(t.scroll_offset(), 1);
        t.truncate(2);
        assert_eq!(t.entries().len(), 2);
        assert_eq!(t.entries()[0].content, "msg 0");
        assert_eq!(t.entries()[1].content, "msg 1");
        assert_eq!(t.scroll_offset(), 0);
        // Over-long len clamps to the entry count.
        t.truncate(100);
        assert_eq!(t.entries().len(), 2);
    }

    #[test]
    fn scroll_up_down_changes_window() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        push_statuses(&mut t, 20);
        t.set_max_lines(3);

        // Bottom: lines 17..20 → contains msg 19.
        let bottom = t.render_transcript_lines(80).join("\n");
        assert!(bottom.contains("msg 19"));
        assert!(!bottom.contains("msg 16"));

        // Up one: lines 16..19 → msg 17 visible, msg 19 gone.
        t.handle_input("\x1b[A");
        assert_eq!(t.scroll_offset(), 1);
        let up = t.render_transcript_lines(80).join("\n");
        assert!(up.contains("msg 17"), "window: {up}");
        assert!(!up.contains("msg 19"), "window: {up}");

        // Down one: back to the bottom.
        t.handle_input("\x1b[B");
        assert_eq!(t.scroll_offset(), 0);
        let down = t.render_transcript_lines(80).join("\n");
        assert!(down.contains("msg 19"));
    }

    #[test]
    fn scroll_clamps_at_top_and_bottom() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        push_statuses(&mut t, 20);
        t.set_max_lines(3);

        // Press up many times → clamped to max_scroll (17).
        for _ in 0..100 {
            t.handle_input("\x1b[A");
        }
        let top = t.render_transcript_lines(80).join("\n");
        assert!(top.contains("msg 0"), "top window: {top}");
        assert!(!top.contains("msg 3"));

        // Press down many times → clamped to 0.
        for _ in 0..100 {
            t.handle_input("\x1b[B");
        }
        let bottom = t.render_transcript_lines(80).join("\n");
        assert!(bottom.contains("msg 19"), "bottom window: {bottom}");
    }

    #[test]
    fn load_wire_populates_entries() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        let n = t.load_wire(&fixture_path()).unwrap();
        assert_eq!(n, 6, "expected the sample-wire entries");
        assert_eq!(t.entries().len(), 6);
        assert_eq!(t.entries()[0].kind, TranscriptEntryKind::User);
        assert_eq!(t.entries()[0].content, "Hello there!");
    }

    #[test]
    fn load_wire_missing_file_returns_error_and_keeps_state() {
        let mut t = TranscriptContainer::new();
        push_statuses(&mut t, 2);
        let before = t.entries().len();
        let result = t.load_wire(Path::new("/nonexistent/wire.jsonl"));
        assert!(result.is_err());
        assert_eq!(t.entries().len(), before, "state preserved on error");
    }

    #[test]
    fn status_color_is_carried_and_rendered() {
        set_palette(DARK_COLORS);
        let mut t = TranscriptContainer::new();
        t.push(status_entry("boom", Some(ColorToken::Error)));
        let lines = render_transcript(t.entries(), 80);
        let joined = lines.join("\n");
        // #E85454 error tone (232,84,84) must appear; default dim #888888 must not.
        assert!(joined.contains("232;84;84"), "error tone: {joined}");
        assert!(!joined.contains("136;136;136"), "not dim: {joined}");
    }
}
