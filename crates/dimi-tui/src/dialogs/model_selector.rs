//! Flat, searchable single-list model picker — port of
//! `apps/dimi/src/tui/components/dialogs/model-selector.ts`
//! (`ModelSelectorComponent`).
//!
//! One navigation axis: ↑/↓ move the cursor (PgUp/PgDn page), typing
//! fuzzy-filters across every provider (provider name included), and ←/→
//! toggle the thinking draft for models that support it. There are no provider
//! tabs — filtering by typing a provider name replaces them. See
//! `.agents/skills/write-tui/DESIGN.md`.

use std::collections::HashMap;

use crate::component::Component;
use crate::keys::matches_key;
use crate::searchable_list::SearchableList;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::{truncate_to_width, wrap_text_with_ansi};

use super::model_common::{
    ModelAlias, ModelChoice, ThinkingAvailability, commit_effort, create_model_choices,
    effort_label, efforts_of, segments_for, thinking_availability,
};
use super::{CURRENT_MARK, SELECT_POINTER};

/// A chosen model + its normalized thinking effort (`ModelSelection`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub alias: String,
    pub thinking: String,
}

/// Action the host reacts to after `handle_input` (mirrors the TS
/// `onSelect` / `onSessionOnlySelect` / `onCancel` callbacks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSelectorAction {
    Select(ModelSelection),
    SessionOnly(ModelSelection),
    Cancel,
}

/// `ModelSelectorOptions`.
#[derive(Debug, Clone)]
pub struct ModelSelectorOptions {
    /// Ordered `(alias, cfg)` model list — order matches JS `Object.entries`.
    pub models: Vec<(String, ModelAlias)>,
    pub current_value: String,
    pub selected_value: Option<String>,
    /// Live thinking effort of the currently active model.
    pub current_thinking_effort: String,
    /// Overrides the default `' Select a model'` title line.
    pub title: Option<String>,
    /// When true, typed characters filter the list (fuzzy).
    pub searchable: bool,
    /// Items per page; defaults to 8.
    pub page_size: Option<usize>,
    /// When true, the hint line mentions the Tab provider switch.
    pub provider_switch_hint: bool,
    /// Rendered as warning-colored lines directly below the key-hint line.
    pub warning: Option<String>,
    /// Whether `onSessionOnlySelect` (Alt+S) is available.
    pub has_session_only: bool,
}

/// `ModelSelectorComponent`.
pub struct ModelSelectorComponent {
    opts: ModelSelectorOptions,
    list: SearchableList<ModelChoice>,
    /// Per-model thinking-effort override set by ←/→.
    thinking_overrides: HashMap<String, String>,
    action: Option<ModelSelectorAction>,
}

impl ModelSelectorComponent {
    pub fn new(opts: ModelSelectorOptions) -> Self {
        let choices = create_model_choices(&opts.models);
        let selected_value = opts
            .selected_value
            .clone()
            .unwrap_or_else(|| opts.current_value.clone());
        let selected_idx = choices
            .iter()
            .position(|choice| choice.alias == selected_value)
            .unwrap_or(0);
        let list = SearchableList::new(
            choices,
            |choice| choice.label.clone(),
            opts.page_size,
            Some(selected_idx),
            opts.searchable,
        );
        ModelSelectorComponent {
            opts,
            list,
            thinking_overrides: HashMap::new(),
            action: None,
        }
    }

    /// Host polls after `handle_input` (mirrors the TS callbacks).
    pub fn take_action(&mut self) -> Option<ModelSelectorAction> {
        self.action.take()
    }

    /// Currently selected row (for host-side reads).
    pub fn get_selected_alias(&self) -> Option<String> {
        self.list.selected().map(|c| c.alias)
    }

