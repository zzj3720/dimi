//! Custom provider dialog — multi-field form for registering a provider by
//! hand. Port of
//! `apps/dimi/src/tui/components/dialogs/custom-provider-dialog.ts`
//! (`CustomProviderDialogComponent`).

use crate::component::{Component, Focusable};
use crate::dialogs::input_line::{InputEvent, InputLine};
use crate::keys::matches_key;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

/// `FIELDS` — (label, placeholder) pairs in order.
pub const CUSTOM_PROVIDER_FIELDS: [(&str, &str); 9] = [
    ("Provider id", "example-provider"),
    ("Display name", "Example Provider"),
    ("Base URL", "https://api.example.test/v1"),
    ("Protocol adapter", "openai-completions"),
    ("Model id", "example-chat"),
    ("Context window", "128000"),
    ("Max output tokens", "8192"),
    ("Input modalities (comma separated)", "text,image"),
    ("Thinking (off, always, or levels)", "off"),
];

/// `INPUTS` — valid input modalities.
pub const CUSTOM_PROVIDER_INPUTS: [&str; 2] = ["text", "image"];

/// The validated provider the dialog produces (`CustomProviderInput`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProviderInput {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api: String,
    pub model_id: String,
    pub reasoning: bool,
    pub input_modalities: Vec<String>,
    pub context_window: i64,
    pub max_tokens: i64,
    /// `thinkingLevelMap` — `(level, mapped)` pairs; `off` maps to `None`.
    pub thinking_level_map: Option<Vec<(String, Option<String>)>>,
}

/// `CustomProviderDialogResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomProviderDialogResult {
    Ok(CustomProviderInput),
    Cancel,
}

/// `CustomProviderDialogComponent`.
pub struct CustomProviderDialogComponent {
    apis: Vec<String>,
    input: InputLine,
    values: Vec<String>,
    index: usize,
    done: bool,
    message: Option<String>,
    focused: bool,
    action: Option<CustomProviderDialogResult>,
}

impl CustomProviderDialogComponent {
    pub fn new(apis: Vec<String>) -> Self {
        CustomProviderDialogComponent {
            apis,
            input: InputLine::new(),
            values: vec![String::new(); CUSTOM_PROVIDER_FIELDS.len()],
            index: 0,
            done: false,
            message: None,
            focused: false,
            action: None,
        }
    }

    /// Host polls after `handle_input` (mirrors the `onDone` callback).
    pub fn take_action(&mut self) -> Option<CustomProviderDialogResult> {
        self.action.take()
    }

    fn advance(&mut self, delta: i64) {
        self.values[self.index] = self.input.get_value().trim().to_owned();
        let next = (self.index as i64 + delta).clamp(0, CUSTOM_PROVIDER_FIELDS.len() as i64 - 1);
        self.index = next as usize;
        self.input.set_value(&self.values[self.index]);
        self.message = None;
    }

    fn submit(&mut self, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.message = Some(format!(
                "{} is required.",
                CUSTOM_PROVIDER_FIELDS[self.index].0
            ));
            return;
        }
        self.values[self.index] = trimmed.to_owned();
        self.message = None;
        if self.index < CUSTOM_PROVIDER_FIELDS.len() - 1 {
            self.advance(1);
            return;
        }
        let context_window = self.values[5].parse::<i64>();
        let max_tokens = self.values[6].parse::<i64>();
        let (Ok(context_window), Ok(max_tokens)) = (context_window, max_tokens) else {
            self.message =
                Some("Context window and max output tokens must be positive integers.".to_owned());
            return;
        };
        if context_window < 1 || max_tokens < 1 {
            self.message =
                Some("Context window and max output tokens must be positive integers.".to_owned());
            return;
        }
        let api = self.values[3].clone();
        if !self.apis.contains(&api) {
            self.message = Some(format!(
                "Protocol adapter must be one of: {}.",
                self.apis.join(", ")
            ));
            return;
        }
        let input_modalities: Vec<String> = self.values[7]
            .split(',')
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect();
        if input_modalities.is_empty()
            || input_modalities
                .iter()
                .any(|v| !CUSTOM_PROVIDER_INPUTS.contains(&v.as_str()))
        {
            self.message = Some("Input modalities must be text or image.".to_owned());
            return;
        }
        let thinking = self.values[8].trim().to_lowercase();
        let always_thinking = thinking == "always";
        let thinking_levels: Vec<String> = if thinking == "off" || always_thinking {
            Vec::new()
        } else {
            thinking
                .split(',')
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .collect()
        };
        if thinking != "off" && !always_thinking && thinking_levels.is_empty() {
            self.message =
                Some("Thinking must be off, always, or comma-separated levels.".to_owned());
            return;
        }

        let thinking_level_map = if thinking == "off" {
            None
        } else {
            let mut pairs: Vec<(String, Option<String>)> = Vec::new();
            if always_thinking {
                pairs.push(("off".to_owned(), None));
            }
            for level in &thinking_levels {
                pairs.push((level.clone(), Some(level.clone())));
            }
            Some(pairs)
        };

        self.done = true;
        self.action = Some(CustomProviderDialogResult::Ok(CustomProviderInput {
            id: self.values[0].clone(),
            name: self.values[1].clone(),
            base_url: self.values[2].clone(),
            api,
            model_id: self.values[4].clone(),
            reasoning: thinking != "off",
            input_modalities,
            context_window,
            max_tokens,
            thinking_level_map,
        }));
    }
}

