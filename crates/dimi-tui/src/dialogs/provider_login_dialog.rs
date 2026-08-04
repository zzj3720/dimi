//! Provider login dialog — one provider-owned authentication surface for
//! OAuth, device-code, API-key, and prompt-driven flows. Port of
//! `apps/dimi/src/tui/components/dialogs/provider-login-dialog.ts`
//! (`ProviderLoginDialogComponent`).
//!
//! The SDK `AuthInteraction` callbacks (`notify` / `prompt`) are bridged by
//! the host: the dialog exposes [`ProviderLoginDialogComponent::notify`] and
//! [`ProviderLoginDialogComponent::set_prompt`] plus a `take_action` the host
//! polls to learn when the user answered or cancelled.

use crate::component::{Component, Focusable};
use crate::dialogs::SELECT_POINTER;
use crate::dialogs::input_line::{InputEvent, InputLine};
use crate::keys::matches_key;
use crate::searchable_list::SearchableList;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::{truncate_to_width, wrap_text_with_ansi};

/// One link line from an `info` event.
#[derive(Debug, Clone)]
pub struct InfoLink {
    pub label: Option<String>,
    pub url: String,
}

/// `ProviderAuthEvent` — the `AuthInteraction.notify` payloads the dialog
/// renders (subset of the SDK event types).
#[derive(Debug, Clone)]
pub enum ProviderAuthEvent {
    AuthUrl {
        instructions: String,
        url: String,
    },
    DeviceCode {
        verification_uri: String,
        user_code: String,
    },
    Info {
        message: String,
        links: Vec<InfoLink>,
    },
    Progress {
        message: String,
    },
}

/// A selectable option in a `select` prompt.
#[derive(Debug, Clone)]
pub struct SelectOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

/// `ProviderAuthPrompt` — a prompt the provider asks the user to answer.
#[derive(Debug, Clone)]
pub enum ProviderAuthPrompt {
    Input {
        message: String,
        placeholder: Option<String>,
        /// When true the typed value is masked with `•`.
        secret: bool,
    },
    Select {
        message: String,
        options: Vec<SelectOption>,
    },
}

impl ProviderAuthPrompt {
    fn message(&self) -> &str {
        match self {
            ProviderAuthPrompt::Input { message, .. } => message,
            ProviderAuthPrompt::Select { message, .. } => message,
        }
    }
}

/// `ProviderLoginDialogOptions`.
#[derive(Debug, Clone)]
pub struct ProviderLoginDialogOptions {
    pub provider_name: String,
    pub method_label: String,
}

/// Action the host reacts to after `handle_input`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginAction {
    Answer(String),
    Cancel,
}

/// `ProviderLoginDialogComponent`.
pub struct ProviderLoginDialogComponent {
    options: ProviderLoginDialogOptions,
    input: InputLine,
    details: Vec<String>,
    /// Active prompt state (`select` keeps a paging/cursor list).
    prompt: Option<ProviderAuthPrompt>,
    select_list: Option<SearchableList<SelectOption>>,
    status: String,
    done: bool,
    focused: bool,
    action: Option<LoginAction>,
}

impl ProviderLoginDialogComponent {
    pub fn new(options: ProviderLoginDialogOptions) -> Self {
        ProviderLoginDialogComponent {
            options,
            input: InputLine::new(),
            details: Vec::new(),
            prompt: None,
            select_list: None,
            status: "Starting authentication…".to_owned(),
            done: false,
            focused: false,
            action: None,
        }
    }

    /// Host polls after `handle_input` (mirrors `resolve`/`reject` callbacks).
    pub fn take_action(&mut self) -> Option<LoginAction> {
        self.action.take()
    }

