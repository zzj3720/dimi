/// Background color function applied to padded lines.
pub type BgFn = dyn Fn(&str) -> String;

/// `Text` component — displays multi-line text with word wrapping
/// (port of `@dimi-agent/pi-tui` `src/components/text.ts`).
use crate::component::Component;
use crate::wrap::{apply_background_to_line, wrap_text_with_ansi};

/// Multi-line text with optional padding and background.
pub struct Text {
    text: String,
    padding_x: usize,
    padding_y: usize,
    custom_bg_fn: Option<Box<BgFn>>,
    cached_text: Option<String>,
    cached_width: Option<usize>,
    cached_lines: Option<Vec<String>>,
}

impl Text {
    pub fn new(text: &str, padding_x: usize, padding_y: usize) -> Self {
        Text {
            text: text.to_owned(),
            padding_x,
            padding_y,
            custom_bg_fn: None,
            cached_text: None,
            cached_width: None,
            cached_lines: None,
        }
    }

    pub fn new_with_bg(
        text: &str,
        padding_x: usize,
        padding_y: usize,
        custom_bg_fn: Option<Box<BgFn>>,
    ) -> Self {
        Text {
            text: text.to_owned(),
            padding_x,
            padding_y,
            custom_bg_fn,
            cached_text: None,
            cached_width: None,
            cached_lines: None,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_owned();
        self.cached_text = None;
        self.cached_width = None;
        self.cached_lines = None;
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Component for Text {
    fn render(&mut self, width: usize) -> Vec<String> {
        if let (Some(cached_text), Some(cached_width), Some(cached_lines)) =
            (&self.cached_text, self.cached_width, &self.cached_lines)
        {
            if cached_text == &self.text && cached_width == width {
                return cached_lines.clone();
            }
        }

        // Don't render anything if there's no actual text.
        if self.text.trim().is_empty() {
            let result = Vec::new();
            self.cached_text = Some(self.text.clone());
            self.cached_width = Some(width);
            self.cached_lines = Some(result.clone());
            return result;
        }

        // Replace tabs with 3 spaces.
        let normalized_text = self.text.replace('\t', "   ");

        // Content width (subtract left/right margins).
        let content_width = width.saturating_sub(self.padding_x * 2).max(1);

        // Wrap text (preserves ANSI codes but does NOT pad).
        let wrapped_lines = wrap_text_with_ansi(&normalized_text, content_width);

        // Add margins and background to each line.
        let left_margin = " ".repeat(self.padding_x);
        let right_margin = " ".repeat(self.padding_x);
        let mut content_lines: Vec<String> = Vec::new();
        for line in wrapped_lines {
            let line_with_margins = format!("{left_margin}{line}{right_margin}");
            if let Some(bg_fn) = &self.custom_bg_fn {
                content_lines.push(apply_background_to_line(&line_with_margins, width, bg_fn));
            } else {
                let visible_len = crate::width::visible_width(&line_with_margins);
                let padding_needed = width.saturating_sub(visible_len);
                content_lines.push(format!("{line_with_margins}{}", " ".repeat(padding_needed)));
            }
        }

        // Top/bottom padding (empty lines).
        let empty_line = " ".repeat(width);
        let mut empty_lines: Vec<String> = Vec::new();
        for _ in 0..self.padding_y {
            if let Some(bg_fn) = &self.custom_bg_fn {
                empty_lines.push(apply_background_to_line(&empty_line, width, bg_fn));
            } else {
                empty_lines.push(empty_line.clone());
            }
        }

        let mut result = empty_lines;
        result.extend(content_lines);
        let pad = " ".repeat(width);
        for _ in 0..self.padding_y {
            if let Some(bg_fn) = &self.custom_bg_fn {
                result.push(apply_background_to_line(&pad, width, bg_fn));
            } else {
                result.push(pad.clone());
            }
        }

        self.cached_text = Some(self.text.clone());
        self.cached_width = Some(width);
        self.cached_lines = Some(result.clone());
        if result.is_empty() {
            vec![String::new()]
        } else {
            result
        }
    }

    fn invalidate(&mut self) {
        self.cached_text = None;
        self.cached_width = None;
        self.cached_lines = None;
    }
}