    /// Thinking effort for a model: an explicit ←/→ override when set,
    /// otherwise the live effort for the active model, otherwise the model's
    /// default effort (effort-capable) or `'on'` (other thinking-capable
    /// models). Port of `draftFor`.
    fn draft_for(&self, choice: &ModelChoice) -> String {
        if let Some(override_effort) = self.thinking_overrides.get(&choice.alias) {
            return override_effort.clone();
        }
        if choice.alias == self.opts.current_value {
            return self.opts.current_thinking_effort.clone();
        }
        let efforts = efforts_of(&choice.model);
        if !efforts.is_empty() {
            let def = choice
                .model
                .default_effort
                .clone()
                .unwrap_or_else(|| efforts[efforts.len() / 2].clone());
            if efforts.contains(&def) {
                return def;
            }
            return efforts[0].clone();
        }
        if thinking_availability(&choice.model) != ThinkingAvailability::Unsupported {
            "on".to_owned()
        } else {
            "off".to_owned()
        }
    }

    /// Draft coerced onto the model's segment list so rendering/selection
    /// never reference an effort the model cannot actually select. Port of
    /// `effectiveEffort`.
    fn effective_effort(&self, choice: &ModelChoice) -> String {
        let draft = self.draft_for(choice);
        let segments = segments_for(&choice.model);
        if segments.contains(&draft) {
            draft
        } else {
            segments[0].clone()
        }
    }

    fn selected_choice(&self) -> Option<ModelChoice> {
        self.list.selected()
    }

    /// Port of `renderThinkingControl` — the `[ On ] Off` segmented control.
    fn render_thinking_control(&self, choice: &ModelChoice) -> String {
        let theme = current_theme();
        let segment = |label: &str, active: bool| -> String {
            if active {
                theme.bold_fg(ColorToken::Primary, &format!("[ {label} ]"))
            } else {
                theme.fg(ColorToken::Text, &format!("  {label}  "))
            }
        };
        // The whole segment is muted, suffix included, so the disabled side
        // reads as a single greyed-out control rather than a selectable option.
        let unavailable = |label: &str| -> String {
            theme.fg(ColorToken::TextMuted, &format!("  {label} (Unsupported)  "))
        };

        // Non-effort always-on / unsupported models keep the original On/Off
        // layout so the control never shifts while moving across legacy models.
        let efforts = efforts_of(&choice.model);
        let availability = thinking_availability(&choice.model);
        if efforts.is_empty() && availability == ThinkingAvailability::AlwaysOn {
            return format!("  {} {}", segment("On", true), unavailable("Off"));
        }
        if efforts.is_empty() && availability == ThinkingAvailability::Unsupported {
            return format!("  {} {}", unavailable("On"), segment("Off", true));
        }

        let segments = segments_for(&choice.model);
        let active = self.effective_effort(choice);
        let rendered: Vec<String> = segments
            .iter()
            .map(|effort| segment(&effort_label(effort), *effort == active))
            .collect();
        format!("  {}", rendered.join("  "))
    }
}