    /// `notify(event)` — push provider progress into the dialog.
    pub fn notify(&mut self, event: ProviderAuthEvent) {
        match event {
            ProviderAuthEvent::AuthUrl { instructions, url } => {
                self.push_detail(&instructions);
                self.push_detail(&format!("Open: {url}"));
                self.status = "Waiting for authorization…".to_owned();
            }
            ProviderAuthEvent::DeviceCode {
                verification_uri,
                user_code,
            } => {
                self.push_detail(&format!("Open: {verification_uri}"));
                self.push_detail(&format!("Verification code: {user_code}"));
                self.status = "Waiting for authorization…".to_owned();
            }
            ProviderAuthEvent::Info { message, links } => {
                self.push_detail(&message);
                for link in links {
                    let text = match link.label {
                        Some(label) => format!("{label}: {}", link.url),
                        None => link.url,
                    };
                    self.push_detail(&text);
                }
            }
            ProviderAuthEvent::Progress { message } => {
                self.status = message;
            }
        }
    }

    /// `prompt(prompt)` — show a prompt the user must answer.
    pub fn set_prompt(&mut self, prompt: ProviderAuthPrompt) {
        if self.done {
            return;
        }
        self.input.set_value("");
        self.status = "Input required".to_owned();
        match prompt {
            ProviderAuthPrompt::Select { message, options } => {
                let to_search = |o: &SelectOption| {
                    format!("{} {}", o.label, o.description.clone().unwrap_or_default())
                        .trim()
                        .to_owned()
                };
                let list = SearchableList::new(options, to_search, Some(8), None, false);
                self.prompt = Some(ProviderAuthPrompt::Select {
                    message,
                    options: Vec::new(),
                });
                self.select_list = Some(list);
            }
            other => {
                self.prompt = Some(other);
                self.select_list = None;
            }
        }
    }

    fn resolve_prompt(&mut self, value: String) {
        if self.prompt.is_none() {
            return;
        }
        self.prompt = None;
        self.select_list = None;
        self.input.set_focused(false);
        self.status = "Continuing authentication…".to_owned();
        self.action = Some(LoginAction::Answer(value));
    }

    fn cancel(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        self.action = Some(LoginAction::Cancel);
    }

    fn push_detail(&mut self, value: &str) {
        if value.is_empty() || self.details.iter().any(|d| d == value) {
            return;
        }
        self.details.push(value.to_owned());
    }

    fn render_prompt_content(&mut self, inner_width: usize) -> Vec<String> {
        let theme = current_theme();
        let mut content: Vec<String> = Vec::new();
        let Some(prompt) = &self.prompt else {
            return content;
        };
        content.push(String::new());
        content.push(theme.fg(ColorToken::TextStrong, prompt.message()));
        if let ProviderAuthPrompt::Input {
            placeholder: Some(placeholder),
            ..
        } = prompt
        {
            content.push(theme.fg(ColorToken::TextMuted, &format!("e.g. {placeholder}")));
        }
        if matches!(prompt, ProviderAuthPrompt::Select { .. }) {
            let Some(list) = &self.select_list else {
                return content;
            };
            let view = list.view();
            for index in view.page.start..view.page.end {
                let Some(option) = view.items.get(index) else {
                    continue;
                };
                let selected = index == view.selected_index;
                let pointer = if selected { SELECT_POINTER } else { " " };
                let label = if selected {
                    theme.bold_fg(ColorToken::Primary, &option.label)
                } else {
                    theme.fg(ColorToken::Text, &option.label)
                };
                content.push(format!(
                    "{}{}",
                    theme.fg(
                        if selected {
                            ColorToken::Primary
                        } else {
                            ColorToken::TextDim
                        },
                        &format!(" {pointer} "),
                    ),
                    label
                ));
                if let Some(description) = &option.description {
                    content.push(theme.fg(ColorToken::TextMuted, &format!("   {description}")));
                }
            }
            content.push(String::new());
            content.push(theme.fg(
                ColorToken::TextMuted,
                "↑↓ navigate · Enter select · Esc cancel",
            ));
        } else {
            let rendered = self.input.render(inner_width);
            let is_secret = matches!(prompt, ProviderAuthPrompt::Input { secret: true, .. });
            let line = if is_secret && !self.input.get_value().is_empty() {
                mask_input_line(&rendered)
            } else {
                rendered
            };
            content.push(line);
            content.push(String::new());
            content.push(theme.fg(ColorToken::TextMuted, "Enter submit · Esc cancel"));
        }
        content
    }
}

