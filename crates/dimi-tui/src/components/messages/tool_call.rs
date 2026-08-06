//! `ToolCallComponent` — a tool call block in the transcript
//! (port of `apps/dimi/src/tui/components/messages/tool-call.ts`, slice 2
//! scope: header + call preview + result body; subagent / WaitFor /
//! streaming-args / plan-file features land in later slices).

use crate::component::Component;
use crate::components::messages::plan_box::PlanBoxComponent;
use crate::components::messages::shell_execution::{
    ShellExecutionComponent, ShellExecutionOptions,
};
use crate::components::messages::tool_renderers::{
    ToolCallData, ToolResultData, pick_chip, render_tool_result_body,
};
use crate::components::messages::{COMMAND_PREVIEW_LINES, STATUS_BULLET};
use crate::components::spacer::Spacer;
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};

const MAX_ARG_LENGTH: usize = 60;

/// `extractKeyArgument` — the key argument shown in the header.
fn extract_key_argument(
    tool_name: &str,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let key_map: &[&str] = match tool_name {
        "Bash" => &["command"],
        "Read" => &["path", "file_path"],
        "Write" => &["path", "file_path"],
        "Edit" => &["path", "file_path"],
        "Grep" => &["pattern"],
        "Glob" => &["pattern"],
        "FetchURL" => &["url"],
        "WebSearch" => &["query"],
        "Agent" => &["description", "prompt"],
        _ => &[],
    };

    if tool_name == "Glob" {
        let pattern = args.get("pattern").and_then(|v| v.as_str());
        if let Some(pattern) = pattern {
            if pattern.is_empty() {
                return None;
            }
            let mut summary = pattern.to_owned();
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                if !path.is_empty() {
                    summary.push_str(&format!(" · {path}"));
                }
            }
            if args.get("include_ignored").and_then(|v| v.as_bool()) == Some(true) {
                summary.push_str(" · include ignored");
            }
            return Some(truncate_arg_value("pattern", &summary));
        }
        return None;
    }

    let candidates: Vec<String> = if key_map.is_empty() {
        args.keys().cloned().collect()
    } else {
        key_map.iter().map(|s| s.to_string()).collect()
    };
    for key in candidates {
        let val = args.get(&key).and_then(|v| v.as_str());
        if let Some(val) = val {
            if val.is_empty() {
                continue;
            }
            let first_line = val.split('\n').next().unwrap_or(val);
            let display_value = if tool_name == "Bash" && val.contains('\n') {
                format!("{first_line}…")
            } else {
                first_line.to_owned()
            };
            return Some(format_key_argument(tool_name, &key, &display_value));
        }
    }
    None
}

fn truncate_arg_value(key: &str, value: &str) -> String {
    if value.chars().count() <= MAX_ARG_LENGTH {
        return value.to_owned();
    }
    if key == "path" || key == "file_path" {
        // Preserve the tail (filename).
        let mut s = String::from("…");
        let chars: Vec<char> = value.chars().collect();
        s.extend(chars[chars.len() - (MAX_ARG_LENGTH - 1)..].iter());
        s
    } else {
        let chars: Vec<char> = value.chars().collect();
        let mut s: String = chars[..MAX_ARG_LENGTH - 3].iter().collect();
        s.push_str("...");
        s
    }
}

fn format_key_argument(tool_name: &str, key: &str, value: &str) -> String {
    truncate_arg_value(
        if tool_name == "Read" && (key == "path" || key == "file_path") {
            "file_path"
        } else {
            key
        },
        value,
    )
}

/// `interpretExitPlanModeOutcome` — the slice 2 subset needed for the header
/// chip and plan box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitPlanModeOutcome {
    Approved { chosen: Option<String> },
    AutoApproved,
    Rejected { feedback: Option<String> },
}

