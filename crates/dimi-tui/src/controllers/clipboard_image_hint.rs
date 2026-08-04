//! Clipboard-image hint controller — port of `apps/dimi/src/tui/controllers/
//! clipboard-image-hint.ts`.
//!
//! Pure state machine: focus in/out, the debounce generation counter, the
//! "first observation establishes a baseline" rule, and the armed/notified
//! latch so the same lingering image does not nag on every focus. The
//! clipboard read (`clipboardHasImage`), the input listener, the footer
//! `setTransientHint`, and the debounce / clear timers are
//! `// TODO(legacy)`.

/// `FOCUS_DEBOUNCE_MS`.
pub const FOCUS_DEBOUNCE_MS: u64 = 1_000;
/// `HINT_DISPLAY_MS`.
pub const HINT_DISPLAY_MS: u64 = 4_000;

/// Platform for the paste shortcut label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Other,
}

/// `getPasteImageShortcut` — `Alt+V` on Windows, `Ctrl+V` elsewhere.
pub fn paste_image_shortcut(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "Alt+V",
        Platform::Other => "Ctrl+V",
    }
}

/// What a clipboard-check run asks the host to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintAction {
    /// Show the transient hint (and arm the clear timer).
    ShowHint(String),
    /// Clear the owned hint (timer fired / stop).
    ClearHint,
    /// Nothing to do.
    None,
}

/// The clipboard-image hint controller (port of
/// `ClipboardImageHintController`).
#[derive(Debug, Clone)]
pub struct ClipboardImageHintController {
    /// First observation after start only establishes a baseline.
    initialized: bool,
    /// Whether a detected clipboard image may trigger a hint.
    armed: bool,
    focused: bool,
    check_generation: u64,
    last_hint_text: Option<String>,
}

