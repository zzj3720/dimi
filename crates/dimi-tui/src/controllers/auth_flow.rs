//! Auth flow controller — the pure parts of `apps/dimi/src/tui/controllers/
//! auth-flow.ts`.
//!
//! The TS controller is almost entirely async SDK orchestration
//! (`createSession`, `setModel`, `refreshModels`, …). This port keeps the
//! pure state patches (`enterLoginRequiredStartupState`,
//! `refreshConfigAfterLogout`), the provider-model projection helpers
//! (`groupModelIds`, `refreshProviderModels` diff), and the session-option
//! builder. Everything async / SDK is `// TODO(legacy)`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// A provider model entry (the `auth.models()` rows the TUI reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModel {
    pub provider: String,
    pub id: String,
    /// `name` — display name (`Model.name`).
    pub name: Option<String>,
    /// `contextWindow` → `ModelAlias.maxContextSize` (read by
    /// `refreshConfigAfterLogin`).
    pub context_window: Option<u64>,
    /// `maxTokens` → `ModelAlias.maxOutputSize`.
    pub max_tokens: Option<u64>,
    /// `input` capability tokens (e.g. `["text", "image"]`) — `image` yields
    /// the `image_in` capability.
    pub input: Vec<String>,
    /// `reasoning` — thinking capability.
    pub reasoning: bool,
    /// `thinkingLevelMap` — level → value (map value `None` = explicit `null`).
    pub thinking_level_map: BTreeMap<String, Option<Value>>,
    /// `defaultThinkingLevel` — the default effort when supported.
    pub default_thinking_level: Option<String>,
}

/// `ModelAlias` — the projected model alias the TUI stores in
/// `availableModels` (`providerModelToAlias` in `provider-model.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAlias {
    pub provider: String,
    pub model: String,
    pub display_name: Option<String>,
    pub max_context_size: Option<u64>,
    pub max_output_size: Option<u64>,
    pub capabilities: Vec<String>,
    pub support_efforts: Vec<String>,
    pub default_effort: Option<String>,
}

/// A provider identity (`auth.providers()` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
}

/// One provider whose model list changed after a refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderChange {
    pub provider_id: String,
    pub provider_name: String,
    pub added: usize,
    pub removed: usize,
}

/// `RefreshResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelDiff {
    pub changed: Vec<ProviderChange>,
    pub unchanged: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// The app-state fields the auth flow patches, as a diff-able patch.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppStatePatch {
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub thinking_effort: Option<String>,
    pub context_tokens: Option<u64>,
    pub max_context_tokens: Option<u64>,
    pub context_usage: Option<f64>,
    pub session_usage: Option<Value>,
    pub latest_prompt_usage: Option<Value>,
    pub session_title: Option<String>,
}

/// `enterLoginRequiredStartupState` — the app-state reset applied when the
/// startup session requires a login.
pub fn login_required_reset() -> AppStatePatch {
    AppStatePatch {
        session_id: Some(String::new()),
        model: Some(String::new()),
        thinking_effort: Some("off".to_owned()),
        context_tokens: Some(0),
        max_context_tokens: Some(0),
        context_usage: Some(0.0),
        session_usage: Some(Value::Null),
        latest_prompt_usage: Some(Value::Null),
        session_title: None,
    }
}

/// `refreshConfigAfterLogout` — the app-state reset applied after logout.
pub fn logout_reset() -> AppStatePatch {
    AppStatePatch {
        model: Some(String::new()),
        thinking_effort: Some("off".to_owned()),
        max_context_tokens: Some(0),
        context_usage: Some(0.0),
        context_tokens: Some(0),
        session_usage: Some(Value::Null),
        latest_prompt_usage: Some(Value::Null),
        ..AppStatePatch::default()
    }
}

/// `clearActiveSessionAfterLogout` patch — sessionId/model/title cleared.
pub fn active_session_logout_reset() -> AppStatePatch {
    AppStatePatch {
        session_id: Some(String::new()),
        model: Some(String::new()),
        session_title: Some(String::new()),
        ..AppStatePatch::default()
    }
}

/// The model alias key — `` `${model.provider}/${model.id}` ``.
pub fn model_alias_key(provider: &str, id: &str) -> String {
    format!("{provider}/{id}")
}

/// `groupModelIds` — per-provider set of model ids among the `included`
/// providers.
pub fn group_model_ids(
    models: &[ProviderModel],
    included: &BTreeSet<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for model in models {
        if !included.contains(&model.provider) {
            continue;
        }
        out.entry(model.provider.clone())
            .or_default()
            .insert(model.id.clone());
    }
    out
}

