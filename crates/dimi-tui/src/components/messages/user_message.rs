//! `UserMessageComponent` — renders a user message in the transcript
//! (port of `apps/dimi/src/tui/components/messages/user-message.ts`).

use crate::component::Component;
use crate::components::messages::USER_MESSAGE_BULLET;
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

/// Renders a user message: a role-colored bullet + the message text, wrapped
/// with continuation indent after the bullet.
pub struct UserMessageComponent {
    text: String,
    bullet: Option<String>,
    render_cache: Option<(usize, Vec<String>)>,
}

impl UserMessageComponent {
    pub fn new(text: &str, bullet: Option<String>) -> Self {
        UserMessageComponent {
            text: text.to_owned(),
            bullet,
            render_cache: None,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_owned();
        self.render_cache = None;
    }
}

impl Component for UserMessageComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let safe_width = width;
        if safe_width == 0 {
            return vec![String::new()];
        }
        if let Some((cached_width, cached_lines)) = &self.render_cache {
            if *cached_width == safe_width {
                return cached_lines.clone();
            }
        }

        let marker = self
            .bullet
            .clone()
            .unwrap_or_else(|| USER_MESSAGE_BULLET.to_owned());
        let bullet = if marker.is_empty() {
            String::new()
        } else {
            current_theme().bold_fg(ColorToken::RoleUser, &marker)
        };
        let bullet_width = visible_width(&marker);
        let content_width = (safe_width.saturating_sub(bullet_width)).max(1);

        let mut lines: Vec<String> = Vec::new();
        // Spacer(1).
        lines.push(String::new());

        let colored_text = current_theme().bold_fg(ColorToken::RoleUser, &self.text);
        let mut text_component = Text::new(&colored_text, 0, 0);
        let text_lines = text_component.render(content_width);
        for (i, text_line) in text_lines.iter().enumerate() {
            let prefix = if i == 0 {
                bullet.clone()
            } else {
                " ".repeat(bullet_width)
            };
            lines.push(format!("{prefix}{text_line}"));
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
    }
}

/// Invisible turn-boundary marker (replay) — renders zero lines.
pub struct ReplayTurnBoundaryComponent;

impl Component for ReplayTurnBoundaryComponent {
    fn render(&mut self, _width: usize) -> Vec<String> {
        Vec::new()
    }
    fn invalidate(&mut self) {}
}
