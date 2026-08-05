//! Small single-list selectors — thin wrappers over the choice picker.
//!
//! Port of `apps/dimi/src/tui/components/dialogs/{theme,context-size,
//! permission,editor,busy-input-mode,update-preference}-selector.ts`. Each TS
//! class is a `ChoicePickerComponent` subclass that only configures options,
//! so each Rust equivalent is a constructor returning the already-verified
//! [`crate::chrome::ChoicePickerComponent`] with the right options.

use crate::chrome::{ChoiceOption, ChoicePickerComponent, ChoicePickerOptions};

/// `CONTEXT_SIZE_FLOOR_TOKENS` from `@dimi-agent/agent-core-v2`
/// (`agent/loop/contextSize.ts`).
pub const CONTEXT_SIZE_FLOOR_TOKENS: i64 = 200_000;

/// `trimDecimal` — one decimal place, dropping a redundant `.0`.
fn trim_decimal(v: f64) -> String {
    let s = format!("{v:.1}");
    if let Some(stripped) = s.strip_suffix(".0") {
        stripped.to_owned()
    } else {
        s
    }
}

/// `formatDecimalTokenCount` — 1000-based units ("1M", "950k", "200k").
pub fn format_decimal_token_count(n: i64) -> String {
    if n < 0 {
        return "0".to_owned();
    }
    let nf = n as f64;
    if nf >= 1_000_000.0 {
        return format!("{}M", trim_decimal(nf / 1_000_000.0));
    }
    if nf >= 1_000.0 {
        return format!("{}k", trim_decimal(nf / 1_000.0));
    }
    n.to_string()
}

/// Built-in theme options (`THEME_OPTIONS`).
pub const THEME_OPTIONS: [(&str, &str); 3] = [
    ("auto", "Auto (match terminal)"),
    ("dark", "Dark"),
    ("light", "Light"),
];

/// `ThemeSelectorOptions`.
#[derive(Debug, Clone)]
pub struct ThemeSelectorOptions {
    pub current_value: String,
    /// Custom theme names from `listCustomThemesSync()`; rendered as
    /// `Custom: <name>`.
    pub custom_themes: Vec<String>,
}

