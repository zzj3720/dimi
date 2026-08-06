//! Experimental features toggle list — port of
//! `apps/dimi/src/tui/components/dialogs/experiments-selector.ts`
//! (`ExperimentsSelectorComponent`).
//!
//! A searchable list where `Space` toggles each row's draft state in place;
//! `Enter` applies the accumulated changes. Rows from `env` / `master-env`
//! sources are locked and cannot be toggled.

use std::collections::HashMap;

use crate::component::Component;
use crate::keys::{decode_printable_key, matches_key};
use crate::searchable_list::SearchableList;
use crate::theme::{ColorToken, current_theme};
use crate::wrap::truncate_to_width;

use super::{SELECT_POINTER, wrap_text};

const ELLIPSIS: &str = "…";

/// `ExperimentalFeatureState['source']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureSource {
    Env,
    MasterEnv,
    Config,
    Default,
}

/// One experimental feature row.
#[derive(Debug, Clone)]
pub struct ExperimentalFeatureState {
    pub id: String,
    pub title: String,
    pub description: String,
    pub env: String,
    pub source: FeatureSource,
    pub enabled: bool,
}

/// A draft change (`ExperimentalFeatureDraftChange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftChange {
    pub id: String,
    pub enabled: bool,
}

/// Action surfaced via [`ExperimentsSelectorComponent::take_action`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExperimentsSelectorAction {
    Apply(Vec<DraftChange>),
    Cancel,
}

/// `ExperimentsSelectorOptions`.
#[derive(Debug, Clone)]
pub struct ExperimentsSelectorOptions {
    pub features: Vec<ExperimentalFeatureState>,
}

/// `ExperimentsSelectorComponent`.
pub struct ExperimentsSelectorComponent {
    opts: ExperimentsSelectorOptions,
    list: SearchableList<ExperimentalFeatureState>,
    draft: HashMap<String, bool>,
    action: Option<ExperimentsSelectorAction>,
}

