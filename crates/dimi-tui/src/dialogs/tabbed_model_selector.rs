//! Tabbed model selector — splits the model list into per-provider tabs over
//! the flat [`ModelSelectorComponent`]. Port of
//! `apps/dimi/src/tui/components/dialogs/tabbed-model-selector.ts`
//! (`TabbedModelSelectorComponent`).

use crate::component::{Component, Focusable};
use crate::dialogs::model_common::{model_provider_name, provider_display_name};
use crate::dialogs::model_selector::{ModelSelectorComponent, ModelSelectorOptions};
use crate::keys::matches_key;
use crate::tab_strip::render_tab_strip;
use crate::theme::current_theme;
use crate::wrap::truncate_to_width;

/// `TabbedModelSelectorOptions`.
#[derive(Debug, Clone)]
pub struct TabbedModelSelectorOptions {
    pub models: Vec<(String, crate::dialogs::model_common::ModelAlias)>,
    pub current_value: String,
    pub selected_value: Option<String>,
    pub current_thinking_effort: String,
    pub title: Option<String>,
    /// When set, the tab for this provider id opens active instead of "All".
    pub initial_tab_id: Option<String>,
    pub warning: Option<String>,
    pub has_session_only: bool,
}

struct ModelTab {
    id: String,
    label: String,
    selector: ModelSelectorComponent,
}

/// `TabbedModelSelectorComponent`.
pub struct TabbedModelSelectorComponent {
    tabs: Vec<ModelTab>,
    active_index: usize,
    focused: bool,
}