/// `maskInputLine` — mask everything after the `> ` prompt except ANSI spans.
fn mask_input_line(raw: &str) -> String {
    let prefix = "> ";
    if !raw.starts_with(prefix) {
        return raw.to_owned();
    }
    let bytes = raw.as_bytes();
    let mut end = raw.len();
    while end > prefix.len() && bytes[end - 1] == b' ' {
        end -= 1;
    }
    let content = &raw[prefix.len()..end];
    let padding = &raw[end..];
    // Split on ANSI escape sequences, preserving them.
    let mut masked = String::new();
    let mut rest = content;
    while !rest.is_empty() {
        match rest.find("\x1b") {
            Some(pos) => {
                masked.push_str(&"•".repeat(pos));
                rest = &rest[pos..];
                // Consume the escape sequence `ESC[...m` or `ESC_pi:c BEL`.
                if let Some(close) = rest.find(['m', '\x07']) {
                    masked.push_str(&rest[..=close]);
                    rest = &rest[close + 1..];
                } else {
                    masked.push_str(rest);
                    rest = "";
                }
            }
            None => {
                masked.push_str(&"•".repeat(rest.len()));
                rest = "";
            }
        }
    }
    format!("{prefix}{masked}{padding}")
}

/// `renderBox` — the shared rounded box wrapper.
fn render_box(content: &[String], width: usize, inner_width: usize) -> Vec<String> {
    if width < 4 {
        return content
            .iter()
            .map(|line| truncate_to_width(line, width, "…", false))
            .collect();
    }
    let theme = current_theme();
    let border = |s: String| theme.fg(ColorToken::Primary, &s);
    let mut lines: Vec<String> = vec![
        String::new(),
        border(format!("╭{}╮", "─".repeat(width - 2))),
        border(format!("│{}│", " ".repeat(width - 2))),
    ];
    for line in content {
        let truncated = truncate_to_width(line, inner_width, "…", false);
        lines.push(border(format!(
            "│  {truncated}{}│",
            " ".repeat(inner_width.saturating_sub(visible_width(&truncated)))
        )));
    }
    lines.push(border(format!("│{}│", " ".repeat(width - 2))));
    lines.push(border(format!("╰{}╯", "─".repeat(width - 2))));
    lines.push(String::new());
    lines
        .iter()
        .map(|line| truncate_to_width(line, width, "…", false))
        .collect()
}