pub fn interpret_exit_plan_mode_outcome(output: &str) -> ExitPlanModeOutcome {
    const REJECT_PREFIX: &str = "User rejected the plan.";
    const REJECT_FEEDBACK_PREFIX: &str = "User rejected the plan. Feedback:";
    const PLAN_REJECT_PREFIX: &str = "Plan rejected by user.";
    const AUTO_APPROVED_PLAN_MARKER: &str = "## Plan (auto-approved, not user-reviewed):";

    if output.starts_with(REJECT_PREFIX) {
        if let Some(rest) = output.strip_prefix(REJECT_FEEDBACK_PREFIX) {
            let feedback = rest.trim_start().to_owned();
            return ExitPlanModeOutcome::Rejected {
                feedback: Some(feedback),
            };
        }
        return ExitPlanModeOutcome::Rejected { feedback: None };
    }
    if output.starts_with(PLAN_REJECT_PREFIX) {
        return ExitPlanModeOutcome::Rejected { feedback: None };
    }
    if output.contains(AUTO_APPROVED_PLAN_MARKER) {
        return ExitPlanModeOutcome::AutoApproved;
    }
    const SELECTED_APPROACH_RE: &str = "Exited plan mode. Selected approach: ";
    if let Some(idx) = output.find(SELECTED_APPROACH_RE) {
        let rest = &output[idx + SELECTED_APPROACH_RE.len()..];
        let chosen = rest.split('\n').next().unwrap_or("").to_owned();
        return ExitPlanModeOutcome::Approved {
            chosen: Some(chosen),
        };
    }
    const APPROVED_OPTION_RE_PREFIX: &str = "User approved option \"";
    if let Some(rest) = output.strip_prefix(APPROVED_OPTION_RE_PREFIX) {
        if let Some(end) = rest.find('"') {
            let chosen = rest[..end].to_owned();
            return ExitPlanModeOutcome::Approved {
                chosen: Some(chosen),
            };
        }
    }
    ExitPlanModeOutcome::Approved { chosen: None }
}

/// The tool call block component.
pub struct ToolCallComponent {
    tool_call: ToolCallData,
    result: Option<ToolResultData>,
    expanded: bool,
    children: Vec<Box<dyn Component>>,
    render_cache: Option<(usize, Vec<String>)>,
}

