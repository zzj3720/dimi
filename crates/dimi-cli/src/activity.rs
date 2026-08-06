//! `ActivityComponent` — the streaming activity pane (port of
//! `apps/dimi/src/tui/components/panes/activity-pane.ts`).
//!
//! While a turn is streaming the pane shows a single moon-spinner line
//! (spinner frame + mode label + working tip) just below the transcript;
//! while idle it renders nothing (0 lines), so the layout gives the whole
//! budget to the transcript. The host advances the spinner once per event-loop
//! tick via [`ActivityComponent::advance`] and switches modes from the engine
//! streaming state via [`ActivityComponent::set_mode`].

use dimi_tui::component::Component;
use dimi_tui::loader::{MoonLoader, SpinnerStyle};
use dimi_tui::theme::{ColorToken, current_theme};
use dimi_tui::working_tips::current_working_tip;

/// Activity pane mode (`ActivityPaneMode` in the TS source). The non-idle
/// variants beyond `Composing` are wired by later slices (subagent waits,
/// thinking phases); they are constructed by tests here.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityMode {
    Idle,
    Waiting,
    Thinking,
    Composing,
    Tool,
}

impl ActivityMode {
    pub fn is_idle(&self) -> bool {
        matches!(self, ActivityMode::Idle)
    }

    /// The spinner label for a non-idle mode (`MoonLoader.setLabel`).
    fn label(&self) -> &'static str {
        match self {
            ActivityMode::Idle => "",
            ActivityMode::Waiting => "Waiting…",
            ActivityMode::Thinking => "Thinking…",
            ActivityMode::Composing => "Working…",
            ActivityMode::Tool => "Running…",
        }
    }
}

/// Spinner colour — the primary token (`currentTheme.fg('primary', frame)`).
fn spinner_color(text: &str) -> String {
    current_theme().fg(ColorToken::Primary, text)
}

/// The streaming activity pane. Holds a [`MoonLoader`] whose frame the host
/// advances; the tip is a working tip (see `dimi_tui::working_tips`).
pub struct ActivityComponent {
    mode: ActivityMode,
    loader: MoonLoader,
    tip: String,
}

impl ActivityComponent {
    pub fn new() -> Self {
        let mut loader = MoonLoader::new(SpinnerStyle::Moon, Some(spinner_color), "Working…");
        let tip = current_working_tip(0)
            .map(|t| t.text.to_owned())
            .unwrap_or_default();
        loader.set_tip(&format!(" · Tip: {tip}"));
        ActivityComponent {
            mode: ActivityMode::Idle,
            loader,
            tip,
        }
    }

    /// `#[allow(dead_code)]`: read only by tests / later slices; the app
    /// writes the mode via [`ActivityComponent::set_mode`].
    #[allow(dead_code)]
    pub fn mode(&self) -> ActivityMode {
        self.mode
    }

    /// Switch the pane to `mode`: idle hides the pane; a non-idle mode shows
    /// the spinner with that mode's label. Idempotent (re-renders only on
    /// change).
    pub fn set_mode(&mut self, mode: ActivityMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if !mode.is_idle() {
            self.loader.set_label(mode.label());
            self.loader.set_tip(&format!(" · Tip: {}", self.tip));
        }
    }

    /// Advance the spinner frame. Returns whether the pane is currently
    /// visible (the host re-renders only then). No-op while idle.
    pub fn advance(&mut self) -> bool {
        if self.mode.is_idle() {
            return false;
        }
        self.loader.advance();
        true
    }
}

impl Default for ActivityComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ActivityComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.mode.is_idle() {
            return Vec::new();
        }
        self.loader.set_available_width(width);
        self.loader.render(width)
    }

    fn invalidate(&mut self) {
        self.loader.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    #[test]
    fn idle_renders_zero_lines() {
        set_palette(DARK_COLORS);
        let mut a = ActivityComponent::new();
        assert_eq!(a.mode(), ActivityMode::Idle);
        assert!(
            Component::render(&mut a, 80).is_empty(),
            "idle hides the pane"
        );
        assert!(!a.advance(), "no spinner while idle");
    }

    #[test]
    fn streaming_renders_spinner_frame_and_label() {
        set_palette(DARK_COLORS);
        let mut a = ActivityComponent::new();
        a.set_mode(ActivityMode::Composing);
        assert!(!a.mode().is_idle());
        let lines = Component::render(&mut a, 80);
        assert_eq!(lines.len(), 1, "one spinner line: {lines:?}");
        let joined = lines.join("\n");
        assert!(joined.contains('🌑'), "spinner frame present: {joined:?}");
        assert!(joined.contains("Working…"), "mode label: {joined:?}");
        // The frame rotates as the host advances.
        assert!(a.advance(), "visible pane advances the spinner");
        let joined2 = Component::render(&mut a, 80).join("\n");
        assert!(joined2.contains('🌒'), "frame advanced: {joined2:?}");
    }

    #[test]
    fn switching_back_to_idle_hides_the_pane() {
        set_palette(DARK_COLORS);
        let mut a = ActivityComponent::new();
        a.set_mode(ActivityMode::Thinking);
        assert_eq!(Component::render(&mut a, 80).len(), 1);
        a.set_mode(ActivityMode::Idle);
        assert!(Component::render(&mut a, 80).is_empty());
        assert!(!a.advance());
    }
}