impl Component for ProviderLoginDialogComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.input.set_focused(
            self.focused && !matches!(self.prompt, Some(ProviderAuthPrompt::Select { .. })),
        );
        let safe_width = width;
        if safe_width == 0 {
            return vec![String::new()];
        }
        let inner_width = safe_width.saturating_sub(4).max(1);
        let theme = current_theme();

        let mut content: Vec<String> = vec![
            theme.bold_fg(
                ColorToken::TextStrong,
                &format!("Connect to {}", self.options.provider_name),
            ),
            theme.fg(ColorToken::TextMuted, &self.options.method_label),
            String::new(),
        ];
        for detail in &self.details {
            for line in wrap_text_with_ansi(detail, inner_width) {
                content.push(theme.fg(ColorToken::Text, &line));
            }
        }
        if !self.details.is_empty() {
            content.push(String::new());
        }
        content.push(theme.fg(ColorToken::TextDim, &self.status));

        content.extend(self.render_prompt_content(inner_width));
        if self.prompt.is_none() {
            content.push(String::new());
            content.push(theme.fg(ColorToken::TextMuted, "Esc cancel"));
        }

        render_box(&content, safe_width, inner_width)
    }

    fn handle_input(&mut self, data: &str) {
        if self.done {
            return;
        }
        if matches_key(data, "escape") || matches_key(data, "ctrl+c") || matches_key(data, "ctrl+d")
        {
            self.cancel();
            return;
        }
        match &self.prompt {
            Some(ProviderAuthPrompt::Select { .. }) => {
                if matches_key(data, "enter") {
                    let selected = self.select_list.as_ref().and_then(|l| l.selected());
                    if let Some(selected) = selected {
                        self.resolve_prompt(selected.id);
                    }
                    return;
                }
                if let Some(list) = self.select_list.as_mut() {
                    list.handle_key(data);
                }
            }
            // Input and any other non-select prompt variants: printable keys
            // feed the input line; Submit resolves the prompt.
            Some(_) if self.input.handle_input(data) == InputEvent::Submit => {
                let value = self.input.get_value().to_owned();
                let trimmed = value.trim().to_owned();
                if trimmed.is_empty() {
                    self.status = "A value is required.".to_owned();
                    return;
                }
                self.resolve_prompt(trimmed);
            }
            // Any other non-select prompt input (e.g. a running submit) just
            // feeds the input line without resolving.
            Some(_) => {}
            None => {}
        }
    }

    fn invalidate(&mut self) {}

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl Focusable for ProviderLoginDialogComponent {
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

    fn component() -> ProviderLoginDialogComponent {
        ProviderLoginDialogComponent::new(ProviderLoginDialogOptions {
            provider_name: "Anthropic".to_owned(),
            method_label: "OAuth".to_owned(),
        })
    }

    #[test]
    fn renders_title_and_status() {
        set_palette(DARK_COLORS);
        let mut c = component();
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("Connect to Anthropic"), "{joined}");
        assert!(joined.contains("OAuth"), "{joined}");
        assert!(joined.contains("Starting authentication…"), "{joined}");
        assert!(joined.contains("Esc cancel"), "{joined}");
    }

    #[test]
    fn notify_pushes_details() {
        set_palette(DARK_COLORS);
        let mut c = component();
        c.notify(ProviderAuthEvent::AuthUrl {
            instructions: "Visit the URL".to_owned(),
            url: "https://example.com/auth".to_owned(),
        });
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("Visit the URL"), "{joined}");
        assert!(
            joined.contains("Open: https://example.com/auth"),
            "{joined}"
        );
        assert!(joined.contains("Waiting for authorization…"), "{joined}");
    }

    #[test]
    fn input_prompt_answers() {
        set_palette(DARK_COLORS);
        let mut c = component();
        c.set_prompt(ProviderAuthPrompt::Input {
            message: "Paste your API key".to_owned(),
            placeholder: Some("sk-…".to_owned()),
            secret: false,
        });
        for ch in "sk-1234".chars() {
            c.handle_input(&ch.to_string());
        }
        c.handle_input("\r");
        assert_eq!(
            c.take_action(),
            Some(LoginAction::Answer("sk-1234".to_owned()))
        );
    }

    #[test]
    fn secret_input_masks() {
        set_palette(DARK_COLORS);
        let mut c = component();
        c.set_prompt(ProviderAuthPrompt::Input {
            message: "Secret".to_owned(),
            placeholder: None,
            secret: true,
        });
        for ch in "hunter2".chars() {
            c.handle_input(&ch.to_string());
        }
        let joined = plain(&c.render(80).join("\n"));
        assert!(!joined.contains("hunter2"), "{joined}");
        assert!(joined.contains("•"), "{joined}");
    }

    #[test]
    fn select_prompt_navigates() {
        set_palette(DARK_COLORS);
        let mut c = component();
        c.set_prompt(ProviderAuthPrompt::Select {
            message: "Pick an account".to_owned(),
            options: vec![
                SelectOption {
                    id: "a".to_owned(),
                    label: "Alpha".to_owned(),
                    description: Some("first".to_owned()),
                },
                SelectOption {
                    id: "b".to_owned(),
                    label: "Beta".to_owned(),
                    description: None,
                },
            ],
        });
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("Pick an account"), "{joined}");
        assert!(joined.contains("❯ Alpha"), "{joined}");
        assert!(
            joined.contains("↑↓ navigate · Enter select · Esc cancel"),
            "{joined}"
        );
        c.handle_input("\x1b[B"); // down
        c.handle_input("\r");
        assert_eq!(c.take_action(), Some(LoginAction::Answer("b".to_owned())));
    }

    #[test]
    fn escape_cancels() {
        set_palette(DARK_COLORS);
        let mut c = component();
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(LoginAction::Cancel));
    }
}