impl Default for ClipboardImageHintController {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardImageHintController {
    pub fn new() -> Self {
        ClipboardImageHintController {
            initialized: false,
            armed: true,
            focused: true,
            check_generation: 0,
            last_hint_text: None,
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn last_hint_text(&self) -> Option<&str> {
        self.last_hint_text.as_deref()
    }

    /// `start` — the input listener is legacy; `establish_initial_baseline`
    /// is exposed separately.
    pub fn start(&mut self) {
        // TODO(legacy): ui.addInputListener(handleInput)
    }

    /// `stop`.
    pub fn stop(&mut self) {
        // TODO(legacy): clearDebounceTimer(); clearClearHintTimer();
        //   disposeInputListener()
        self.check_generation += 1;
        self.last_hint_text = None;
        self.initialized = false;
        self.armed = true;
    }

    /// `handleInput` — terminal focus in/out markers.
    pub fn handle_input(&mut self, focus_in: bool, focus_out: bool) -> Option<u64> {
        if focus_in {
            self.focused = true;
            return Some(self.schedule_check());
        }
        if focus_out {
            self.focused = false;
            // TODO(legacy): clearDebounceTimer()
            return None;
        }
        None
    }

    /// `scheduleCheck` — bumps the generation and returns it.
    pub fn schedule_check(&mut self) -> u64 {
        // TODO(legacy): clearDebounceTimer(); setTimeout(runCheck, FOCUS_DEBOUNCE_MS)
        self.check_generation += 1;
        self.check_generation
    }

    /// `establishInitialBaseline` — the first observation records the state
    /// and stays quiet.
    pub fn establish_initial_baseline(&mut self, has_image: bool) {
        if !self.focused {
            return;
        }
        self.check_generation += 1;
        let generation = self.check_generation;
        if generation != self.check_generation {
            return;
        }
        self.initialized = true;
        self.armed = !has_image;
    }

    /// `runCheck` — the post-debounce clipboard observation decision.
    pub fn run_check(
        &mut self,
        generation: u64,
        model_supports_image: bool,
        has_image: bool,
    ) -> HintAction {
        if !self.focused {
            return HintAction::None;
        }
        if !model_supports_image {
            return HintAction::None;
        }
        // TODO(legacy): hasImage = await clipboardHasImage()
        if generation != self.check_generation {
            return HintAction::None;
        }
        if !self.focused {
            return HintAction::None;
        }

        // First observation after start only establishes the baseline.
        if !self.initialized {
            self.initialized = true;
            self.armed = !has_image;
            return HintAction::None;
        }

        if !has_image {
            // Clipboard holds no image, so the next image that appears is a
            // new one worth notifying about.
            self.armed = true;
            return HintAction::None;
        }

        if !self.armed {
            return HintAction::None;
        }

        let hint_text = format!(
            "Image in clipboard · {} to paste",
            paste_image_shortcut(Platform::Other)
        );
        // TODO(legacy): clearClearHintTimer(); footer.setTransientHint(hintText)
        self.last_hint_text = Some(hint_text.clone());
        self.armed = false;
        // TODO(legacy): clearHintTimer = setTimeout(clearOwnedHint, HINT_DISPLAY_MS)
        HintAction::ShowHint(hint_text)
    }

    /// `clearOwnedHint` — clears the hint when the host footer is showing it.
    pub fn clear_owned_hint(&mut self, footer_showing_hint: bool) -> HintAction {
        if footer_showing_hint {
            self.last_hint_text = None;
            // TODO(legacy): footer.setTransientHint(null); requestRender()
            return HintAction::ClearHint;
        }
        self.last_hint_text = None;
        HintAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observation_establishes_baseline() {
        let mut c = ClipboardImageHintController::new();
        c.establish_initial_baseline(true);
        assert!(c.is_initialized());
        assert!(!c.is_armed()); // an image already present is not "new"
    }

    #[test]
    fn empty_clipboard_rearms_and_stays_quiet() {
        let mut c = ClipboardImageHintController::new();
        c.establish_initial_baseline(true);
        let gen_id = c.schedule_check();
        assert_eq!(c.run_check(gen_id, true, false), HintAction::None);
        assert!(c.is_armed());
    }

    #[test]
    fn new_image_triggers_hint_once_then_latches() {
        let mut c = ClipboardImageHintController::new();
        c.establish_initial_baseline(false);
        assert!(c.is_armed());

        let gen_id = c.schedule_check();
        let action = c.run_check(gen_id, true, true);
        match action {
            HintAction::ShowHint(text) => {
                assert!(text.contains("Image in clipboard"));
                assert!(text.contains("Ctrl+V"));
            }
            _ => panic!("expected ShowHint"),
        }
        assert!(!c.is_armed());

        // Same lingering image → quiet.
        let gen_id = c.schedule_check();
        assert_eq!(c.run_check(gen_id, true, true), HintAction::None);
    }

    #[test]
    fn focus_out_blocks_check() {
        let mut c = ClipboardImageHintController::new();
        c.establish_initial_baseline(false);
        let gen_id = c.schedule_check();
        c.handle_input(false, true);
        assert!(!c.is_focused());
        assert_eq!(c.run_check(gen_id, true, true), HintAction::None);
    }

    #[test]
    fn model_without_image_support_never_hints() {
        let mut c = ClipboardImageHintController::new();
        c.establish_initial_baseline(false);
        let gen_id = c.schedule_check();
        assert_eq!(c.run_check(gen_id, false, true), HintAction::None);
    }

    #[test]
    fn stop_resets_state() {
        let mut c = ClipboardImageHintController::new();
        c.establish_initial_baseline(false);
        let gen_id = c.schedule_check();
        c.run_check(gen_id, true, true);
        assert!(!c.is_armed());
        c.stop();
        assert!(!c.is_initialized());
        assert!(c.is_armed());
        assert_eq!(c.last_hint_text(), None);
    }

    #[test]
    fn stale_generation_is_ignored() {
        let mut c = ClipboardImageHintController::new();
        c.establish_initial_baseline(false);
        let gen_id = c.schedule_check();
        c.schedule_check(); // newer generation supersedes
        assert_eq!(c.run_check(gen_id, true, true), HintAction::None);
    }

    #[test]
    fn paste_shortcut_by_platform() {
        assert_eq!(paste_image_shortcut(Platform::Windows), "Alt+V");
        assert_eq!(paste_image_shortcut(Platform::Other), "Ctrl+V");
    }
}
