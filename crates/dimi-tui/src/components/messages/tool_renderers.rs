//! Tool result renderers — chip providers, summary renderers, and the
//! truncated output component (port of
//! `apps/dimi/src/tui/components/messages/tool-renderers/*`).

use crate::component::Component;
use crate::components::messages::RESULT_PREVIEW_LINES;
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};

/// A tool call block (subset of `ToolCallBlockData` used by slice 2).
#[derive(Debug, Clone)]
pub struct ToolCallData {
    pub id: String,
    pub name: String,
    pub args: serde_json::Map<String, serde_json::Value>,
    pub truncated: bool,
}

/// A tool result block (subset of `ToolResultBlockData`).
#[derive(Debug, Clone)]
pub struct ToolResultData {
    pub tool_call_id: String,
    pub output: String,
    pub is_error: bool,
}

/// `strArg` — first non-empty string arg among the keys.
pub fn str_arg(args: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = args.get(*key).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                return v.to_owned();
            }
        }
    }
    String::new()
}

/// `countNonEmptyLines`.
pub fn count_non_empty_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.split('\n').filter(|l| !l.is_empty()).count()
}

fn pluralize(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{n} {singular}s")
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

/// `computeWriteStats` — line count with trailing-newline normalization.
fn compute_write_lines(args: &serde_json::Map<String, serde_json::Value>) -> usize {
    let content = str_arg(args, &["content"]);
    let normalized = content.strip_suffix('\n').unwrap_or(&content);
    if normalized.is_empty() {
        0
    } else {
        normalized.split('\n').count()
    }
}

/// Chip providers — short numeric "stat" suffixes for tool headers
/// (`pickChip` + `buildHeaderChip`).
pub fn pick_chip(tool_name: &str, tool_call: &ToolCallData, result: &ToolResultData) -> String {
    match tool_name {
        "Edit" => {
            let old_str = str_arg(&tool_call.args, &["old_string"]);
            let new_str = str_arg(&tool_call.args, &["new_string"]);
            if old_str.is_empty() && new_str.is_empty() {
                return String::new();
            }
            // computeEditStats: added/removed line counts. Slice 2 uses a
            // line-count approximation matching the chip for simple content;
            // the full diff renderer lands with diff-preview.
            let old_lines = old_str.lines().count();
            let new_lines = new_str.lines().count();
            let mut parts = Vec::new();
            if new_lines > old_lines {
                parts.push(format!("+{}", new_lines - old_lines));
            }
            if old_lines > new_lines {
                parts.push(format!("-{}", old_lines - new_lines));
            }
            if parts.is_empty() {
                return String::new();
            }
            parts.join(" ")
        }
        "Write" => pluralize(compute_write_lines(&tool_call.args), "line"),
        "Read" => pluralize(count_non_empty_lines(&result.output), "line"),
        "ReadMediaFile" => String::new(),
        "Grep" => {
            let matches = count_non_empty_lines(&result.output);
            if matches == 0 {
                "no matches".to_owned()
            } else if matches == 1 {
                "1 match".to_owned()
            } else {
                format!("{matches} matches")
            }
        }
        "Glob" => {
            let files = count_non_empty_lines(&result.output);
            if files == 0 {
                "no files".to_owned()
            } else {
                pluralize(files, "file")
            }
        }
        "FetchURL" => format_bytes(result.output.len()),
        "WebSearch" => {
            let lines: Vec<&str> = result
                .output
                .split('\n')
                .filter(|l| !l.trim().is_empty())
                .collect();
            let mut count = 0usize;
            for line in &lines {
                let t = line.trim_start();
                // TS regex: /^\s*(\d+\.|[-*])\s+/ — digits + "." or "-"/"*"
                // followed by whitespace.
                let rest = t.trim_start_matches(|c: char| c.is_ascii_digit());
                let after_digits = rest != t;
                let numbered = after_digits
                    && rest.starts_with('.')
                    && rest[1..].chars().next().is_some_and(|c| c.is_whitespace());
                let bullet = (t.starts_with('-') || t.starts_with('*'))
                    && t[1..].chars().next().is_some_and(|c| c.is_whitespace());
                if numbered || bullet {
                    count += 1;
                }
            }
            if count == 0 {
                if lines.is_empty() {
                    "no results".to_owned()
                } else {
                    "web result".to_owned()
                }
            } else {
                pluralize(count, "result")
            }
        }
        _ => String::new(),
    }
}

/// Trim trailing empty lines (`trimTrailingEmptyLines`).
pub fn trim_trailing_empty_lines(lines: Vec<String>) -> Vec<String> {
    let mut end = lines.len();
    while end > 0 {
        if lines[end - 1].is_empty() {
            end -= 1;
        } else {
            break;
        }
    }
    lines[..end].to_vec()
}

/// Component that renders tool output with wrap-aware line truncation
/// (port of `TruncatedOutputComponent`).
pub struct TruncatedOutputComponent {
    text_component: Text,
    expanded: bool,
    max_lines: usize,
    indent: usize,
    expand_hint: bool,
    tail: bool,
}

impl TruncatedOutputComponent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        output: &str,
        expanded: bool,
        is_error: bool,
        max_lines: Option<usize>,
        indent: Option<usize>,
        expand_hint: Option<bool>,
        tail: Option<bool>,
        color: ColorToken,
    ) -> Self {
        let max_lines = max_lines.unwrap_or(RESULT_PREVIEW_LINES);
        let indent = indent.unwrap_or(2);
        let expand_hint = expand_hint.unwrap_or(true);
        let tail = tail.unwrap_or(false);
        let cleaned =
            trim_trailing_empty_lines(output.split('\n').map(str::to_owned).collect()).join("\n");
        let text = if is_error {
            current_theme().fg(ColorToken::Error, &cleaned)
        } else {
            current_theme().fg(color, &cleaned)
        };
        TruncatedOutputComponent {
            text_component: Text::new(&text, indent, 0),
            expanded,
            max_lines,
            indent,
            expand_hint,
            tail,
        }
    }

    fn render_hint(&self, width: usize, hint: &str) -> String {
        let indent_width = self.indent.min(width.max(0));
        let hint_width = width.saturating_sub(indent_width);
        format!(
            "{}{}",
            " ".repeat(indent_width),
            current_theme().dim(&crate::wrap::truncate_to_width(
                hint, hint_width, "…", false
            ))
        )
    }
}

