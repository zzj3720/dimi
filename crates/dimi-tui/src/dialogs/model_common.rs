//! Shared model metadata helpers for the dialog components — port of the pure
//! functions in `apps/dimi/src/tui/components/dialogs/model-selector.ts`
//! (`effortLabel`, `modelDisplayName`, `providerDisplayName`,
//! `modelProviderName`, `createModelChoiceOptions`, `thinkingAvailability`,
//! `effortsOf`, `segmentsFor`, `defaultThinkingEffortFor`, `commitEffort`).
//!
//! These are reused by the model selector, effort selector, and (for
//! provider/title display) the settings selectors.

use crate::chrome::ChoiceOption;

/// `DEFAULT_OAUTH_PROVIDER_NAME` from `constant/app.ts`.
pub const DEFAULT_OAUTH_PROVIDER_NAME: &str = "kimi-coding";
/// `PRODUCT_NAME` from `constant/app.ts`.
pub const PRODUCT_NAME: &str = "Dimi";

/// `ThinkingAvailability` in model-selector.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingAvailability {
    Toggle,
    AlwaysOn,
    Unsupported,
}

/// Rust shape of the SDK `ModelAlias` (the fields the model selector reads:
/// `displayName`, `model`, `provider`, `capabilities`, `supportEfforts`,
/// `defaultEffort`).
#[derive(Debug, Clone)]
pub struct ModelAlias {
    pub model: String,
    pub display_name: Option<String>,
    pub provider: String,
    pub capabilities: Vec<String>,
    pub support_efforts: Vec<String>,
    pub default_effort: Option<String>,
}

impl ModelAlias {
    pub fn new(model: &str, display_name: Option<&str>, provider: &str) -> Self {
        ModelAlias {
            model: model.to_owned(),
            display_name: display_name.map(str::to_owned),
            provider: provider.to_owned(),
            capabilities: Vec::new(),
            support_efforts: Vec::new(),
            default_effort: None,
        }
    }
}

/// `modelDisplayName`: `model?.displayName ?? model?.model ?? alias`.
pub fn model_display_name(alias: &str, model: Option<&ModelAlias>) -> String {
    match model {
        Some(m) => m.display_name.clone().unwrap_or_else(|| m.model.clone()),
        None => alias.to_owned(),
    }
}

/// `providerDisplayName`: the oauth provider renders as the product name;
/// `managed:` prefixes are stripped.
pub fn provider_display_name(provider: &str) -> String {
    if provider == DEFAULT_OAUTH_PROVIDER_NAME {
        return PRODUCT_NAME.to_owned();
    }
    if let Some(stripped) = provider.strip_prefix("managed:") {
        return stripped.to_owned();
    }
    provider.to_owned()
}

/// `modelProviderName`.
pub fn model_provider_name(model: &ModelAlias) -> &str {
    &model.provider
}

/// `createModelChoiceOptions` — used by the choice-picker based flows.
///
/// `models` is an **ordered** `(alias, cfg)` list: the TS source iterates
/// `Object.entries(models)`, which preserves insertion order, so the order of
/// the returned options is the order of the caller-provided list (a Rust
/// `HashMap` has no order, so callers pass the order they want rendered).
pub fn create_model_choice_options(models: &[(String, ModelAlias)]) -> Vec<ChoiceOption> {
    models
        .iter()
        .map(|(alias, cfg)| {
            let name = model_display_name(alias, Some(cfg));
            let provider = provider_display_name(model_provider_name(cfg));
            ChoiceOption {
                value: alias.clone(),
                label: format!("{name} ({provider})"),
                description: None,
                tone: None,
                description_tone: None,
            }
        })
        .collect()
}

/// One row in the model selector list (`ModelChoice`).
#[derive(Debug, Clone)]
pub struct ModelChoice {
    pub alias: String,
    pub model: ModelAlias,
    /// Model display name (left column).
    pub name: String,
    /// Provider display name (right column).
    pub provider: String,
    /// Combined text the fuzzy filter matches against (name + provider).
    pub label: String,
}