/// `refreshProviderModels` diff — compare the model projections before/after a
/// refresh, collecting added/removed counts per provider.
pub fn compute_provider_model_diff(
    before: &BTreeMap<String, BTreeSet<String>>,
    after: &BTreeMap<String, BTreeSet<String>>,
    included: &BTreeSet<String>,
    provider_names: &BTreeMap<String, String>,
    failures: &[(String, String)],
) -> ProviderModelDiff {
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for provider in included {
        let previous = before.get(provider).cloned().unwrap_or_default();
        let next = after.get(provider).cloned().unwrap_or_default();
        let added = next.difference(&previous).count();
        let removed = previous.difference(&next).count();
        if added == 0 && removed == 0 {
            unchanged.push(provider.clone());
        } else {
            let provider_name = provider_names
                .get(provider)
                .cloned()
                .unwrap_or_else(|| provider.clone());
            changed.push(ProviderChange {
                provider_id: provider.clone(),
                provider_name,
                added,
                removed,
            });
        }
    }
    ProviderModelDiff {
        changed,
        unchanged,
        failed: failures.to_vec(),
    }
}

/// `providerModelToAlias` — project a [`ProviderModel`] into its
/// [`ModelAlias`] (port of `provider-model.ts`).
pub fn provider_model_to_alias(model: &ProviderModel) -> ModelAlias {
    let support_efforts: Vec<String> = model
        .thinking_level_map
        .iter()
        .filter(|(level, value)| level.as_str() != "off" && value.is_some())
        .map(|(level, _)| level.clone())
        .collect();
    let default_effort = if support_efforts
        .iter()
        .any(|e| Some(e.as_str()) == model.default_thinking_level.as_deref())
    {
        model.default_thinking_level.clone()
    } else {
        support_efforts.get(support_efforts.len() / 2).cloned()
    };
    let mut capabilities: Vec<String> = Vec::new();
    if model.input.iter().any(|i| i == "image") {
        capabilities.push("image_in".to_owned());
    }
    if model.reasoning {
        // `always_thinking` when `thinkingLevelMap['off']` is explicitly null.
        let always_thinking = model
            .thinking_level_map
            .get("off")
            .is_some_and(|value| value.is_none());
        capabilities.push(if always_thinking {
            "always_thinking".to_owned()
        } else {
            "thinking".to_owned()
        });
    }
    capabilities.push("tool_use".to_owned());
    ModelAlias {
        provider: model.provider.clone(),
        model: model.id.clone(),
        display_name: model.name.clone(),
        max_context_size: model.context_window,
        max_output_size: model.max_tokens,
        capabilities,
        support_efforts,
        default_effort,
    }
}