impl ToolCallComponent {
    pub fn new(tool_call: ToolCallData, result: Option<ToolResultData>) -> Self {
        let mut component = ToolCallComponent {
            tool_call,
            result,
            expanded: false,
            children: Vec::new(),
            render_cache: None,
        };
        component.rebuild_body();
        component
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.rebuild_body();
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    fn rebuild_body(&mut self) {
        self.children.clear();
        self.children.push(Box::new(Spacer::new(1)));
        self.children
            .push(Box::new(Text::new(&self.build_header(), 0, 0)));
        self.build_call_preview();
        self.build_content();
        self.render_cache = None;
    }

    fn build_header(&self) -> String {
        let result = &self.result;
        let is_finished = result.is_some();
        let is_error = result.as_ref().is_some_and(|r| r.is_error);
        let is_truncated = self.tool_call.truncated && !is_finished;

        let bullet = if is_finished {
            if is_error {
                current_theme().fg(ColorToken::Error, "✗ ")
            } else {
                current_theme().fg(ColorToken::Success, STATUS_BULLET)
            }
        } else if is_truncated {
            current_theme().fg(ColorToken::Error, "✗ ")
        } else {
            current_theme().fg(ColorToken::Text, STATUS_BULLET)
        };

        let name = &self.tool_call.name;
        if name == "ExitPlanMode" {
            let label = current_theme().bold_fg(ColorToken::Primary, "Current plan");
            if !is_finished || result.is_none() || result.as_ref().is_some_and(|r| r.is_error) {
                return label;
            }
            let outcome = interpret_exit_plan_mode_outcome(&result.as_ref().unwrap().output);
            match outcome {
                ExitPlanModeOutcome::Approved { chosen } => {
                    let chip_text = match chosen {
                        Some(c) if !c.is_empty() => format!("Approved: {c}"),
                        _ => "Approved".to_owned(),
                    };
                    format!(
                        "{label}{}",
                        current_theme().fg(ColorToken::Success, &format!(" · {chip_text}"))
                    )
                }
                ExitPlanModeOutcome::AutoApproved => {
                    format!(
                        "{label}{}",
                        current_theme().fg(ColorToken::Warning, " · Auto-approved")
                    )
                }
                ExitPlanModeOutcome::Rejected { .. } => label,
            }
        } else if name == "AllDone" {
            if is_error {
                return format!(
                    "{bullet}{}",
                    current_theme().bold_fg(ColorToken::Error, "AllDone failed")
                );
            }
            let label = if is_finished {
                "Work complete"
            } else {
                "Completing work"
            };
            let tone = if is_finished {
                ColorToken::Success
            } else {
                ColorToken::Primary
            };
            format!("{bullet}{}", current_theme().bold_fg(tone, label))
        } else if name == "AskUserQuestion" {
            let is_background_ask = self
                .tool_call
                .args
                .get("background")
                .and_then(|v| v.as_bool())
                == Some(true);
            let label = if is_finished {
                if is_error {
                    "Could not collect your input"
                } else if is_background_ask {
                    "Started background question"
                } else {
                    "Collected your answers"
                }
            } else if is_background_ask {
                "Starting background question"
            } else {
                "Waiting for your input"
            };
            let tone = if is_error {
                ColorToken::Error
            } else {
                ColorToken::Primary
            };
            format!("{bullet}{}", current_theme().bold_fg(tone, label))
        } else if name == "Bash" {
            if is_truncated {
                return format!(
                    "{bullet}{} {}",
                    current_theme().fg(ColorToken::Error, "Truncated"),
                    current_theme().bold_fg(ColorToken::Primary, "Bash")
                );
            }
            let label = if is_finished {
                "Ran a command"
            } else {
                "Running a command"
            };
            let tone = if is_error {
                ColorToken::Error
            } else {
                ColorToken::Primary
            };
            let chip_str = if is_finished {
                if let Some(result) = result {
                    self.build_header_chip(result)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            format!(
                "{bullet}{}{}",
                current_theme().bold_fg(tone, label),
                chip_str
            )
        } else {
            let verb = if is_finished {
                "Used"
            } else if is_truncated {
                "Truncated"
            } else {
                "Using"
            };
            let verb_styled = if is_truncated {
                current_theme().fg(ColorToken::Error, verb)
            } else {
                verb.to_owned()
            };
            let tool_label = current_theme().bold_fg(ColorToken::Primary, name);
            let arg_str = extract_key_argument(name, &self.tool_call.args)
                .map(|a| current_theme().dim(&format!(" ({a})")))
                .unwrap_or_default();
            let chip_str = if is_finished {
                if let Some(result) = result {
                    self.build_header_chip(result)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            format!("{bullet}{verb_styled} {tool_label}{arg_str}{chip_str}")
        }
    }

    fn build_header_chip(&self, result: &ToolResultData) -> String {
        let text = pick_chip(&self.tool_call.name, &self.tool_call, result);
        if text.is_empty() {
            return String::new();
        }
        if result.is_error {
            current_theme().fg(ColorToken::Error, &format!(" · {text}"))
        } else {
            current_theme().dim(&format!(" · {text}"))
        }
    }

    fn build_call_preview(&mut self) {
        let name = &self.tool_call.name;
        if name == "ExitPlanMode" {
            self.build_plan_preview();
            return;
        }
        if self.result.is_none() && self.tool_call.truncated {
            self.children.push(Box::new(Text::new(
                &current_theme()
                    .dim("Tool call arguments truncated by max_tokens — call never executed."),
                2,
                0,
            )));
            return;
        }
        if name == "Bash" {
            let command = crate::components::messages::tool_renderers::str_arg(
                &self.tool_call.args,
                &["command"],
            );
            if command.is_empty() {
                return;
            }
            self.children.push(Box::new(ShellExecutionComponent::new(
                ShellExecutionOptions {
                    command: Some(command),
                    result: None,
                    expanded: self.expanded,
                    show_command: true,
                    command_preview_lines: if self.expanded {
                        None
                    } else {
                        Some(COMMAND_PREVIEW_LINES)
                    },
                    result_preview_lines: None,
                    tail_output: false,
                    expand_hint: true,
                },
            )));
        }
        // Write/Edit diff previews land with the media slice.
    }

    fn build_plan_preview(&mut self) {
        let plan = self.resolve_plan_for_preview();
        if plan.is_empty() {
            return;
        }
        let border_hex = current_theme().color(ColorToken::Success);
        self.children.push(Box::new(PlanBoxComponent::new(
            &plan,
            &border_hex,
            None,
            None,
        )));
    }

    fn resolve_plan_for_preview(&self) -> String {
        let inline_plan =
            crate::components::messages::tool_renderers::str_arg(&self.tool_call.args, &["plan"]);
        if !inline_plan.is_empty() {
            return inline_plan;
        }
        if let Some(result) = &self.result {
            if !result.is_error {
                if let Some(plan) = extract_approved_plan(&result.output) {
                    return plan;
                }
            }
        }
        String::new()
    }

    fn build_content(&mut self) {
        let Some(result) = self.result.clone() else {
            return;
        };
        let name = self.tool_call.name.clone();

        // system-reminder outputs are harness noise.
        if result.output.trim_start().starts_with("<system-reminder>") {
            return;
        }

        // TodoList / EnterPlanMode / WaitFor / AllDone non-error: header only.
        if matches!(
            name.as_str(),
            "TodoList" | "EnterPlanMode" | "WaitFor" | "AllDone"
        ) && !result.is_error
        {
            return;
        }

        // AskUserQuestion structured result.
        if name == "AskUserQuestion"
            && self
                .tool_call
                .args
                .get("background")
                .and_then(|v| v.as_bool())
                != Some(true)
            && !result.is_error
            && self.render_ask_user_question_result(&result)
        {
            return;
        }

        if let Some(components) = render_tool_result_body(&name, &result, self.expanded) {
            for component in components {
                self.children.push(component);
            }
        }
    }

    fn render_ask_user_question_result(&mut self, result: &ToolResultData) -> bool {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result.output);
        let Ok(parsed) = parsed else {
            return false;
        };
        if !parsed.is_object() {
            return false;
        }
        let answers = parsed.get("answers");
        let note = parsed.get("note").and_then(|v| v.as_str());
        let has_answers =
            answers.is_some_and(|a| a.is_object() && !a.as_object().unwrap().is_empty());
        if !has_answers {
            let note_text = note
                .filter(|n| !n.is_empty())
                .unwrap_or("User dismissed the question.");
            self.children.push(Box::new(Text::new(
                &current_theme().dim(&format!("  {note_text}")),
                0,
                0,
            )));
            return true;
        }
        let answers = answers.unwrap().as_object().unwrap();
        for (question, answer) in answers {
            let answer_text = match answer {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            self.children.push(Box::new(Text::new(
                &format!("  {}  {question}", current_theme().dim("Q")),
                0,
                0,
            )));
            self.children.push(Box::new(Text::new(
                &format!(
                    "  {}  {answer_text}",
                    current_theme().fg(ColorToken::Primary, "→")
                ),
                0,
                0,
            )));
        }
        true
    }
}

/// `extractApprovedPlan` — pull the plan body after the approved marker.
fn extract_approved_plan(output: &str) -> Option<String> {
    const APPROVED_PLAN_MARKER: &str = "## Approved Plan:";
    const AUTO_APPROVED_PLAN_MARKER: &str = "## Plan (auto-approved, not user-reviewed):";
    let marker = if output.contains(AUTO_APPROVED_PLAN_MARKER) {
        AUTO_APPROVED_PLAN_MARKER
    } else {
        APPROVED_PLAN_MARKER
    };
    let marker_index = output.find(marker)?;
    let plan = output[marker_index + marker.len()..].trim();
    if plan.is_empty() {
        None
    } else {
        Some(plan.to_owned())
    }
}

impl Component for ToolCallComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if let Some((cached_width, cached_lines)) = &self.render_cache {
            if *cached_width == width {
                return cached_lines.clone();
            }
        }
        let mut out = Vec::new();
        for child in &mut self.children {
            out.extend(child.render(width));
        }
        self.render_cache = Some((width, out.clone()));
        out
    }

    fn invalidate(&mut self) {
        self.render_cache = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::theme::{DARK_COLORS, set_palette};

    fn ask_user(output: &str, is_error: bool) -> ToolCallComponent {
        ToolCallComponent::new(
            ToolCallData {
                id: "c".into(),
                name: "AskUserQuestion".into(),
                args: serde_json::json!({ "questions": [{"question": "Pick one?"}] })
                    .as_object()
                    .cloned()
                    .unwrap(),
                truncated: false,
            },
            Some(ToolResultData {
                tool_call_id: "c".into(),
                output: output.to_owned(),
                is_error,
            }),
        )
    }

    #[test]
    fn ask_user_answered_renders_qa_lines() {
        set_palette(DARK_COLORS);
        let mut c = ask_user(r#"{"answers":{"Pick one?":"Rust"}}"#, false);
        let lines = c.render(80);
        let joined = lines.join("\n");
        assert!(joined.contains("Q"), "should show Q marker: {joined}");
        assert!(
            joined.contains("Pick one?"),
            "should show question: {joined}"
        );
        assert!(joined.contains("Rust"), "should show answer: {joined}");
    }

    #[test]
    fn ask_user_dismissed_renders_note() {
        set_palette(DARK_COLORS);
        let mut c = ask_user(r#"{"answers":{}}"#, false);
        let lines = c.render(80);
        let joined = lines.join("\n");
        assert!(joined.contains("User dismissed the question."), "{joined}");
    }
}
