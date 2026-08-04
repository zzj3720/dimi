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

/// A provider model entry (`{ provider, id }` — the `auth.models()` rows the
/// TUI reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModel {
    pub provider: String,
    pub id: String,
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

/// Build the model map + provider map from raw lists
/// (`refreshAvailableModels`).
pub fn build_available_maps(
    models: &[ProviderModel],
    providers: &[ProviderInfo],
) -> (BTreeMap<String, String>, BTreeMap<String, Value>) {
    let available_models: BTreeMap<String, String> = models
        .iter()
        .map(|m| (model_alias_key(&m.provider, &m.id), m.id.clone()))
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
        let models = vec![
            ProviderModel {
                provider: "p1".to_owned(),
                id: "a".to_owned(),
            },
            ProviderModel {
                provider: "p1".to_owned(),
                id: "b".to_owned(),
            },
            ProviderModel {
                provider: "p2".to_owned(),
                id: "c".to_owned(),
            },
        ];
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
    fn model_alias_key_and_maps() {
        assert_eq!(model_alias_key("anthropic", "claude"), "anthropic/claude");
        let models = vec![ProviderModel {
            provider: "anthropic".to_owned(),
            id: "claude".to_owned(),
        }];
        let providers = vec![ProviderInfo {
            id: "anthropic".to_owned(),
            name: "Anthropic".to_owned(),
        }];
        let (available_models, available_providers) = build_available_maps(&models, &providers);
        assert_eq!(
            available_models.get("anthropic/claude").map(String::as_str),
            Some("claude")
        );
        assert!(available_providers.contains_key("anthropic"));
    }
}