/// `createModelChoices` — order preserved from the caller-provided `(alias,
/// cfg)` list (mirrors JS `Object.entries` insertion order).
pub fn create_model_choices(models: &[(String, ModelAlias)]) -> Vec<ModelChoice> {
    models
        .iter()
        .map(|(alias, cfg)| {
            let name = model_display_name(alias, Some(cfg));
            let provider = provider_display_name(model_provider_name(cfg));
            ModelChoice {
                alias: alias.clone(),
                model: cfg.clone(),
                name: name.clone(),
                provider: provider.clone(),
                label: format!("{name} ({provider})"),
            }
        })
        .collect()
}

/// `thinkingAvailability`.
pub fn thinking_availability(model: &ModelAlias) -> ThinkingAvailability {
    if model.capabilities.iter().any(|c| c == "always_thinking") {
        return ThinkingAvailability::AlwaysOn;
    }
    if model.capabilities.iter().any(|c| c == "thinking") {
        return ThinkingAvailability::Toggle;
    }
    ThinkingAvailability::Unsupported
}

/// `effortsOf` — `supportEfforts ?? []`.
pub fn efforts_of(model: &ModelAlias) -> &[String] {
    &model.support_efforts
}

/// `segmentsFor` — the ordered list of selectable thinking efforts.
pub fn segments_for(model: &ModelAlias) -> Vec<String> {
    let efforts = efforts_of(model);
    match thinking_availability(model) {
        ThinkingAvailability::AlwaysOn => {
            if efforts.is_empty() {
                vec!["on".to_owned()]
            } else {
                efforts.to_vec()
            }
        }
        ThinkingAvailability::Unsupported => {
            if efforts.is_empty() {
                vec!["off".to_owned()]
            } else {
                let mut seg = vec!["off".to_owned()];
                seg.extend_from_slice(efforts);
                seg
            }
        }
        ThinkingAvailability::Toggle => {
            if efforts.is_empty() {
                vec!["on".to_owned(), "off".to_owned()]
            } else {
                let mut seg = vec!["off".to_owned()];
                seg.extend_from_slice(efforts);
                seg
            }
        }
    }
}

/// `effortLabel` — capitalize the first character.
pub fn effort_label(effort: &str) -> String {
    if effort.is_empty() {
        return effort.to_owned();
    }
    let mut chars = effort.chars();
    let first = chars.next().unwrap_or_default();
    first.to_uppercase().collect::<String>() + chars.as_str()
}

/// `defaultThinkingEffortFor`.
pub fn default_thinking_effort_for(model: &ModelAlias) -> String {
    if thinking_availability(model) == ThinkingAvailability::Unsupported {
        return "off".to_owned();
    }
    let efforts = efforts_of(model);
    if !efforts.is_empty() {
        return model
            .default_effort
            .clone()
            .unwrap_or_else(|| efforts[efforts.len() / 2].clone());
    }
    "on".to_owned()
}

