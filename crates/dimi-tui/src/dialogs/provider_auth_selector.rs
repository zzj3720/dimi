//! Provider auth selectors — thin wrappers over the choice picker. Port of
//! `apps/dimi/src/tui/components/dialogs/provider-auth-selector.ts`
//! (`ProviderAuthSelectorComponent` and `AuthTypeSelectorComponent`).

use crate::chrome::{ChoiceOption, ChoicePickerComponent, ChoicePickerOptions};

/// `AuthType` from `@dimi-agent/dimi-sdk`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    OAuth,
    ApiKey,
}

impl AuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthType::OAuth => "oauth",
            AuthType::ApiKey => "apikey",
        }
    }
}

/// `ProviderAuthMethod`.
#[derive(Debug, Clone)]
pub struct ProviderAuthMethod {
    pub auth_type: AuthType,
    pub label: String,
}

/// `ProviderAuthState`.
#[derive(Debug, Clone)]
pub struct ProviderAuthState {
    pub id: String,
    pub name: String,
    pub configured: bool,
    /// `credentialType` — `'oauth' | 'apikey'`.
    pub credential_type: AuthType,
    pub source: Option<String>,
    pub methods: Vec<ProviderAuthMethod>,
}

/// `ProviderAuthSelectorOptions`.
#[derive(Debug, Clone)]
pub struct ProviderAuthSelectorOptions {
    pub title: String,
    pub providers: Vec<ProviderAuthState>,
    pub auth_type: Option<AuthType>,
}

/// `providerOption` — one provider row filtered by `authType`.
fn provider_option(provider: &ProviderAuthState, auth_type: Option<AuthType>) -> ChoiceOption {
    let methods: Vec<&ProviderAuthMethod> = provider
        .methods
        .iter()
        .filter(|m| auth_type.is_none() || Some(m.auth_type) == auth_type)
        .collect();
    let method_label = methods
        .iter()
        .map(|m| m.label.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    let status = if provider.configured {
        let cred = if provider.credential_type == AuthType::OAuth {
            "OAuth"
        } else {
            "API key"
        };
        let source = provider
            .source
            .as_deref()
            .map(|s| format!(" ({s})"))
            .unwrap_or_default();
        format!("connected via {cred}{source}")
    } else {
        "not connected".to_owned()
    };
    ChoiceOption {
        value: provider.id.clone(),
        label: provider.name.clone(),
        description: Some(
            [provider.id.clone(), method_label, status]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · "),
        ),
        tone: None,
        description_tone: None,
    }
}

/// `ProviderAuthSelectorComponent`.
pub fn provider_auth_selector_component(
    opts: ProviderAuthSelectorOptions,
) -> ChoicePickerComponent {
    let options: Vec<ChoiceOption> = opts
        .providers
        .iter()
        .filter(|p| {
            opts.auth_type.is_none()
                || p.methods
                    .iter()
                    .any(|m| Some(m.auth_type) == opts.auth_type)
        })
        .map(|p| provider_option(p, opts.auth_type))
        .collect();
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: opts.title,
        options,
        current_value: None,
        hint: None,
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: true,
        page_size: None,
        has_session_only: false,
    })
}

/// `uniqueMethods` — dedupe by `AuthType`, preserving order.
fn unique_methods(methods: &[ProviderAuthMethod]) -> Vec<ProviderAuthMethod> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for method in methods {
        if seen.contains(&method.auth_type) {
            continue;
        }
        seen.push(method.auth_type);
        out.push(method.clone());
    }
    out
}

/// `AuthTypeSelectorOptions`.
#[derive(Debug, Clone)]
pub struct AuthTypeSelectorOptions {
    pub provider_name: Option<String>,
    pub methods: Vec<ProviderAuthMethod>,
}

/// `AuthTypeSelectorComponent`.
pub fn auth_type_selector_component(opts: AuthTypeSelectorOptions) -> ChoicePickerComponent {
    let methods = unique_methods(&opts.methods);
    let options: Vec<ChoiceOption> = methods
        .iter()
        .map(|method| ChoiceOption {
            value: method.auth_type.as_str().to_owned(),
            label: if opts.provider_name.is_none() {
                match method.auth_type {
                    AuthType::OAuth => "Sign in with an account".to_owned(),
                    AuthType::ApiKey => "Sign in with an API key".to_owned(),
                }
            } else {
                method.label.clone()
            },
            description: Some(
                if method.auth_type == AuthType::OAuth {
                    "OAuth subscription"
                } else {
                    "Stored API key"
                }
                .to_owned(),
            ),
            tone: None,
            description_tone: None,
        })
        .collect();
    let title = match &opts.provider_name {
        None => "Select authentication method".to_owned(),
        Some(name) => format!("Select authentication method for {name}"),
    };
    ChoicePickerComponent::new(ChoicePickerOptions {
        title,
        options,
        current_value: None,
        hint: None,
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::theme::{DARK_COLORS, set_palette};

    fn plain(joined: &str) -> String {
        crate::ansi::strip_ansi(joined)
    }

    fn provider(id: &str, name: &str, configured: bool, methods: &[&str]) -> ProviderAuthState {
        ProviderAuthState {
            id: id.to_owned(),
            name: name.to_owned(),
            configured,
            credential_type: AuthType::OAuth,
            source: None,
            methods: methods
                .iter()
                .map(|m| ProviderAuthMethod {
                    auth_type: if *m == "oauth" {
                        AuthType::OAuth
                    } else {
                        AuthType::ApiKey
                    },
                    label: m.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn provider_auth_selector_filters_by_type() {
        set_palette(DARK_COLORS);
        let providers = vec![
            provider("p1", "Anthropic", false, &["oauth", "apikey"]),
            provider("p2", "OpenAI", true, &["apikey"]),
        ];
        let mut c = provider_auth_selector_component(ProviderAuthSelectorOptions {
            title: "Connect a provider".to_owned(),
            providers,
            auth_type: Some(AuthType::OAuth),
        });
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("Anthropic"), "{joined}");
        assert!(!joined.contains("OpenAI"), "{joined}"); // api-key only, filtered out
    }

    #[test]
    fn auth_type_selector_labels() {
        set_palette(DARK_COLORS);
        let mut c = auth_type_selector_component(AuthTypeSelectorOptions {
            provider_name: Some("Anthropic".to_owned()),
            methods: vec![
                ProviderAuthMethod {
                    auth_type: AuthType::OAuth,
                    label: "OAuth".to_owned(),
                },
                ProviderAuthMethod {
                    auth_type: AuthType::OAuth,
                    label: "OAuth (dup)".to_owned(),
                },
            ],
        });
        let joined = plain(&c.render(80).join("\n"));
        assert!(
            joined.contains("Select authentication method for Anthropic"),
            "{joined}"
        );
        // dedup: only one OAuth row.
        assert_eq!(joined.matches("OAuth subscription").count(), 1);
    }
}
