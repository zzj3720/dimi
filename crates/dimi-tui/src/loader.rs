//! Loader / spinner component — `MoonLoader` from
//! `apps/dimi/src/tui/components/chrome/moon-loader.ts`, ported.
//!
//! The TS side drives the frame rotation with `setInterval`; here the host
//! controller calls [`MoonLoader::advance`] once per tick (this slice ships
//! the component + frame stepping only, no timer).

use crate::component::Component;
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;

/// Moon phase spinner frames (`MOON_SPINNER_FRAMES`).
pub const MOON_SPINNER_FRAMES: [&str; 8] = ["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"];
/// Moon spinner interval (ms).
pub const MOON_SPINNER_INTERVAL_MS: u64 = 120;
/// Braille spinner frames (`BRAILLE_SPINNER_FRAMES`).
pub const BRAILLE_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Braille spinner interval (ms).
pub const BRAILLE_SPINNER_INTERVAL_MS: u64 = 80;

/// Spinner visual style (`SpinnerStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinnerStyle {
    Moon,
    Braille,
}

/// Loader component — a single line of spinner frame + optional label + an
/// optional tip that is only shown when it fits the available width.
///
/// Byte layout mirrors the TS `MoonLoader` (which extends `Text` with padding
/// `(1, 0)`): the composed line is rendered through the [`Text`] component, so
/// each output row is padded to the render width with a one-cell left/right
/// margin.
pub struct MoonLoader {
    frames: &'static [&'static str],
    current_frame: usize,
    interval_ms: u64,
    color_fn: Option<fn(&str) -> String>,
    label: String,
    display_text: String,
    inline_text: String,
    tip: String,
    available_width: usize,
    text: Text,
}

impl MoonLoader {
    pub fn new(style: SpinnerStyle, color_fn: Option<fn(&str) -> String>, label: &str) -> Self {
        let (frames, interval_ms) = match style {
            SpinnerStyle::Moon => (&MOON_SPINNER_FRAMES[..], MOON_SPINNER_INTERVAL_MS),
            SpinnerStyle::Braille => (&BRAILLE_SPINNER_FRAMES[..], BRAILLE_SPINNER_INTERVAL_MS),
        };
        let mut loader = MoonLoader {
            frames,
            current_frame: 0,
            interval_ms,
            color_fn,
            label: label.to_owned(),
            display_text: String::new(),
            inline_text: String::new(),
            tip: String::new(),
            available_width: 0,
            text: Text::new("", 1, 0),
        };
        loader.update_display();
        loader
    }

    /// Frame interval in milliseconds (the TS `setInterval` period).
    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    /// Index of the currently displayed frame.
    pub fn current_frame(&self) -> usize {
        self.current_frame
    }

    /// Advance to the next frame and recompute the display line. The host
    /// controller calls this once per tick.
    pub fn advance(&mut self) {
        self.current_frame = (self.current_frame + 1) % self.frames.len();
        self.update_display();
    }

    pub fn set_label(&mut self, label: &str) {
        self.label = label.to_owned();
        self.update_display();
    }

    pub fn set_color_fn(&mut self, color_fn: Option<fn(&str) -> String>) {
        self.color_fn = color_fn;
        self.update_display();
    }

    pub fn set_tip(&mut self, tip: &str) {
        self.tip = tip.to_owned();
        self.update_display();
    }

    pub fn set_available_width(&mut self, width: usize) {
        if self.available_width == width {
            return;
        }
        self.available_width = width;
        self.update_display();
    }

    /// The inline text — spinner + label, intentionally excluding the tip
    /// (`renderInline` in the TS source).
    pub fn render_inline(&self) -> &str {
        &self.inline_text
    }

    /// Directly replace the rendered text (mirrors the TS `setText` used by
    /// the login-progress spinner stop handler, e.g. `✓ Downloading`).
    pub fn set_text(&mut self, text: &str) {
        self.display_text = text.to_owned();
        self.text.set_text(text);
    }

    fn update_display(&mut self) {
        let frame = self.frames[self.current_frame];
        let colored_frame = match self.color_fn {
            Some(f) => f(frame),
            None => frame.to_owned(),
        };
        let base_text = if self.label.is_empty() {
            colored_frame
        } else {
            format!("{colored_frame} {}", self.label)
        };
        self.inline_text = base_text.clone();

        let mut text = base_text.clone();
        if !self.tip.is_empty() {
            let theme = current_theme();
            let with_tip = format!("{base_text}{}", theme.fg(ColorToken::TextDim, &self.tip));
            if self.available_width == 0 || visible_width(&with_tip) <= self.available_width {
                text = with_tip;
            }
        }
        self.display_text = text;
        self.text.set_text(&self.display_text);
    }
}

impl Component for MoonLoader {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.text.render(width)
    }

    fn invalidate(&mut self) {
        self.text.invalidate();
    }
}
