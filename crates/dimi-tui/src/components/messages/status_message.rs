//! `StatusMessageComponent` / `NoticeMessageComponent` — status and notice
//! rows (port of `apps/dimi/src/tui/components/messages/status-message.ts`).

use crate::component::Component;
use crate::components::messages::MESSAGE_INDENT;
use crate::components::spacer::Spacer;
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};

/// A single status line: indented, dimmed (or error/warning colored) text.
pub struct StatusMessageComponent {
    content: String,
    color: Option<ColorToken>,
    text_component: Text,
}

impl StatusMessageComponent {
    pub fn new(content: &str, color: Option<ColorToken>) -> Self {
        let text = render_text(content, color);
        StatusMessageComponent {
            content: content.to_owned(),
            color,
            text_component: Text::new(&text, 0, 0),
        }
    }

    /// Update the body in place (used for live-streamed `!` shell output).
    pub fn update_content(&mut self, content: &str) {
        self.content = content.to_owned();
        let text = render_text(content, self.color);
        self.text_component.set_text(&text);
    }
}

/// Indent every line (not just the first) and strip carriage returns.
fn render_text(content: &str, color: Option<ColorToken>) -> String {
    let colored = match color {
        Some(token) => current_theme().fg(token, content),
        None => current_theme().fg(ColorToken::TextDim, content),
    };
    colored
        .replace('\r', "")
        .split('\n')
        .map(|line| format!("{MESSAGE_INDENT}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Component for StatusMessageComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.text_component.render(width)
    }

    fn invalidate(&mut self) {
        let text = render_text(&self.content, self.color);
        self.text_component.set_text(&text);
    }
}

/// A notice: blank line + title + optional detail.
pub struct NoticeMessageComponent {
    title: String,
    detail: Option<String>,
    title_text: Text,
    detail_text: Option<Text>,
    has_spacer: bool,
}

impl NoticeMessageComponent {
    pub fn new(title: &str, detail: Option<&str>) -> Self {
        let title_text = Text::new(
            &format!(
                "{MESSAGE_INDENT}{}",
                current_theme().fg(ColorToken::TextStrong, title)
            ),
            0,
            0,
        );
        let detail_text = detail.filter(|d| !d.is_empty()).map(|d| {
            Text::new(
                &format!(
                    "{MESSAGE_INDENT}{}",
                    current_theme().fg(ColorToken::TextDim, d)
                ),
                0,
                0,
            )
        });
        NoticeMessageComponent {
            title: title.to_owned(),
            detail: detail.map(str::to_owned),
            title_text,
            detail_text,
            has_spacer: true,
        }
    }
}

impl Component for NoticeMessageComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if self.has_spacer {
            lines.extend(Spacer::new(1).render(width));
        }
        lines.extend(self.title_text.render(width));
        if let Some(detail) = &mut self.detail_text {
            lines.extend(detail.render(width));
        }
        lines
    }

    fn invalidate(&mut self) {
        self.title_text.set_text(&format!(
            "{MESSAGE_INDENT}{}",
            current_theme().fg(ColorToken::TextStrong, &self.title)
        ));
        if let (Some(detail), Some(detail_text)) = (&self.detail, &mut self.detail_text) {
            detail_text.set_text(&format!(
                "{MESSAGE_INDENT}{}",
                current_theme().fg(ColorToken::TextDim, detail)
            ));
        }
    }
}
