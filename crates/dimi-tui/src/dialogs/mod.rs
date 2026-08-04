//! Dialog components — port of `apps/dimi/src/tui/components/dialogs/`
//! (slice 9: the dialog core + approval/question panels).
//!
//! Slice scope (see the rustify plan §5 and the parent task):
//!   1. model-selector, session-picker, effort-selector, undo-selector,
//!      experiments-selector
//!   2. theme/context-size/permission/editor/busy-input-mode/update-preference
//!      selectors (thin wrappers over the choice picker)
//!   3. approval-panel + approval-preview + question-dialog (the reverse-rpc
//!      UI side; the reverse-rpc callback wiring is left to a later slice)
//!   4. help-panel
//!
//! Every component implements [`crate::component::Component`]; keyboard
//! navigation state is maintained in `handle_input` and real callbacks
//! (`onSelect` / `onCancel` / …) are surfaced via a `take_action()`-style
//! enum, mirroring `chrome::ChoicePickerComponent::take_action`. The host
//! polls that after each input event.
//!
//! Golden fixtures: `testdata/dialogs-golden.jsonl` captured from the real TS
//! components (see `.tmp/capture-dialogs.vitest.ts`); the byte-exact Rust test
//! lives in the `tests` module at the bottom of this file.

pub mod approval;
pub mod custom_provider_dialog;
pub mod effort_selector;
pub mod experiments_selector;
pub mod feedback_input_dialog;
pub mod help;
pub mod input_line;
pub mod model_common;
pub mod model_selector;
pub mod plugin_types;
pub mod plugins_selector;
pub mod provider_auth_selector;
pub mod provider_login_dialog;
pub mod question;
pub mod selectors;
pub mod session_picker;
pub mod settings_selector;
pub mod start_permission_prompt;
pub mod swarm_start_permission_prompt;
pub mod tabbed_model_selector;
pub mod task_output_viewer;
pub mod task_types;
pub mod undo_selector;

use crate::width::visible_width;
use crate::wrap::{truncate_to_width, wrap_text_with_ansi};

/// `❯` — selected pointer (`SELECT_POINTER`).
pub(crate) const SELECT_POINTER: &str = "❯";
/// `← current` — currently-active marker (`CURRENT_MARK`).
pub(crate) const CURRENT_MARK: &str = "← current";

