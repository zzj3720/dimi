//! `AssistantMessageComponent` — renders an assistant message using the
//! Markdown component (port of
//! `apps/dimi/src/tui/components/messages/assistant-message.ts`).

use crate::component::Component;
use crate::components::messages::{MESSAGE_INDENT, STATUS_BULLET};
use crate::markdown::Markdown;
use crate::markdown_theme::create_markdown_theme;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

/// Assistant message with a bullet prefix and markdown content indented to
/// align after the bullet.
pub struct AssistantMessageComponent {
    show_bullet: bool,
    last_text: String,
    markdown: Option<Markdown>,
    render_cache: Option<(usize, Vec<String>)>,
}

impl AssistantMessageComponent {
    pub fn new(show_bullet: bool) -> Self {
        AssistantMessageComponent {
            show_bullet,
            last_text: String::new(),
            markdown: None,
            render_cache: None,
        }
    }

    pub fn set_show_bullet(&mut self, show: bool) {
        if self.show_bullet == show {
            return;
        }
        self.show_bullet = show;
        self.render_cache = None;
    }

    /// Update the message content, rebuilding the Markdown child when the
    /// text or transient mode changes.
    pub fn update_content(&mut self, text: &str) {
        let display_text = text.trim().to_owned();
        if display_text == self.last_text {
            return;
        }
        self.last_text = display_text.clone();
        self.render_cache = None;
        if display_text.is_empty() {
            self.markdown = None;
            return;
        }
        let mut markdown = Markdown::new(
            &display_text,
            0,
            0,
            Box::new(create_markdown_theme()),
            None,
            Default::default(),
        );
        markdown.set_hyperlinks(false);
        self.markdown = Some(markdown);
    }

    /// True when the message carries no visible content.
    pub fn is_empty(&self) -> bool {
        self.last_text.trim().is_empty()
    }
}

impl Component for AssistantMessageComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.last_text.trim().is_empty() {
            return Vec::new();
        }
        let safe_width = width;
        if safe_width == 0 {
            return vec![String::new()];
        }
        if let Some((cached_width, cached_lines)) = &self.render_cache {
            if *cached_width == safe_width {
                return cached_lines.clone();
            }
        }

        let prefix = if self.show_bullet {
            STATUS_BULLET
        } else {
            MESSAGE_INDENT
        };
        let content_width = (safe_width.saturating_sub(visible_width(prefix))).max(1);

        let content_lines = match &mut self.markdown {
            Some(md) => md.render(content_width),
            None => Vec::new(),
        };

        let mut lines: Vec<String> = vec![String::new()];
        for (i, content_line) in content_lines.iter().enumerate() {
            let p = if i == 0 && self.show_bullet {
                current_theme().fg(ColorToken::Text, STATUS_BULLET)
            } else {
                MESSAGE_INDENT.to_owned()
            };
            lines.push(format!("{p}{content_line}"));
        }
        let rendered: Vec<String> = lines
            .iter()
            .map(|line| truncate_to_width(line, safe_width, "…", false))
            .collect();
        self.render_cache = Some((safe_width, rendered.clone()));
        rendered
    }

    fn invalidate(&mut self) {
        self.render_cache = None;
        if let Some(md) = &mut self.markdown {
            md.invalidate();
        }
    }
}