impl ExperimentsSelectorComponent {
    pub fn new(opts: ExperimentsSelectorOptions) -> Self {
        let list = SearchableList::new(
            opts.features.clone(),
            |feature| format!("{} {} {}", feature.title, feature.id, feature.description),
            None,
            None,
            true,
        );
        ExperimentsSelectorComponent {
            opts,
            list,
            draft: HashMap::new(),
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<ExperimentsSelectorAction> {
        self.action.take()
    }

    fn is_locked(feature: &ExperimentalFeatureState) -> bool {
        matches!(
            feature.source,
            FeatureSource::Env | FeatureSource::MasterEnv
        )
    }

    fn toggle_draft(&mut self, feature: &ExperimentalFeatureState) {
        if Self::is_locked(feature) {
            return;
        }
        let enabled = !self.effective_enabled(feature);
        if enabled == feature.enabled {
            self.draft.remove(&feature.id);
            return;
        }
        self.draft.insert(feature.id.clone(), enabled);
    }

    fn effective_enabled(&self, feature: &ExperimentalFeatureState) -> bool {
        self.draft
            .get(&feature.id)
            .copied()
            .unwrap_or(feature.enabled)
    }

    fn is_draft_changed(&self, feature: &ExperimentalFeatureState) -> bool {
        self.effective_enabled(feature) != feature.enabled
    }

    fn draft_changes(&self) -> Vec<DraftChange> {
        let mut changes = Vec::new();
        for feature in &self.opts.features {
            if self.is_draft_changed(feature) {
                changes.push(DraftChange {
                    id: feature.id.clone(),
                    enabled: self.effective_enabled(feature),
                });
            }
        }
        changes
    }

    fn feature_detail(feature: &ExperimentalFeatureState) -> String {
        feature.detail()
    }

    fn render_apply_button(&self) -> String {
        let theme = current_theme();
        let changes = self.draft_changes();
        let count = changes.len();
        let label = "[ Apply changes and reload ]";
        let summary = if count == 0 {
            "no changes".to_owned()
        } else if count == 1 {
            "1 change".to_owned()
        } else {
            format!("{count} changes")
        };
        let button = if count == 0 {
            theme.fg(ColorToken::TextDim, label)
        } else {
            theme.bold_fg(ColorToken::Primary, label)
        };
        let summary_text = if count == 0 {
            theme.fg(ColorToken::TextMuted, &summary)
        } else {
            theme.fg(ColorToken::Success, &summary)
        };
        format!(" {button}  {summary_text}")
    }

    fn render_feature(
        &self,
        feature: &ExperimentalFeatureState,
        selected: bool,
        width: usize,
    ) -> Vec<String> {
        let theme = current_theme();
        let pointer = if selected { SELECT_POINTER } else { " " };
        let prefix = theme.fg(
            if selected {
                ColorToken::Primary
            } else {
                ColorToken::TextDim
            },
            &format!("  {pointer} "),
        );
        let label = if selected {
            theme.bold_fg(ColorToken::Primary, &feature.title)
        } else {
            theme.fg(ColorToken::Text, &feature.title)
        };
        let enabled = self.effective_enabled(feature);
        let status = if enabled { "enabled" } else { "disabled" };
        let status_text = if enabled {
            theme.fg(ColorToken::Success, status)
        } else {
            theme.fg(ColorToken::TextDim, status)
        };
        let detail = if self.is_draft_changed(feature) {
            format!("{} · modified", Self::feature_detail(feature))
        } else {
            Self::feature_detail(feature)
        };
        let mut lines = vec![
            format!("{prefix}{label}  {status_text}"),
            theme.fg(ColorToken::TextMuted, &format!("    {detail}")),
        ];
        let description_width = width.saturating_sub(4).max(1);
        for line in wrap_text(&feature.description, description_width) {
            lines.push(theme.fg(ColorToken::TextMuted, &format!("    {line}")));
        }
        lines
    }
}

fn source_label_for(feature: &ExperimentalFeatureState) -> String {
    match feature.source {
        FeatureSource::MasterEnv => "locked by DIMI_CODE_EXPERIMENTAL_FLAG".to_owned(),
        FeatureSource::Env => format!("locked by {}", feature.env),
        FeatureSource::Config => "config".to_owned(),
        FeatureSource::Default => "default".to_owned(),
    }
}

impl ExperimentalFeatureState {
    /// `featureDetail` — `id <id> · <source>` (env/master-env) or
    /// `id <id> · <source> · <env>`.
    fn detail(&self) -> String {
        if matches!(self.source, FeatureSource::Env | FeatureSource::MasterEnv) {
            format!("id {} · {}", self.id, source_label_for(self))
        } else {
            format!("id {} · {} · {}", self.id, source_label_for(self), self.env)
        }
    }
}

impl Component for ExperimentsSelectorComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let view = self.list.view();
        let title_suffix = if view.query.is_empty() {
            theme.fg(ColorToken::TextMuted, "  (type to search)")
        } else {
            String::new()
        };
        let mut hint_parts = vec!["↑↓ navigate"];
        if view.page.page_count > 1 {
            hint_parts.push("PgUp/PgDn page");
        }
        hint_parts.push("Space toggle");
        hint_parts.push("Enter apply");
        hint_parts.push("Esc cancel");
        if !view.query.is_empty() {
            hint_parts.push("Backspace clear");
        }

        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            format!(
                "{}{}",
                theme.bold_fg(ColorToken::Primary, " Experimental features"),
                title_suffix
            ),
            theme.fg(
                ColorToken::TextMuted,
                &format!(" {}", hint_parts.join(" · ")),
            ),
            String::new(),
        ];

        if !view.query.is_empty() {
            lines.push(format!(
                "{}{}",
                theme.fg(ColorToken::Primary, " Search: "),
                theme.fg(ColorToken::Text, &view.query)
            ));
        }

        if view.items.is_empty() {
            lines.push(theme.fg(ColorToken::TextMuted, "   No matches"));
        }