/// Build the model map + provider map from raw lists
/// (`refreshAvailableModels`).
pub fn build_available_maps(
    models: &[ProviderModel],
    providers: &[ProviderInfo],
) -> (BTreeMap<String, ModelAlias>, BTreeMap<String, Value>) {
    let available_models: BTreeMap<String, ModelAlias> = models
        .iter()
        .map(|m| {
            (
                model_alias_key(&m.provider, &m.id),
                provider_model_to_alias(m),
            )
        })
        .collect();
    let available_providers: BTreeMap<String, Value> = providers
        .iter()
        .map(|p| (p.id.clone(), Value::String(p.name.clone())))
        .collect();
    (available_models, available_providers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_required_reset_clears_session_state() {
        let patch = login_required_reset();
        assert_eq!(patch.session_id.as_deref(), Some(""));
        assert_eq!(patch.model.as_deref(), Some(""));
        assert_eq!(patch.thinking_effort.as_deref(), Some("off"));
        assert_eq!(patch.context_tokens, Some(0));
        assert_eq!(patch.max_context_tokens, Some(0));
        assert_eq!(patch.context_usage, Some(0.0));
        assert_eq!(patch.session_usage, Some(Value::Null));
        assert_eq!(patch.latest_prompt_usage, Some(Value::Null));
    }

    #[test]
    fn logout_reset_clears_models_and_usage() {
        let patch = logout_reset();
        assert_eq!(patch.model.as_deref(), Some(""));
        assert_eq!(patch.thinking_effort.as_deref(), Some("off"));
        assert_eq!(patch.max_context_tokens, Some(0));
        assert_eq!(patch.context_usage, Some(0.0));
        assert_eq!(patch.session_usage, Some(Value::Null));
        // session id is left alone on logout.
        assert_eq!(patch.session_id, None);
    }

    #[test]
    fn group_model_ids_builds_per_provider_sets() {
        let model = |provider: &str, id: &str| ProviderModel {
            provider: provider.to_owned(),
            id: id.to_owned(),
            name: None,
            context_window: None,
            max_tokens: None,
            input: Vec::new(),
            reasoning: false,
            thinking_level_map: BTreeMap::new(),
            default_thinking_level: None,
        };
        let models = vec![model("p1", "a"), model("p1", "b"), model("p2", "c")];
        let included: BTreeSet<String> = ["p1".to_owned()].into_iter().collect();
        let grouped = group_model_ids(&models, &included);
        assert_eq!(
            grouped.get("p1").unwrap(),
            &BTreeSet::from(["a".to_owned(), "b".to_owned()])
        );
        assert!(!grouped.contains_key("p2"));
    }

    #[test]
    fn provider_model_diff_reports_added_removed_unchanged() {
        let before: BTreeMap<String, BTreeSet<String>> = BTreeMap::from([
            (
                "p1".to_owned(),
                BTreeSet::from(["a".to_owned(), "old".to_owned()]),
            ),
            ("p2".to_owned(), BTreeSet::from(["x".to_owned()])),
        ]);
        let after: BTreeMap<String, BTreeSet<String>> = BTreeMap::from([
            (
                "p1".to_owned(),
                BTreeSet::from(["a".to_owned(), "new".to_owned()]),
            ),
            ("p2".to_owned(), BTreeSet::from(["x".to_owned()])),
        ]);
        let included: BTreeSet<String> = ["p1".to_owned(), "p2".to_owned()].into_iter().collect();
        let names: BTreeMap<String, String> =
            BTreeMap::from([("p1".to_owned(), "Provider One".to_owned())]);
        let diff = compute_provider_model_diff(
            &before,
            &after,
            &included,
            &names,
            &[("p3".to_owned(), "timeout".to_owned())],
        );
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].provider_id, "p1");
        assert_eq!(diff.changed[0].provider_name, "Provider One");
        assert_eq!(diff.changed[0].added, 1);
        assert_eq!(diff.changed[0].removed, 1);
        assert_eq!(diff.unchanged, vec!["p2".to_owned()]);
        assert_eq!(diff.failed, vec![("p3".to_owned(), "timeout".to_owned())]);
    }

    #[test]
    fn provider_model_projection_keeps_max_context_and_capabilities() {
        let model = ProviderModel {
            provider: "anthropic".to_owned(),
            id: "claude".to_owned(),
            name: Some("Claude".to_owned()),
            context_window: Some(200_000),
            max_tokens: Some(64_000),
            input: vec!["text".to_owned(), "image".to_owned()],
            reasoning: true,
            thinking_level_map: BTreeMap::from([
                ("off".to_owned(), Some(Value::String("off".to_owned()))),
                ("low".to_owned(), Some(Value::String("low".to_owned()))),
                ("high".to_owned(), Some(Value::String("high".to_owned()))),
            ]),
            default_thinking_level: Some("high".to_owned()),
        };
        let providers = vec![ProviderInfo {
            id: "anthropic".to_owned(),
            name: "Anthropic".to_owned(),
        }];
        let (available_models, available_providers) =
            build_available_maps(std::slice::from_ref(&model), &providers);
        let alias = available_models.get("anthropic/claude").unwrap();
        assert_eq!(alias.provider, "anthropic");
        assert_eq!(alias.model, "claude");
        assert_eq!(alias.display_name.as_deref(), Some("Claude"));
        // `selected.maxContextSize` in `refreshConfigAfterLogin` reads this.
        assert_eq!(alias.max_context_size, Some(200_000));
        assert_eq!(alias.max_output_size, Some(64_000));
        // image in input + reasoning (off is a value → "thinking") + tool_use.
        assert_eq!(alias.capabilities, vec!["image_in", "thinking", "tool_use"]);
        // BTreeMap iteration is sorted; "off" is excluded from efforts.
        assert_eq!(alias.support_efforts, vec!["high", "low"]);
        assert_eq!(alias.default_effort.as_deref(), Some("high"));
        assert!(available_providers.contains_key("anthropic"));
    }

    #[test]
    fn model_alias_key_and_maps() {
        assert_eq!(model_alias_key("anthropic", "claude"), "anthropic/claude");
        let models = vec![ProviderModel {
            provider: "anthropic".to_owned(),
            id: "claude".to_owned(),
            name: Some("Claude".to_owned()),
            context_window: Some(200_000),
            max_tokens: None,
            input: vec!["text".to_owned()],
            reasoning: false,
            thinking_level_map: BTreeMap::new(),
            default_thinking_level: None,
        }];
        let providers = vec![ProviderInfo {
            id: "anthropic".to_owned(),
            name: "Anthropic".to_owned(),
        }];
        let (available_models, available_providers) = build_available_maps(&models, &providers);
        let alias = available_models.get("anthropic/claude").unwrap();
        assert_eq!(alias.model, "claude");
        assert_eq!(alias.max_context_size, Some(200_000));
        assert!(alias.capabilities.contains(&"tool_use".to_owned()));
        assert!(available_providers.contains_key("anthropic"));
    }
}