impl Component for TruncatedOutputComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let content_lines = self.text_component.render(width);
        if self.expanded || content_lines.len() <= self.max_lines {
            return content_lines;
        }
        let remaining = content_lines.len() - self.max_lines;
        if self.tail {
            let shown = content_lines[content_lines.len() - self.max_lines..].to_vec();
            let mut out = Vec::new();
            out.push(self.render_hint(width, &format!("... ({remaining} earlier lines)")));
            out.extend(shown);
            out
        } else {
            let shown = content_lines[..self.max_lines].to_vec();
            let hint = if self.expand_hint {
                format!("... ({remaining} more lines, ctrl+o to expand)")
            } else {
                format!("... ({remaining} more lines)")
            };
            let mut out = shown;
            out.push(self.render_hint(width, &hint));
            out
        }
    }

    fn invalidate(&mut self) {
        self.text_component.invalidate();
    }
}

/// `pickResultRenderer` — the tool result body renderer dispatch. Returns the
/// body components for a finished tool call, or `None` when the body is empty
/// (mirrors `renderTruncated` / `withGlance` and the special-case skips).
pub fn render_tool_result_body(
    tool_name: &str,
    result: &ToolResultData,
    expanded: bool,
) -> Option<Vec<Box<dyn Component>>> {
    if result.output.is_empty() {
        return None;
    }
    match tool_name {
        // Summary renderers (glance null) — body only when expanded.
        "Read" | "FetchURL" | "WebSearch" | "Think" | "Edit" | "Write" => {
            if result.is_error {
                Some(vec![Box::new(TruncatedOutputComponent::new(
                    &result.output,
                    expanded,
                    true,
                    None,
                    None,
                    None,
                    None,
                    ColorToken::TextDim,
                ))])
            } else if expanded {
                Some(vec![Box::new(raw_output_component(result, 4))])
            } else {
                None
            }
        }
        // Grep/Glob show a path-sample glance when not expanded.
        "Grep" | "Glob" => {
            if result.is_error {
                Some(vec![Box::new(TruncatedOutputComponent::new(
                    &result.output,
                    expanded,
                    true,
                    None,
                    None,
                    None,
                    None,
                    ColorToken::TextDim,
                ))])
            } else {
                let mut out: Vec<Box<dyn Component>> = Vec::new();
                let glance = glance_lines(tool_name, result);
                if !glance.is_empty() {
                    out.push(Box::new(Text::new(
                        &current_theme().dim(&format!("  {glance}")),
                        0,
                        0,
                    )));
                }
                if expanded {
                    out.push(Box::new(raw_output_component(result, 4)));
                }
                if out.is_empty() { None } else { Some(out) }
            }
        }
        "Bash" => Some(vec![Box::new(TruncatedOutputComponent::new(
            &result.output,
            expanded,
            result.is_error,
            None,
            None,
            None,
            None,
            ColorToken::TextMuted,
        ))]),
        _ => Some(vec![Box::new(TruncatedOutputComponent::new(
            &result.output,
            expanded,
            result.is_error,
            None,
            None,
            None,
            None,
            ColorToken::TextDim,
        ))]),
    }
}