/// Normalize whitespace runs to single spaces and trim (port of the TS
/// `singleLine` helper used by session-picker).
pub(crate) fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Word-wrap `text` to `width` columns (port of the `wrapText` helper shared
/// by choice-picker / experiments-selector). Returns wrapped lines, each ≤
/// `width` visible columns; long words are ellipsis-truncated.
pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let max_width = width.max(1);
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in words {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if visible_width(&candidate) <= max_width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(current);
        }
        current = if visible_width(word) <= max_width {
            word.to_owned()
        } else {
            truncate_to_width(word, max_width, "…", false)
        };
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Push `content` to `lines`, wrapping it to fit `width` with a hanging
/// indent (port of the `appendWrapped` helper in question-dialog.ts /
/// `appendWrappedLine` in approval-panel.ts).
///
/// The first physical line starts with `first_prefix`; continuation lines get
/// `continuation_prefix`. When `tone` is provided every emitted line is
/// wrapped in a single ANSI span (cleaner for selection highlights); pass
/// `None` when the prefixes already carry their own mixed styling.
pub(crate) fn append_wrapped(
    lines: &mut Vec<String>,
    first_prefix: &str,
    continuation_prefix: &str,
    content: &str,
    width: usize,
    tone: Option<&dyn Fn(&str) -> String>,
) {
    let prefix_width = visible_width(first_prefix).max(visible_width(continuation_prefix));
    let content_width = (width.saturating_sub(prefix_width)).max(1);
    let wrapped = wrap_text_with_ansi(content, content_width);
    let style = |s: String| -> String {
        match tone {
            Some(t) => t(&s),
            None => s,
        }
    };
    if wrapped.is_empty() {
        lines.push(style(first_prefix.to_owned()));
        return;
    }
    lines.push(style(format!("{first_prefix}{}", wrapped[0])));
    for line in wrapped.iter().skip(1) {
        lines.push(style(format!("{continuation_prefix}{line}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_collapses_whitespace() {
        assert_eq!(single_line("  a\t b\n\n c  "), "a b c");
        assert_eq!(single_line(""), "");
    }

    #[test]
    fn wrap_text_keeps_lines_under_width() {
        let lines = wrap_text(
            "The quick brown fox jumps over the lazy dog and keeps running",
            20,
        );
        // "The quick brown fox"=19, "jumps over the lazy"=19, "dog and
        // keeps"=13, "running"=7 — each ≤ 20.
        assert_eq!(
            lines,
            vec![
                "The quick brown fox",
                "jumps over the lazy",
                "dog and keeps",
                "running",
            ]
        );
        for line in &lines {
            assert!(visible_width(line) <= 20, "line too wide: {line}");
        }
    }

    #[test]
    fn append_wrapped_hanging_indent() {
        let mut lines = Vec::new();
        append_wrapped(
            &mut lines,
            " ? ",
            "   ",
            "The quick brown fox jumps over the lazy dog",
            24,
            None,
        );
        // Content wrapped to width - 3 = 21.
        assert_eq!(lines[0], " ? The quick brown fox");
        assert_eq!(lines[1], "   jumps over the lazy");
        assert_eq!(lines[2], "   dog");
    }
}

// ===========================================================================
// Golden tests (byte-exact against TS-captured `testdata/dialogs-golden.jsonl`)
// ===========================================================================

#[cfg(test)]
mod golden_tests {
    use crate::component::Component;
    use crate::dialogs::approval::{
        ApprovalPanelComponent, ApprovalPanelData, ApprovalPreviewBlock, ApprovalPreviewViewer,
        DisplayBlock, PendingApproval,
    };
    use crate::dialogs::effort_selector::{EffortSelectorComponent, EffortSelectorOptions};
    use crate::dialogs::experiments_selector::{
        ExperimentalFeatureState, ExperimentsSelectorComponent, ExperimentsSelectorOptions,
        FeatureSource,
    };
    use crate::dialogs::help::{HelpPanelCommand, HelpPanelComponent, HelpPanelOptions};
    use crate::dialogs::model_common::ModelAlias;
    use crate::dialogs::model_selector::{ModelSelectorComponent, ModelSelectorOptions};
    use crate::dialogs::question::{
        PendingQuestion, QuestionDialogComponent, QuestionOption, QuestionPanelData,
        QuestionPanelItem,
    };
    use crate::dialogs::selectors::{
        ContextSizeSelectorOptions, PermissionSelectorOptions, ThemeSelectorOptions,
        context_size_selector_component, permission_selector_component, theme_selector_component,
    };
    use crate::dialogs::session_picker::{
        SessionPickerComponent, SessionPickerOptions, SessionRow,
    };
    use crate::dialogs::undo_selector::{UndoChoice, UndoSelectorComponent, UndoSelectorOptions};
    use crate::theme::{DARK_COLORS, set_palette};
    use serde::Deserialize;
    use std::fs;

    #[derive(Deserialize)]
    struct Fixture {
        name: String,
        #[serde(default)]
        width: usize,
        #[serde(default)]
        lines: Vec<String>,
        #[serde(default)]
        now: Option<i64>,
        #[serde(default)]
        rows: Option<usize>,
    }

    fn golden_path() -> String {
        format!(
            "{}/testdata/dialogs-golden.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    fn model_alias(
        model: &str,
        display_name: Option<&str>,
        provider: &str,
        capabilities: &[&str],
        support_efforts: &[&str],
        default_effort: Option<&str>,
    ) -> ModelAlias {
        ModelAlias {
            model: model.to_owned(),
            display_name: display_name.map(str::to_owned),
            provider: provider.to_owned(),
            capabilities: capabilities.iter().map(|s| (*s).to_owned()).collect(),
            support_efforts: support_efforts.iter().map(|s| (*s).to_owned()).collect(),
            default_effort: default_effort.map(str::to_owned),
        }
    }

    /// Mirrors the TS capture setup exactly (same data, same construction
    /// order). `now` / `rows` come from the fixture for deterministic output.
    fn build(name: &str, now: Option<i64>, rows: Option<usize>) -> Box<dyn Component> {
        match name {
            "model_selector_basic" | "model_selector_warning" => {
                let models = vec![
                    (
                        "sonnet".to_owned(),
                        model_alias(
                            "claude-sonnet",
                            Some("Sonnet"),
                            "anthropic",
                            &["thinking"],
                            &["low", "high"],
                            Some("high"),
                        ),
                    ),
                    (
                        "kimi".to_owned(),
                        model_alias(
                            "kimi-k2",
                            Some("Kimi K2"),
                            "kimi-coding",
                            &["always_thinking"],
                            &["low", "medium", "high"],
                            None,
                        ),
                    ),
                    (
                        "legacy".to_owned(),
                        model_alias("old-model", None, "managed:self-hosted", &[], &[], None),
                    ),
                ];
                let warning = if name.ends_with("warning") {
                    Some(
                        "Switching models mid-conversation resets the current turn context."
                            .to_owned(),
                    )
                } else {
                    None
                };
                let has_session_only = name.ends_with("basic");
                Box::new(ModelSelectorComponent::new(ModelSelectorOptions {
                    models,
                    current_value: "sonnet".to_owned(),
                    selected_value: None,
                    current_thinking_effort: "high".to_owned(),
                    title: None,
                    searchable: true,
                    page_size: None,
                    provider_switch_hint: false,
                    warning,
                    has_session_only,
                }))
            }
            "session_picker_list" => {
                let now_ms = now.unwrap_or(0);
                Box::new(SessionPickerComponent::new(SessionPickerOptions {
                    sessions: vec![
                        SessionRow {
                            id: "session_aaa".to_owned(),
                            title: Some("Add auth".to_owned()),
                            last_prompt: Some("Add github auth to the project".to_owned()),
                            work_dir: "/home/user/proj".to_owned(),
                            updated_at: now_ms - 5_000,
                        },
                        SessionRow {
                            id: "session_bbb".to_owned(),
                            title: Some("Fix TUI".to_owned()),
                            last_prompt: None,
                            work_dir: "/home/user/other".to_owned(),
                            updated_at: now_ms - 2 * 3_600_000,
                        },
                        SessionRow {
                            id: "session_ccc".to_owned(),
                            title: None,
                            last_prompt: None,
                            work_dir: "/home/user/proj/deep".to_owned(),
                            updated_at: now_ms - 3 * 86_400_000,
                        },
                    ],
                    loading: false,
                    current_session_id: "session_bbb".to_owned(),
                    scope: crate::dialogs::session_picker::SessionScope::Cwd,
                    initial_selected_session_id: None,
                    page_size: None,
                    max_visible_sessions: None,
                    has_toggle_scope: true,
                    // The capture ran under the real HOME, so '/home/user/…'
                    // was not `~`-aliased — pass an empty home to match.
                    home: Some(String::new()),
                    now_ms: Some(now_ms),
                }))
            }
            "session_picker_loading" => {
                Box::new(SessionPickerComponent::new(SessionPickerOptions {
                    sessions: Vec::new(),
                    loading: true,
                    current_session_id: "session_aaa".to_owned(),
                    scope: crate::dialogs::session_picker::SessionScope::Cwd,
                    initial_selected_session_id: None,
                    page_size: None,
                    max_visible_sessions: None,
                    has_toggle_scope: false,
                    home: Some(String::new()),
                    now_ms: now,
                }))
            }
            "session_picker_empty" => Box::new(SessionPickerComponent::new(SessionPickerOptions {
                sessions: Vec::new(),
                loading: false,
                current_session_id: "session_aaa".to_owned(),
                scope: crate::dialogs::session_picker::SessionScope::Cwd,
                initial_selected_session_id: None,
                page_size: None,
                max_visible_sessions: None,
                has_toggle_scope: false,
                home: Some(String::new()),
                now_ms: now,
            })),
            "effort_selector_basic" | "effort_selector_warning" => {
                let warning = if name.ends_with("warning") {
                    Some(
                        "Switching effort mid-conversation resets the current turn context."
                            .to_owned(),
                    )
                } else {
                    None
                };
                let efforts = if name.ends_with("warning") {
                    vec!["off", "low", "high"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                } else {
                    vec!["off", "low", "high", "max"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                };
                let current = if name.ends_with("warning") {
                    "low"
                } else {
                    "high"
                };
                Box::new(EffortSelectorComponent::new(EffortSelectorOptions {
                    title: None,
                    efforts,
                    current_value: current.to_owned(),
                    has_session_only: name.ends_with("basic"),
                    warning,
                }))
            }
            "undo_selector_basic" => {
                let choices = vec![
                    UndoChoice {
                        id: "msg_1".to_owned(),
                        count: 1,
                        input: "first message".to_owned(),
                        label: "1. first message".to_owned(),
                    },
                    UndoChoice {
                        id: "msg_2".to_owned(),
                        count: 2,
                        input: "second message".to_owned(),
                        label: "2. second message".to_owned(),
                    },
                    UndoChoice {
                        id: "msg_3".to_owned(),
                        count: 3,
                        input: "third message".to_owned(),
                        label: "3. third message".to_owned(),
                    },
                ];
                Box::new(UndoSelectorComponent::new(UndoSelectorOptions { choices }))
            }
            "experiments_selector_basic" => Box::new(ExperimentsSelectorComponent::new(
                ExperimentsSelectorOptions {
                    features: vec![
                        ExperimentalFeatureState {
                            id: "steer".to_owned(),
                            title: "Steer into turn".to_owned(),
                            description: "Inject follow-ups into the current turn.".to_owned(),
                            env: "DIMI_EXP_STEER".to_owned(),
                            source: FeatureSource::Config,
                            enabled: false,
                        },
                        ExperimentalFeatureState {
                            id: "swarm".to_owned(),
                            title: "Swarm mode".to_owned(),
                            description: "Run multiple agents in parallel.".to_owned(),
                            env: "DIMI_EXP_SWARM".to_owned(),
                            source: FeatureSource::Default,
                            enabled: true,
                        },
                        ExperimentalFeatureState {
                            id: "locked".to_owned(),
                            title: "Locked flag".to_owned(),
                            description: "Controlled by an env var.".to_owned(),
                            env: "DIMI_EXP_LOCKED".to_owned(),
                            source: FeatureSource::Env,
                            enabled: false,
                        },
                    ],
                },
            )),
            "approval_panel_shell" => Box::new(ApprovalPanelComponent::new(
                PendingApproval {
                    data: ApprovalPanelData {
                        id: "approval_shell".to_owned(),
                        tool_call_id: "tool_1".to_owned(),
                        tool_name: "Bash".to_owned(),
                        action: "run".to_owned(),
                        description: String::new(),
                        display: vec![DisplayBlock::Shell {
                            language: "bash".to_owned(),
                            command: "rm -rf /tmp/cache && echo done".to_owned(),
                            cwd: Some("/home/user/proj".to_owned()),
                            description: None,
                            danger: Some("recursive delete".to_owned()),
                        }],
                        choices: vec![
                            crate::dialogs::approval::ApprovalChoice {
                                label: "Approve once".to_owned(),
                                response: "approved".to_owned(),
                                selected_label: None,
                                requires_feedback: false,
                                description: None,
                            },
                            crate::dialogs::approval::ApprovalChoice {
                                label: "Reject with feedback".to_owned(),
                                response: "rejected".to_owned(),
                                selected_label: None,
                                requires_feedback: true,
                                description: None,
                            },
                        ],
                    },
                },
                true,
                true,
            )),
            "approval_panel_diff" => Box::new(ApprovalPanelComponent::new(
                PendingApproval {
                    data: ApprovalPanelData {
                        id: "approval_diff".to_owned(),
                        tool_call_id: "tool_2".to_owned(),
                        tool_name: "Edit".to_owned(),
                        action: "apply edit".to_owned(),
                        description: String::new(),
                        display: vec![DisplayBlock::Diff {
                            path: "src/foo.ts".to_owned(),
                            old_text: "alpha\nbeta\ngamma".to_owned(),
                            new_text: "alpha\nBETA\ngamma".to_owned(),
                            old_start: None,
                            new_start: None,
                            is_summary: None,
                        }],
                        choices: vec![
                            crate::dialogs::approval::ApprovalChoice {
                                label: "Approve once".to_owned(),
                                response: "approved".to_owned(),
                                selected_label: Some("approve".to_owned()),
                                requires_feedback: false,
                                description: None,
                            },
                            crate::dialogs::approval::ApprovalChoice {
                                label: "Reject".to_owned(),
                                response: "rejected".to_owned(),
                                selected_label: None,
                                requires_feedback: false,
                                description: None,
                            },
                        ],
                    },
                },
                true,
                true,
            )),
            "approval_preview_diff" => Box::new(ApprovalPreviewViewer::new(
                ApprovalPreviewBlock::Diff {
                    path: "src/foo.ts".to_owned(),
                    old_text: "alpha\nbeta\ngamma\ndelta\nepsilon".to_owned(),
                    new_text: "alpha\nBETA\ngamma\ndelta\nzeta".to_owned(),
                    old_start: None,
                    new_start: None,
                },
                rows.unwrap_or(24),
            )),
            "question_dialog_single" | "question_dialog_submit" => {
                let request = PendingQuestion {
                    data: QuestionPanelData {
                        id: "question_1".to_owned(),
                        tool_call_id: "tool_3".to_owned(),
                        questions: vec![QuestionPanelItem {
                            question: "Pick an option".to_owned(),
                            header: Some("Q1".to_owned()),
                            body: Some("Please choose one of the following.".to_owned()),
                            multi_select: false,
                            other_label: None,
                            other_description: None,
                            options: vec![
                                QuestionOption {
                                    label: "Alpha".to_owned(),
                                    description: Some("first choice".to_owned()),
                                },
                                QuestionOption {
                                    label: "Beta".to_owned(),
                                    description: None,
                                },
                                QuestionOption {
                                    label: "Gamma".to_owned(),
                                    description: None,
                                },
                            ],
                        }],
                    },
                };
                let mut c = QuestionDialogComponent::new(request, Some(6), true);
                if name.ends_with("submit") {
                    c.handle_input("\r"); // answer Alpha → advance to submit tab
                }
                Box::new(c)
            }
            "theme_selector_basic" => Box::new(theme_selector_component(ThemeSelectorOptions {
                current_value: "dark".to_owned(),
                custom_themes: Vec::new(),
            })),
            "context_size_selector_basic" => Box::new(context_size_selector_component(
                ContextSizeSelectorOptions {
                    context_window: 1_000_000,
                    percent_options: vec![100, 75, 50],
                    current_percent: 100,
                },
            )),
            "permission_selector_basic" => {
                Box::new(permission_selector_component(PermissionSelectorOptions {
                    current_value: "yolo".to_owned(),
                }))
            }
            "help_panel_basic" => Box::new(HelpPanelComponent::new(HelpPanelOptions {
                commands: vec![
                    HelpPanelCommand {
                        name: "model".to_owned(),
                        aliases: vec!["m".to_owned()],
                        description: "Select a model".to_owned(),
                    },
                    HelpPanelCommand {
                        name: "help".to_owned(),
                        aliases: Vec::new(),
                        description: "Show this panel".to_owned(),
                    },
                    HelpPanelCommand {
                        name: "skill:agent".to_owned(),
                        aliases: Vec::new(),
                        description: "Run a skill".to_owned(),
                    },
                ],
                shortcuts: None,
                max_visible: None,
            })),
            other => panic!("unknown fixture {other}"),
        }
    }

    #[test]
    fn dialogs_golden_byte_exact() {
        set_palette(DARK_COLORS);
        let data = fs::read_to_string(golden_path()).expect("golden file");
        let mut passed = 0usize;
        for line in data.lines() {
            let fixture: Fixture = serde_json::from_str(line).expect("fixture json");
            let mut component = build(&fixture.name, fixture.now, fixture.rows);
            let rendered = component.render(fixture.width);
            assert_eq!(
                rendered, fixture.lines,
                "fixture {} (width {})",
                fixture.name, fixture.width
            );
            passed += 1;
        }
        eprintln!("dialogs golden passed: {passed} fixtures");
    }

    #[test]
    fn golden_fixture_has_required_coverage() {
        let data = fs::read_to_string(golden_path()).expect("golden file");
        let names: Vec<String> = data
            .lines()
            .filter_map(|l| serde_json::from_str::<Fixture>(l).ok())
            .map(|f| f.name)
            .collect();
        for required in [
            "model_selector_basic",
            "session_picker_list",
            "effort_selector_basic",
            "undo_selector_basic",
            "approval_preview_diff",
            "question_dialog_single",
            "help_panel_basic",
            "theme_selector_basic",
        ] {
            assert!(names.iter().any(|n| n == required), "missing {required}");
        }
    }
}