impl TabbedModelSelectorComponent {
    pub fn new(opts: TabbedModelSelectorOptions) -> Self {
        let tabs = build_tabs(&opts);
        let initial_tab_idx = opts
            .initial_tab_id
            .as_deref()
            .and_then(|id| tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        TabbedModelSelectorComponent {
            tabs,
            active_index: initial_tab_idx,
            focused: false,
        }
    }

    /// Host polls after `handle_input` (mirrors the inner selector's
    /// `onSelect` / `onCancel` / `onSessionOnlySelect`).
    pub fn take_action(&mut self) -> Option<crate::dialogs::model_selector::ModelSelectorAction> {
        self.tabs[self.active_index].selector.take_action()
    }

    fn sync_focus_to_active(&mut self) {
        // Inner selectors keep selection state; focus is tracked by the host.
        let _ = &self.tabs;
    }
}

/// `makeSelector` — build an inner flat selector for a model subset.
fn make_selector(
    opts: &TabbedModelSelectorOptions,
    subset: &[(String, crate::dialogs::model_common::ModelAlias)],
) -> ModelSelectorComponent {
    let candidate = opts
        .selected_value
        .as_deref()
        .unwrap_or(&opts.current_value);
    let selected_value = if subset.iter().any(|(alias, _)| alias == candidate) {
        Some(candidate.to_owned())
    } else {
        None
    };
    ModelSelectorComponent::new(ModelSelectorOptions {
        models: subset.to_vec(),
        current_value: opts.current_value.clone(),
        selected_value,
        current_thinking_effort: opts.current_thinking_effort.clone(),
        title: opts.title.clone(),
        searchable: true,
        page_size: None,
        provider_switch_hint: true,
        warning: opts.warning.clone(),
        has_session_only: opts.has_session_only,
    })
}

/// `buildTabs` — "All" tab first, then one tab per provider (insertion order).
fn build_tabs(opts: &TabbedModelSelectorOptions) -> Vec<ModelTab> {
    let mut provider_ids: Vec<String> = Vec::new();
    for (_, model) in &opts.models {
        let provider = model_provider_name(model).to_owned();
        if !provider_ids.contains(&provider) {
            provider_ids.push(provider);
        }
    }
    let mut tabs = vec![ModelTab {
        id: "all".to_owned(),
        label: "All".to_owned(),
        selector: make_selector(opts, &opts.models),
    }];
    for provider_id in &provider_ids {
        let subset: Vec<(String, crate::dialogs::model_common::ModelAlias)> = opts
            .models
            .iter()
            .filter(|(_, m)| model_provider_name(m) == provider_id)
            .cloned()
            .collect();
        tabs.push(ModelTab {
            id: provider_id.clone(),
            label: provider_display_name(provider_id),
            selector: make_selector(opts, &subset),
        });
    }
    tabs
}

impl Component for TabbedModelSelectorComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let active = &mut self.tabs[self.active_index];
        let inner = active.selector.render(width);
        if self.tabs.len() <= 1 {
            return inner
                .iter()
                .map(|line| truncate_to_width(line, width, "…", false))
                .collect();
        }
        // Layout: divider, title, hint, optional warning, blank, tab strip,
        // blank, then the model list. The header ends at its first blank line.
        let labels: Vec<&str> = self.tabs.iter().map(|t| t.label.as_str()).collect();
        let strip_line =
            render_tab_strip(&labels, self.active_index, width, current_theme().palette());
        let header_end = inner.iter().position(|line| line.is_empty());
        let split_at = header_end.unwrap_or(3);
        let mut out: Vec<String> = Vec::new();
        for line in inner.iter().take(split_at + 1) {
            out.push(line.clone());
        }
        out.push(strip_line);
        out.push(String::new());
        for line in inner.iter().skip(split_at + 1) {
            out.push(line.clone());
        }
        out.iter()
            .map(|line| truncate_to_width(line, width, "…", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if self.tabs.len() > 1 {
            if matches_key(data, "tab") {
                self.active_index = (self.active_index + 1) % self.tabs.len();
                self.sync_focus_to_active();
                return;
            }
            if matches_key(data, "shift+tab") {
                self.active_index = (self.active_index + self.tabs.len() - 1) % self.tabs.len();
                self.sync_focus_to_active();
                return;
            }
        }
        self.tabs[self.active_index].selector.handle_input(data);
    }

    fn invalidate(&mut self) {
        for tab in &mut self.tabs {
            tab.selector.invalidate();
        }
    }

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl Focusable for TabbedModelSelectorComponent {
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
    use crate::dialogs::model_common::ModelAlias;
    use crate::theme::{DARK_COLORS, set_palette};

    fn plain(joined: &str) -> String {
        crate::ansi::strip_ansi(joined)
    }

    fn alias(model: &str, provider: &str) -> ModelAlias {
        ModelAlias {
            model: model.to_owned(),
            display_name: None,
            provider: provider.to_owned(),
            capabilities: vec![],
            support_efforts: vec![],
            default_effort: None,
        }
    }

    fn opts() -> TabbedModelSelectorOptions {
        TabbedModelSelectorOptions {
            models: vec![
                ("sonnet".to_owned(), alias("claude-sonnet", "anthropic")),
                ("kimi".to_owned(), alias("kimi-k2", "kimi-coding")),
                ("opus".to_owned(), alias("claude-opus", "anthropic")),
            ],
            current_value: "sonnet".to_owned(),
            selected_value: None,
            current_thinking_effort: "high".to_owned(),
            title: None,
            initial_tab_id: None,
            warning: None,
            has_session_only: false,
        }
    }

    #[test]
    fn tabs_built_in_order() {
        set_palette(DARK_COLORS);
        let c = TabbedModelSelectorComponent::new(opts());
        let ids: Vec<&str> = c.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["all", "anthropic", "kimi-coding"]);
    }

    #[test]
    fn renders_tab_strip_in_header() {
        set_palette(DARK_COLORS);
        let mut c = TabbedModelSelectorComponent::new(opts());
        let lines = c.render(80);
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains(" All "), "{joined}");
        assert!(joined.contains(" anthropic "), "{joined}");
        // Strip appears after the header blank line, before the model list.
        let strip_idx = lines
            .iter()
            .position(|l| plain(l).contains(" anthropic "))
            .expect("strip present");
        assert!(plain(&lines[strip_idx + 1]).is_empty(), "blank after strip");
    }

    #[test]
    fn tab_cycles_and_forwards_input() {
        set_palette(DARK_COLORS);
        let mut c = TabbedModelSelectorComponent::new(opts());
        assert_eq!(c.active_index, 0);
        c.handle_input("\t");
        assert_eq!(c.active_index, 1);
        c.handle_input("\x1b[Z"); // shift+tab
        assert_eq!(c.active_index, 0);
    }

    #[test]
    fn initial_tab_opens_provider() {
        set_palette(DARK_COLORS);
        let mut o = opts();
        o.initial_tab_id = Some("anthropic".to_owned());
        let c = TabbedModelSelectorComponent::new(o);
        assert_eq!(c.active_index, 1);
        assert_eq!(c.tabs[1].id, "anthropic");
    }
}