        for i in view.page.start..view.page.end {
            if let Some(feature) = view.items.get(i) {
                let selected = i == view.selected_index;
                lines.extend(self.render_feature(feature, selected, width));
            }
        }

        lines.push(String::new());
        if !view.query.is_empty() {
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &format!(" {} / {}", view.items.len(), self.opts.features.len()),
            ));
        } else if view.page.end < view.items.len() {
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &format!(" ▼ {} more", view.items.len() - view.page.end),
            ));
        }
        lines.push(self.render_apply_button());
        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, ELLIPSIS, false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") {
            if self.list.clear_query() {
                return;
            }
            self.action = Some(ExperimentsSelectorAction::Cancel);
            return;
        }
        if matches_key(data, "enter") {
            let changes = self.draft_changes();
            if !changes.is_empty() {
                self.action = Some(ExperimentsSelectorAction::Apply(changes));
            }
            return;
        }
        let decoded = decode_printable_key(data);
        if matches_key(data, "space") || decoded.as_deref() == Some(" ") {
            if let Some(selected) = self.list.selected() {
                self.toggle_draft(&selected);
            }
            return;
        }
        self.list.handle_key(data);
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(
        id: &str,
        title: &str,
        source: FeatureSource,
        enabled: bool,
    ) -> ExperimentalFeatureState {
        ExperimentalFeatureState {
            id: id.to_owned(),
            title: title.to_owned(),
            description: format!("{title} description"),
            env: format!("DIMI_EXP_{}", id.to_uppercase()),
            source,
            enabled,
        }
    }

    fn opts() -> ExperimentsSelectorOptions {
        ExperimentsSelectorOptions {
            features: vec![
                feature("steer", "Steer into turn", FeatureSource::Config, false),
                feature("swarm", "Swarm mode", FeatureSource::Default, true),
                feature("locked", "Locked flag", FeatureSource::Env, false),
            ],
        }
    }

    #[test]
    fn renders_rows_with_status() {
        let mut c = ExperimentsSelectorComponent::new(opts());
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Experimental features"), "{joined}");
        assert!(joined.contains("Space toggle"), "{joined}");
        assert!(joined.contains("Steer into turn"), "{joined}");
        assert!(joined.contains("enabled"), "{joined}");
        assert!(joined.contains("disabled"), "{joined}");
        assert!(
            joined.contains("id steer · config · DIMI_EXP_STEER"),
            "{joined}"
        );
        assert!(joined.contains("no changes"), "{joined}");
    }

    #[test]
    fn space_toggles_draft_and_apply_emits() {
        let mut c = ExperimentsSelectorComponent::new(opts());
        // select second feature (swarm, enabled) and toggle it off.
        c.handle_input("\x1b[B");
        c.handle_input(" ");
        c.handle_input("\r");
        match c.take_action() {
            Some(ExperimentsSelectorAction::Apply(changes)) => {
                assert_eq!(
                    changes,
                    vec![DraftChange {
                        id: "swarm".to_owned(),
                        enabled: false
                    }]
                );
            }
            other => panic!("expected apply, got {other:?}"),
        }
    }

    #[test]
    fn locked_features_ignore_space() {
        let mut c = ExperimentsSelectorComponent::new(opts());
        c.handle_input("\x1b[B");
        c.handle_input("\x1b[B"); // onto locked
        c.handle_input(" ");
        c.handle_input("\r");
        assert_eq!(c.take_action(), None); // no draft changes → no apply
    }

    #[test]
    fn escape_cancels() {
        let mut c = ExperimentsSelectorComponent::new(opts());
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(ExperimentsSelectorAction::Cancel));
    }

    #[test]
    fn detail_renders_sources() {
        let f = feature("x", "X", FeatureSource::Env, false);
        assert_eq!(f.detail(), "id x · locked by DIMI_EXP_X");
        let f2 = feature("y", "Y", FeatureSource::MasterEnv, false);
        assert_eq!(f2.detail(), "id y · locked by DIMI_CODE_EXPERIMENTAL_FLAG");
    }
}
