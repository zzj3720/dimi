//! `ThinkingComponent` — renders thinking content in the transcript
//! (port of `apps/dimi/src/tui/components/messages/thinking.ts`).

use crate::component::Component;
use crate::components::messages::{
    BRAILLE_SPINNER_FRAMES, MESSAGE_INDENT, STATUS_BULLET, THINKING_PREVIEW_LINES,
};
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};
use crate::wrap::truncate_to_width;

/// Thinking render mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingMode {
    Live,
    Finalized,
}

/// Thinking content block with live in-place updates, expand/collapse, and
/// the live braille spinner header.
pub struct ThinkingComponent {
    text: String,
    show_marker: bool,
    mode: ThinkingMode,
    expanded: bool,
    hidden: bool,
    /// Hold a single Text instance so the (text, width) → lines cache
    /// survives across renders (same as TS).
    text_component: Text,
    render_cache: Option<(usize, Vec<String>)>,
}

impl ThinkingComponent {
    pub fn new(text: &str, show_marker: bool, mode: ThinkingMode) -> Self {
        let styled = current_theme().italic_fg(ColorToken::TextDim, text);
        ThinkingComponent {
            text: text.to_owned(),
            show_marker,
            mode,
            expanded: false,
            hidden: false,
            text_component: Text::new(&styled, 0, 0),
            render_cache: None,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        self.text = text.to_owned();
        self.render_cache = None;
        let styled = current_theme().italic_fg(ColorToken::TextDim, text);
        self.text_component.set_text(&styled);
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.render_cache = None;
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        self.render_cache = None;
    }

    pub fn finalize(&mut self) {
        if self.mode == ThinkingMode::Finalized {
            return;
        }
        self.mode = ThinkingMode::Finalized;
        self.render_cache = None;
    }

    pub fn mode(&self) -> ThinkingMode {
        self.mode
    }
}

impl Component for ThinkingComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.hidden && self.mode == ThinkingMode::Finalized {
            return Vec::new();
        }
        if let Some((cached_width, cached_lines)) = &self.render_cache {
            if *cached_width == width {
                return cached_lines.clone();
            }
        }

        let content_width = (width.saturating_sub(MESSAGE_INDENT.len())).max(1);
        let content_lines = if self.text.is_empty() {
            vec![String::new()]
        } else {
            self.text_component.render(content_width)
        };

        let rendered: Vec<String> = if self.mode == ThinkingMode::Live {
            let visible_lines = if content_lines.len() > THINKING_PREVIEW_LINES {
                content_lines[content_lines.len() - THINKING_PREVIEW_LINES..].to_vec()
            } else {
                content_lines
            };
            let spinner = current_theme().fg(
                ColorToken::TextDim,
                &format!("{} ", BRAILLE_SPINNER_FRAMES[0]),
            );
            let mut out = vec![String::new()];
            out.push(format!(
                "{spinner}{}",
                current_theme().fg(ColorToken::TextDim, "thinking...")
            ));
            for line in visible_lines {
                out.push(format!("{MESSAGE_INDENT}{line}"));
            }
            out
        } else {
            let mut lines: Vec<String> = vec![String::new()];
            for (i, content_line) in content_lines.iter().enumerate() {
                let p = if i == 0 && self.show_marker {
                    current_theme().fg(ColorToken::TextDim, STATUS_BULLET)
                } else {
                    MESSAGE_INDENT.to_owned()
                };
                lines.push(format!("{p}{content_line}"));
            }
            if self.expanded || content_lines.len() <= THINKING_PREVIEW_LINES {
                lines
            } else {
                // Leading blank + first PREVIEW_LINES content lines + hint line.
                let mut truncated = lines[..1 + THINKING_PREVIEW_LINES].to_vec();
                let remaining = content_lines.len() - THINKING_PREVIEW_LINES;
                let hint = format!("... ({remaining} more lines, ctrl+o to expand)");
                let indent_width = MESSAGE_INDENT.len().min(width);
                let hint_width = width.saturating_sub(indent_width);
                truncated.push(format!(
                    "{}{}",
                    " ".repeat(indent_width),
                    current_theme().dim(&truncate_to_width(&hint, hint_width, "…", false))
                ));
                truncated
            }
        };

        self.render_cache = Some((width, rendered.clone()));
        rendered
    }

    fn invalidate(&mut self) {
        self.render_cache = None;
        let styled = current_theme().italic_fg(ColorToken::TextDim, &self.text);
        self.text_component.set_text(&styled);
    }
}