/// `commitEffort` — a boolean `'on'` never leaks past the UI boundary; it
/// becomes the model's default effort.
pub fn commit_effort(choice: &ModelChoice, draft: &str) -> String {
    if draft == "on" {
        return default_thinking_effort_for(&choice.model);
    }
    draft.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> Vec<(String, ModelAlias)> {
        vec![
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
                    provider: DEFAULT_OAUTH_PROVIDER_NAME.to_owned(),
                    capabilities: vec!["always_thinking".to_owned()],
                    support_efforts: vec!["low".to_owned(), "medium".to_owned(), "high".to_owned()],
                    default_effort: None,
                },
            ),
            (
                "legacy".to_owned(),
                ModelAlias {
                    model: "old-model".to_owned(),
                    display_name: None,
                    provider: "managed:self-hosted".to_owned(),
                    capabilities: Vec::new(),
                    support_efforts: Vec::new(),
                    default_effort: None,
                },
            ),
        ]
    }

    fn get<'a>(models: &'a [(String, ModelAlias)], alias: &str) -> Option<&'a ModelAlias> {
        models.iter().find(|(a, _)| a == alias).map(|(_, m)| m)
    }

    #[test]
    fn display_names() {
        let m = models();
        assert_eq!(model_display_name("sonnet", None), "sonnet");
        assert_eq!(model_display_name("sonnet", get(&m, "sonnet")), "Sonnet");
        // No displayName → model id.
        assert_eq!(model_display_name("legacy", get(&m, "legacy")), "old-model");
    }

    #[test]
    fn provider_names() {
        assert_eq!(provider_display_name(DEFAULT_OAUTH_PROVIDER_NAME), "Dimi");
        assert_eq!(provider_display_name("managed:self-hosted"), "self-hosted");
        assert_eq!(provider_display_name("anthropic"), "anthropic");
    }

    #[test]
    fn effort_label_capitalizes() {
        assert_eq!(effort_label("low"), "Low");
        assert_eq!(effort_label("always-on"), "Always-on");
        assert_eq!(effort_label(""), "");
    }

    #[test]
    fn availability_and_segments() {
        let m = models();
        assert_eq!(
            thinking_availability(get(&m, "sonnet").unwrap()),
            ThinkingAvailability::Toggle
        );
        assert_eq!(
            thinking_availability(get(&m, "kimi").unwrap()),
            ThinkingAvailability::AlwaysOn
        );
        assert_eq!(
            thinking_availability(get(&m, "legacy").unwrap()),
            ThinkingAvailability::Unsupported
        );
        // Toggle with efforts → off first.
        assert_eq!(
            segments_for(get(&m, "sonnet").unwrap()),
            vec!["off", "low", "high"]
        );
        // Always-on with efforts → efforts only.
        assert_eq!(
            segments_for(get(&m, "kimi").unwrap()),
            vec!["low", "medium", "high"]
        );
        // Unsupported without efforts → ["off"].
        assert_eq!(segments_for(get(&m, "legacy").unwrap()), vec!["off"]);
    }

    #[test]
    fn default_and_commit_effort() {
        let m = models();
        // Explicit defaultEffort wins.
        assert_eq!(
            default_thinking_effort_for(get(&m, "sonnet").unwrap()),
            "high"
        );
        // No default → middle effort.
        assert_eq!(
            default_thinking_effort_for(get(&m, "kimi").unwrap()),
            "medium"
        );
        // Unsupported → off.
        assert_eq!(
            default_thinking_effort_for(get(&m, "legacy").unwrap()),
            "off"
        );

        let choices = create_model_choices(&m);
        let sonnet = choices.iter().find(|c| c.alias == "sonnet").unwrap();
        assert_eq!(commit_effort(sonnet, "low"), "low");
        // boolean 'on' normalizes to the model default.
        assert_eq!(commit_effort(sonnet, "on"), "high");
    }

    #[test]
    fn choice_options_shape() {
        let m = models();
        let opts = create_model_choice_options(&m);
        let kimi = opts.iter().find(|o| o.value == "kimi").unwrap();
        assert_eq!(kimi.label, "Kimi K2 (Dimi)");
        let sonnet = opts.iter().find(|o| o.value == "sonnet").unwrap();
        assert_eq!(sonnet.label, "Sonnet (anthropic)");
    }

    #[test]
    fn choice_rows_preserve_order() {
        let m = models();
        let choices = create_model_choices(&m);
        let aliases: Vec<&str> = choices.iter().map(|c| c.alias.as_str()).collect();
        assert_eq!(aliases, vec!["sonnet", "kimi", "legacy"]);
        let kimi = choices.iter().find(|c| c.alias == "kimi").unwrap();
        assert_eq!(kimi.name, "Kimi K2");
        assert_eq!(kimi.provider, "Dimi");
        assert_eq!(kimi.label, "Kimi K2 (Dimi)");
    }
}