/// `ThemeSelectorComponent` — `new ThemeSelectorComponent({...})`.
pub fn theme_selector_component(opts: ThemeSelectorOptions) -> ChoicePickerComponent {
    let mut options: Vec<ChoiceOption> = THEME_OPTIONS
        .iter()
        .map(|(value, label)| ChoiceOption {
            value: (*value).to_owned(),
            label: (*label).to_owned(),
            description: None,
            tone: None,
            description_tone: None,
        })
        .collect();
    for name in opts.custom_themes {
        options.push(ChoiceOption {
            value: name.clone(),
            label: format!("Custom: {name}"),
            description: None,
            tone: None,
            description_tone: None,
        });
    }
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: "Select theme".to_owned(),
        options,
        current_value: Some(opts.current_value),
        hint: None,
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

/// `ContextSizeSelectorOptions`.
#[derive(Debug, Clone)]
pub struct ContextSizeSelectorOptions {
    /// The model's default context window in tokens (100% base).
    pub context_window: i64,
    /// Percentage levels offered by the picker.
    pub percent_options: Vec<i64>,
    /// Currently configured percentage (defaults to 100).
    pub current_percent: i64,
}

/// `buildOptions` — one entry per percentage level.
pub fn context_size_options(context_window: i64, percent_options: &[i64]) -> Vec<ChoiceOption> {
    percent_options
        .iter()
        .map(|percent| ChoiceOption {
            value: percent.to_string(),
            // The model's fixed window is always the 100% default; a
            // user-selected percentage only changes the effective size.
            label: if *percent == 100 {
                "100% (default)".to_owned()
            } else {
                format!("{percent}%")
            },
            description: Some(format!(
                "{} tokens",
                format_decimal_token_count((context_window * percent) / 100)
            )),
            tone: None,
            description_tone: None,
        })
        .collect()
}

/// `ContextSizeSelectorComponent`.
pub fn context_size_selector_component(opts: ContextSizeSelectorOptions) -> ChoicePickerComponent {
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: "Context size".to_owned(),
        hint: Some(format!(
            "Model window {} · floor {}",
            format_decimal_token_count(opts.context_window),
            format_decimal_token_count(CONTEXT_SIZE_FLOOR_TOKENS)
        )),
        options: context_size_options(opts.context_window, &opts.percent_options),
        current_value: Some(opts.current_percent.to_string()),
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

/// `PermissionSelectorOptions` (`PermissionMode` = manual | yolo | auto).
#[derive(Debug, Clone)]
pub struct PermissionSelectorOptions {
    pub current_value: String,
}

/// `PermissionSelectorComponent`.
pub fn permission_selector_component(opts: PermissionSelectorOptions) -> ChoicePickerComponent {
    let options = vec![
        ChoiceOption {
            value: "manual".to_owned(),
            label: "Manual".to_owned(),
            description: Some("Approve every action yourself.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "yolo".to_owned(),
            label: "YOLO".to_owned(),
            description: Some(
                "Auto-approve tool actions, but the agent may still ask questions.".to_owned(),
            ),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "auto".to_owned(),
            label: "Auto".to_owned(),
            description: Some(
                "Fully autonomous — agent decides everything without asking.".to_owned(),
            ),
            tone: None,
            description_tone: None,
        },
    ];
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: "Select permission mode".to_owned(),
        options,
        current_value: Some(opts.current_value),
        hint: None,
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

/// `EditorSelectorOptions`.
#[derive(Debug, Clone)]
pub struct EditorSelectorOptions {
    pub current_value: String,
}

/// `EditorSelectorComponent`.
pub fn editor_selector_component(opts: EditorSelectorOptions) -> ChoicePickerComponent {
    let options = [
        ("code --wait", "VS Code (code --wait)"),
        ("vim", "Vim"),
        ("nvim", "Neovim"),
        ("nano", "Nano"),
        ("", "Auto-detect ($VISUAL / $EDITOR)"),
    ]
    .iter()
    .map(|(value, label)| ChoiceOption {
        value: (*value).to_owned(),
        label: (*label).to_owned(),
        description: None,
        tone: None,
        description_tone: None,
    })
    .collect();
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: "Select external editor".to_owned(),
        options,
        current_value: Some(opts.current_value),
        hint: None,
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

/// `BusyInputModeSelectorOptions` (`BusyInputMode` = steer | queue).
#[derive(Debug, Clone)]
pub struct BusyInputModeSelectorOptions {
    pub current_value: String,
}

/// `BusyInputModeSelectorComponent`.
pub fn busy_input_mode_selector_component(
    opts: BusyInputModeSelectorOptions,
) -> ChoicePickerComponent {
    let options = vec![
        ChoiceOption {
            value: "steer".to_owned(),
            label: "Steer (default)".to_owned(),
            description: Some(
                "Enter injects into the current turn immediately (same as Ctrl-S).".to_owned(),
            ),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "queue".to_owned(),
            label: "Queue".to_owned(),
            description: Some(
                "Enter queues for after the current task; Ctrl-S steers immediately.".to_owned(),
            ),
            tone: None,
            description_tone: None,
        },
    ];
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: "Busy input".to_owned(),
        options,
        current_value: Some(opts.current_value),
        hint: None,
        format_hint: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

/// `UpdatePreferenceSelectorOptions` — boolean preference.
#[derive(Debug, Clone)]
pub struct UpdatePreferenceSelectorOptions {
    pub current_value: bool,
}

/// `UpdatePreferenceSelectorComponent`.
pub fn update_preference_selector_component(
    opts: UpdatePreferenceSelectorOptions,
) -> ChoicePickerComponent {
    let options = vec![
        ChoiceOption {
            value: "on".to_owned(),
            label: "On".to_owned(),
            description: Some("Install new versions in the background.".to_owned()),
            tone: None,
            description_tone: None,
        },
        ChoiceOption {
            value: "off".to_owned(),
            label: "Off".to_owned(),
            description: Some("Show the install prompt instead.".to_owned()),
            tone: None,
            description_tone: None,
        },
    ];
    ChoicePickerComponent::new(ChoicePickerOptions {
        title: "Automatic updates".to_owned(),
        options,
        current_value: Some(if opts.current_value { "on" } else { "off" }.to_owned()),
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

    #[test]
    fn decimal_token_count() {
        assert_eq!(format_decimal_token_count(0), "0");
        assert_eq!(format_decimal_token_count(999), "999");
        assert_eq!(format_decimal_token_count(1_000), "1k");
        assert_eq!(format_decimal_token_count(200_000), "200k");
        assert_eq!(format_decimal_token_count(950_000), "950k");
        assert_eq!(format_decimal_token_count(1_000_000), "1M");
        assert_eq!(format_decimal_token_count(1_500_000), "1.5M");
    }

    #[test]
    fn theme_selector_renders_builtin_and_custom() {
        set_palette(DARK_COLORS);
        let mut c = theme_selector_component(ThemeSelectorOptions {
            current_value: "dark".to_owned(),
            custom_themes: vec!["nord".to_owned(), "solar".to_owned()],
        });
        let lines = c.render(80);
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains("Select theme"), "{joined}");
        assert!(joined.contains("Auto (match terminal)"), "{joined}");
        assert!(joined.contains("Custom: nord"), "{joined}");
        assert!(joined.contains("Custom: solar"), "{joined}");
        // current theme marked.
        assert!(joined.contains("← current"), "{joined}");
    }

    #[test]
    fn context_size_options_build() {
        set_palette(DARK_COLORS);
        let opts = context_size_options(1_000_000, &[100, 75, 50]);
        assert_eq!(opts[0].label, "100% (default)");
        assert_eq!(opts[0].description.as_deref(), Some("1M tokens"));
        assert_eq!(opts[1].label, "75%");
        assert_eq!(opts[1].description.as_deref(), Some("750k tokens"));
        assert_eq!(opts[2].label, "50%");
        assert_eq!(opts[2].description.as_deref(), Some("500k tokens"));
    }

    #[test]
    fn permission_selector_renders() {
        set_palette(DARK_COLORS);
        let mut c = permission_selector_component(PermissionSelectorOptions {
            current_value: "yolo".to_owned(),
        });
        let lines = c.render(80);
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains("Select permission mode"), "{joined}");
        assert!(joined.contains("Manual"), "{joined}");
        assert!(joined.contains("YOLO"), "{joined}");
        assert!(joined.contains("Auto-approve"), "{joined}");
    }

    #[test]
    fn editor_selector_renders_auto_detect() {
        set_palette(DARK_COLORS);
        let mut c = editor_selector_component(EditorSelectorOptions {
            current_value: "vim".to_owned(),
        });
        let lines = c.render(80);
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains("Auto-detect"), "{joined}");
    }

    #[test]
    fn update_preference_maps_boolean_to_value() {
        set_palette(DARK_COLORS);
        let mut c = update_preference_selector_component(UpdatePreferenceSelectorOptions {
            current_value: true,
        });
        // current = on.
        let lines = c.render(80);
        assert!(plain(&lines.join("\n")).contains("← current"));
    }
}