impl Component for ModelSelectorComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let searchable = self.opts.searchable;
        let view = self.list.view();
        let total_count = self.opts.models.len();

        let title_suffix = if searchable && view.query.is_empty() {
            theme.fg(ColorToken::TextMuted, "  (type to search)")
        } else {
            String::new()
        };

        // "type to search" already lives in the title suffix, so the hint only
        // surfaces the backspace shortcut once a query is active.
        let mut hint_parts: Vec<&str> = Vec::new();
        if self.opts.provider_switch_hint {
            hint_parts.push("Tab toggle provider");
        }
        hint_parts.push("↑↓ navigate");
        if searchable && !view.query.is_empty() {
            hint_parts.push("Backspace clear");
        }
        hint_parts.push("Enter select");
        if self.opts.has_session_only {
            hint_parts.push("Alt+S session-only");
        }
        hint_parts.push("Esc cancel");

        let title = self
            .opts
            .title
            .clone()
            .unwrap_or_else(|| " Select a model".to_owned());
        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            format!(
                "{}{}",
                theme.bold_fg(ColorToken::Primary, &title),
                title_suffix
            ),
            theme.fg(
                ColorToken::TextMuted,
                &format!(" {}", hint_parts.join(" · ")),
            ),
        ];
        if let Some(warning) = &self.opts.warning {
            for line in wrap_text_with_ansi(warning, width.saturating_sub(1).max(1)) {
                lines.push(theme.fg(ColorToken::Warning, &format!(" {line}")));
            }
        }
        lines.push(String::new());

        if searchable && !view.query.is_empty() {
            lines.push(format!(
                "{}{}",
                theme.fg(ColorToken::Primary, " Search: "),
                theme.fg(ColorToken::Text, &view.query)
            ));
        }

        if view.items.is_empty() {
            lines.push(theme.fg(ColorToken::TextMuted, "   No matches"));
        } else {
            // Column width for model names so the provider column lines up.
            // Capped so the provider + "← current" marker still fit.
            let name_cap = 8usize.max(width / 2);
            let mut name_width = 0usize;
            for i in view.page.start..view.page.end {
                if let Some(choice) = view.items.get(i) {
                    name_width = name_width.max(visible_width(&choice.name));
                }
            }
            name_width = name_width.min(name_cap);

            for i in view.page.start..view.page.end {
                let Some(choice) = view.items.get(i) else {
                    continue;
                };
                let is_selected = i == view.selected_index;
                let is_current = choice.alias == self.opts.current_value;
                let pointer = if is_selected { SELECT_POINTER } else { " " };
                let truncated_name = truncate_to_width(&choice.name, name_width, "…", false);
                let name_pad =
                    " ".repeat(name_width.saturating_sub(visible_width(&truncated_name)));
                let mut line = theme.fg(
                    if is_selected {
                        ColorToken::Primary
                    } else {
                        ColorToken::TextDim
                    },
                    &format!("  {pointer} "),
                );
                line.push_str(&if is_selected {
                    theme.bold_fg(ColorToken::Primary, &truncated_name)
                } else {
                    theme.fg(ColorToken::Text, &truncated_name)
                });
                line.push_str(&name_pad);
                line.push_str(&format!(
                    "  {}",
                    theme.fg(ColorToken::TextMuted, &choice.provider)
                ));
                if is_current {
                    line.push_str(&format!(" {}", theme.fg(ColorToken::Success, CURRENT_MARK)));
                }
                lines.push(line);
            }
        }

        // Scroll / match indicator.
        if !view.query.is_empty() {
            lines.push(String::new());
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &format!(" {} / {total_count}", view.items.len()),
            ));
        } else {
            let below = view.items.len().saturating_sub(view.page.end);
            if below > 0 {
                lines.push(String::new());
                lines.push(theme.fg(ColorToken::TextMuted, &format!(" ▼ {below} more")));
            }
        }

        lines.push(String::new());
        if let Some(selected) = self.selected_choice() {
            let can_switch = segments_for(&selected.model).len() > 1;
            let thinking_header = if can_switch {
                " Thinking  (←→ to switch)"
            } else {
                " Thinking"
            };
            lines.push(theme.fg(ColorToken::TextMuted, thinking_header));
            lines.push(self.render_thinking_control(&selected));
        }
        lines.push(String::new());
        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "...", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") {
            if self.list.clear_query() {
                return;
            }
            self.action = Some(ModelSelectorAction::Cancel);
            return;
        }

        // ↑/↓, PgUp/PgDn, and — when searchable — typing + Backspace.
        if self.list.handle_key(data) {
            return;
        }

        // Left/Right move the active thinking effort within the model's
        // segments.
        if matches_key(data, "left") || matches_key(data, "right") {
            if let Some(selected) = self.selected_choice() {
                let segments = segments_for(&selected.model);
                if segments.len() > 1 {
                    let current = self.effective_effort(&selected);
                    let idx = segments.iter().position(|s| *s == current).unwrap_or(0);
                    // The two-segment case is the legacy boolean On/Off
                    // control: both arrows flip it.
                    let next: usize = if segments.len() == 2 {
                        if idx == 0 { 1 } else { 0 }
                    } else {
                        let delta: isize = if matches_key(data, "left") { -1 } else { 1 };
                        (idx as isize + delta).clamp(0, (segments.len() - 1) as isize) as usize
                    };
                    if next != idx {
                        self.thinking_overrides
                            .insert(selected.alias, segments[next].clone());
                    }
                }
            }
            return;
        }

        if matches_key(data, "enter") {
            if let Some(selected) = self.selected_choice() {
                self.action = Some(ModelSelectorAction::Select(ModelSelection {
                    alias: selected.alias.clone(),
                    thinking: commit_effort(&selected, &self.effective_effort(&selected)),
                }));
            }
            return;
        }

        if matches_key(data, "alt+s") && self.opts.has_session_only {
            if let Some(selected) = self.selected_choice() {
                self.action = Some(ModelSelectorAction::SessionOnly(ModelSelection {
                    alias: selected.alias.clone(),
                    thinking: commit_effort(&selected, &self.effective_effort(&selected)),
                }));
            }
        }
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ModelSelectorOptions {
        ModelSelectorOptions {
            models: vec![
                (
                    "sonnet".to_owned(),
                    ModelAlias {
                        model: "claude-sonnet".to_owned(),
                        display_name: Some("Sonnet".to_owned()),
                        provider: "anthropic".to_owned(),
                        capabilities: vec!["thinking".to_owned()],
                        support_efforts: vec!["low".to_owned(), "high".to_owned()],
                        default_effort: Some("high".to_owned()),
                    },
                ),
                (
                    "kimi".to_owned(),
                    ModelAlias {
                        model: "kimi-k2".to_owned(),
                        display_name: Some("Kimi K2".to_owned()),
                        provider: super::super::model_common::DEFAULT_OAUTH_PROVIDER_NAME
                            .to_owned(),
                        capabilities: vec!["always_thinking".to_owned()],
                        support_efforts: vec![
                            "low".to_owned(),
                            "medium".to_owned(),
                            "high".to_owned(),
                        ],
                        default_effort: None,
                    },
                ),
            ],
            current_value: "sonnet".to_owned(),
            selected_value: None,
            current_thinking_effort: "high".to_owned(),
            title: None,
            searchable: true,
            page_size: None,
            provider_switch_hint: false,
            warning: None,
            has_session_only: true,
        }
    }

    #[test]
    fn renders_title_hint_and_rows() {
        let mut c = ModelSelectorComponent::new(opts());
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains(" Select a model"), "{joined}");
        assert!(joined.contains("↑↓ navigate"), "{joined}");
        assert!(joined.contains("Alt+S session-only"), "{joined}");
        assert!(joined.contains("Sonnet"), "{joined}");
        assert!(joined.contains("Kimi K2"), "{joined}");
        assert!(joined.contains("anthropic"), "{joined}");
        assert!(joined.contains("← current"), "{joined}");
        assert!(joined.contains("Thinking"), "{joined}");
        // Top and bottom border.
        assert!(joined.starts_with('─'));
    }

    #[test]
    fn enter_selects_current_row() {
        let mut c = ModelSelectorComponent::new(opts());
        c.handle_input("\x1b[B"); // down to kimi
        c.handle_input("\r");
        match c.take_action() {
            Some(ModelSelectorAction::Select(sel)) => {
                assert_eq!(sel.alias, "kimi");
                // kimi is always-on, no draft → default effort (middle).
                assert_eq!(sel.thinking, "medium");
            }
            other => panic!("expected select, got {other:?}"),
        }
    }

    #[test]
    fn escape_cancels_and_query_clears_first() {
        let mut c = ModelSelectorComponent::new(opts());
        c.handle_input("x"); // fuzzy search
        c.handle_input("\x1b"); // first esc clears query
        assert!(c.take_action().is_none());
        c.handle_input("\x1b"); // second esc cancels
        assert_eq!(c.take_action(), Some(ModelSelectorAction::Cancel));
    }

    #[test]
    fn left_right_toggles_thinking() {
        let mut c = ModelSelectorComponent::new(opts());
        // sonnet is current with effort 'high' (live). Left → previous segment.
        c.handle_input("\x1b[D"); // left
        c.handle_input("\r");
        match c.take_action() {
            Some(ModelSelectorAction::Select(sel)) => {
                assert_eq!(sel.alias, "sonnet");
                // segments for sonnet: off, low, high. high→left→low.
                assert_eq!(sel.thinking, "low");
            }
            other => panic!("expected select, got {other:?}"),
        }
    }

    #[test]
    fn alt_s_is_session_only() {
        let mut c = ModelSelectorComponent::new(opts());
        c.handle_input("\x1bs"); // alt+s
        match c.take_action() {
            Some(ModelSelectorAction::SessionOnly(sel)) => {
                assert_eq!(sel.alias, "sonnet");
            }
            other => panic!("expected session-only, got {other:?}"),
        }
    }
}
