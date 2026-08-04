//! Settings selector — thin wrapper over the choice picker. Port of
//! `apps/dimi/src/tui/components/dialogs/settings-selector.ts`
//! (`SettingsSelectorComponent`).

use crate::chrome::{ChoiceOption, ChoicePickerComponent, ChoicePickerOptions};

/// A settings destination (`SettingsSelection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSelection {
    Model,
    Permission,
    Theme,
    Editor,
    BusyInput,
    ContextSize,
    Experiments,
    Upgrade,
    Usage,
}

impl SettingsSelection {
    pub fn value(&self) -> &'static str {
        match self {
            SettingsSelection::Model => "model",
            SettingsSelection::Permission => "permission",
            SettingsSelection::Theme => "theme",
            SettingsSelection::Editor => "editor",
            SettingsSelection::BusyInput => "busy-input",
            SettingsSelection::ContextSize => "context-size",
            SettingsSelection::Experiments => "experiments",
            SettingsSelection::Upgrade => "upgrade",
            SettingsSelection::Usage => "usage",
        }
    }
}

/// `SETTINGS_OPTIONS` from settings-selector.ts.
pub fn settings_options() -> Vec<ChoiceOption> {
    vec![
        ChoiceOption {
            value: "model".to_owned(),
            label: "Model".to_owned(),
            description: Some("Switch the active model and thinking mode.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "permission".to_owned(),
            label: "Permission".to_owned(),
            description: Some("Choose how tool actions are approved.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "theme".to_owned(),
            label: "Theme".to_owned(),
            description: Some("Change the terminal UI theme.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "editor".to_owned(),
            label: "Editor".to_owned(),
            description: Some("Set the external editor command.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "busy-input".to_owned(),
            label: "Busy input".to_owned(),
            description: Some(
                "Choose whether Enter queues or steers while the agent is working.".to_owned(),
            ),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "context-size".to_owned(),
            label: "Context size".to_owned(),
            description: Some(
                "Cap the conversation window as a percentage of the model default (min 200k)."
                    .to_owned(),
            ),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "experiments".to_owned(),
            label: "Experiments".to_owned(),
            description: Some("Turn experimental features on or off.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "upgrade".to_owned(),
            label: "Automatic updates".to_owned(),
            description: Some("Turn automatic CLI updates on or off.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "usage".to_owned(),
            label: "Usage".to_owned(),
            description: Some("Show session tokens, context window, and plan quotas.".to_owned()),
            tone: None,
            description_tone: None,
        },
    ]
}

/// `SettingsSelectorComponent`.
pub fn settings_selector_component() -> ChoicePickerComponent {
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: "Settings".to_owned(),
        options: settings_options(),
        current_value: None,
        hint: None,
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
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

    #[test]
    fn settings_options_are_complete() {
        let opts = settings_options();
        assert_eq!(opts.len(), 9);
        assert_eq!(opts[0].value, "model");
        assert_eq!(opts[0].label, "Model");
        assert_eq!(opts[8].value, "usage");
    }

    #[test]
    fn settings_selector_renders() {
        set_palette(DARK_COLORS);
        let mut c = settings_selector_component();
        let lines = c.render(80);
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains("Settings"), "{joined}");
        assert!(joined.contains("Switch the active model"), "{joined}");
        assert!(joined.contains("Automatic updates"), "{joined}");
    }
}