/// Raw dim output at an indent (expanded summary bodies).
fn raw_output_component(result: &ToolResultData, indent: usize) -> Text {
    let text = if result.is_error {
        current_theme().fg(ColorToken::Error, &result.output)
    } else {
        current_theme().dim(&result.output)
    };
    Text::new(&text, indent, 0)
}

/// Grep/Glob glance: first 3 samples (+ "N more" tail).
fn glance_lines(tool_name: &str, result: &ToolResultData) -> String {
    const GLANCE_SAMPLES: usize = 3;
    let lines: Vec<String> = result
        .output
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let samples: Vec<String> = if tool_name == "Grep" {
        lines
            .iter()
            .take(GLANCE_SAMPLES)
            .map(|l| path_from_grep_line(l))
            .collect()
    } else {
        lines.iter().take(GLANCE_SAMPLES).cloned().collect()
    };
    let remaining = lines.len() - samples.len();
    let tail = if remaining > 0 {
        format!(", +{remaining} more")
    } else {
        String::new()
    };
    format!("{}{}", samples.join(", "), tail)
}

/// Strip `:line:col:text` so the glance shows the file path only.
fn path_from_grep_line(line: &str) -> String {
    let Some(idx) = line.find(':') else {
        return line.to_owned();
    };
    if idx == 0 {
        return line.to_owned();
    }
    let Some(second) = line[idx + 1..].find(':') else {
        return line.to_owned();
    };
    line[..idx + 1 + second].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    fn websearch_result(output: &str) -> ToolResultData {
        ToolResultData {
            tool_call_id: "c".into(),
            output: output.to_owned(),
            is_error: false,
        }
    }

    fn call(query: &str) -> ToolCallData {
        ToolCallData {
            id: "c".into(),
            name: "WebSearch".into(),
            args: serde_json::json!({ "query": query })
                .as_object()
                .cloned()
                .unwrap(),
            truncated: false,
        }
    }

    #[test]
    fn websearch_chip_counts_list_items_only() {
        set_palette(DARK_COLORS);
        // Matches: numbered list items and dash/star bullets.
        let r = websearch_result("1. first\n2. second\n- third");
        assert_eq!(pick_chip("WebSearch", &call("q"), &r), "3 results");
        // "10) ten" is NOT a match (TS regex requires \d+\. or [-*]).
        let r = websearch_result("10) ten\n7) seven");
        assert_eq!(pick_chip("WebSearch", &call("q"), &r), "web result");
        // Empty → no results.
        let r = websearch_result("");
        assert_eq!(pick_chip("WebSearch", &call("q"), &r), "no results");
        // Plain prose → web result.
        let r = websearch_result("Search results...");
        assert_eq!(pick_chip("WebSearch", &call("q"), &r), "web result");
    }
}