impl Component for CustomProviderDialogComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.input.set_focused(self.focused && !self.done);
        let safe_width = width;
        if safe_width < 4 {
            return vec![String::new()];
        }
        let inner_width = safe_width - 4;
        let theme = current_theme();

        let mut content: Vec<String> = vec![
            theme.bold_fg(ColorToken::TextStrong, "Add custom provider"),
            theme.fg(
                ColorToken::TextMuted,
                "Tab/↑↓ navigate · Enter next · Esc cancel",
            ),
            String::new(),
        ];
        for (index, (label, placeholder)) in CUSTOM_PROVIDER_FIELDS.iter().enumerate() {
            let active = index == self.index;
            let value = if active {
                self.input.render(inner_width)
            } else {
                self.values[index].clone()
            };
            let label_styled = theme.fg(
                if active {
                    ColorToken::Primary
                } else {
                    ColorToken::TextMuted
                },
                &format!("{label}{}", if active { " *" } else { "" }),
            );
            let value_styled = {
                let rendered = if value.is_empty() {
                    format!("  {placeholder}")
                } else {
                    value.clone()
                };
                theme.fg(
                    if active {
                        ColorToken::Text
                    } else {
                        ColorToken::TextMuted
                    },
                    &rendered,
                )
            };
            content.push(label_styled);
            content.push(value_styled);
        }
        if let Some(message) = &self.message {
            content.push(String::new());
            content.push(theme.fg(ColorToken::Error, message));
        }
        content.push(String::new());
        content.push(theme.fg(
            ColorToken::TextMuted,
            if self.index == CUSTOM_PROVIDER_FIELDS.len() - 1 {
                "Enter submit"
            } else {
                "Enter next"
            },
        ));

        let border = |value: String| theme.fg(ColorToken::Primary, &value);
        let mut lines: Vec<String> = vec![border(format!("╭{}╮", "─".repeat(safe_width - 2)))];
        for line in content {
            let clipped = truncate_to_width(&line, inner_width, "…", false);
            lines.push(border(format!(
                "│  {clipped}{}│",
                " ".repeat(inner_width.saturating_sub(visible_width(&clipped)))
            )));
        }
        lines.push(border(format!("╰{}╯", "─".repeat(safe_width - 2))));
        lines
    }

    fn handle_input(&mut self, data: &str) {
        if self.done {
            return;
        }
        if matches_key(data, "escape") || matches_key(data, "ctrl+c") || matches_key(data, "ctrl+d")
        {
            self.done = true;
            self.action = Some(CustomProviderDialogResult::Cancel);
            return;
        }
        if matches_key(data, "tab") || matches_key(data, "down") {
            self.advance(1);
            return;
        }
        if matches_key(data, "shift+tab") || matches_key(data, "up") {
            self.advance(-1);
            return;
        }
        if self.input.handle_input(data) == InputEvent::Submit {
            let value = self.input.get_value().to_owned();
            self.submit(&value);
        }
    }

    fn invalidate(&mut self) {}

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl Focusable for CustomProviderDialogComponent {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    fn plain(joined: &str) -> String {
        crate::ansi::strip_ansi(joined)
    }

    #[test]
    fn renders_form_fields() {
        set_palette(DARK_COLORS);
        let mut c = CustomProviderDialogComponent::new(vec!["openai-completions".to_owned()]);
        let lines = c.render(80);
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains("Add custom provider"), "{joined}");
        assert!(joined.contains("Provider id *"), "{joined}");
        // The active (empty) field shows the live "> " input; placeholders
        // appear on the non-active empty fields.
        assert!(joined.contains("Example Provider"), "{joined}");
        assert!(joined.contains("https://api.example.test/v1"), "{joined}");
        assert!(joined.contains("Enter next"), "{joined}");
        // Box borders.
        assert!(plain(&lines[0]).starts_with('╭'));
        assert!(plain(lines.last().unwrap()).starts_with('╰'));
    }

    #[test]
    fn advance_commits_value() {
        set_palette(DARK_COLORS);
        let mut c = CustomProviderDialogComponent::new(vec![]);
        for ch in "my-provider".chars() {
            c.handle_input(&ch.to_string());
        }
        c.handle_input("\t"); // advance
        assert_eq!(c.values[0], "my-provider");
        assert_eq!(c.index, 1);
    }

    #[test]
    fn submit_requires_field() {
        set_palette(DARK_COLORS);
        let mut c = CustomProviderDialogComponent::new(vec![]);
        c.handle_input("\r");
        assert_eq!(c.take_action(), None);
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("Provider id is required."), "{joined}");
    }

    #[test]
    fn full_submit_produces_provider() {
        set_palette(DARK_COLORS);
        let mut c = CustomProviderDialogComponent::new(vec!["openai-completions".to_owned()]);
        let values = [
            "acme",
            "Acme",
            "https://api.acme.test/v1",
            "openai-completions",
            "acme-chat",
            "128000",
            "8192",
            "text,image",
            "always",
        ];
        for v in &values[..8] {
            for ch in v.chars() {
                c.handle_input(&ch.to_string());
            }
            c.handle_input("\t");
        }
        // Last field: type then Enter submits.
        for ch in values[8].chars() {
            c.handle_input(&ch.to_string());
        }
        c.handle_input("\r");
        match c.take_action() {
            Some(CustomProviderDialogResult::Ok(provider)) => {
                assert_eq!(provider.id, "acme");
                assert_eq!(provider.api, "openai-completions");
                assert_eq!(provider.context_window, 128_000);
                assert_eq!(provider.max_tokens, 8192);
                assert_eq!(provider.input_modalities, vec!["text", "image"]);
                assert!(provider.reasoning);
                assert_eq!(
                    provider.thinking_level_map,
                    Some(vec![("off".to_owned(), None)])
                );
            }
            other => panic!("expected Ok provider, got {other:?}"),
        }
    }
}
