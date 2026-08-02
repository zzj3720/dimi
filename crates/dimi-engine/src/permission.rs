//! Permission policy chain — the slice-2a pure evaluation core.
//!
//! Mirrors the TS `permissionPolicyService` chain (default-tool-approve →
//! auto-mode-approve → yolo-mode-approve → user rules → session history →
//! fallback ask) plus the `permissionRules` pattern syntax
//! (`ToolName` / `ToolName(argGlob)`).

use serde::{Deserialize, Serialize};

/// Permission mode (permissionModeOps.ts: manual is the default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Manual,
    Yolo,
    Auto,
}

/// Rule decision vocabulary (permissionRules.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleDecision {
    Allow,
    Deny,
    Ask,
}

/// User-configured permission rule (PermissionRule in permissionRules.ts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub decision: RuleDecision,
    #[serde(default)]
    pub scope: String,
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The policy outcome for one tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PolicyDecision {
    Approve,
    Deny { reason: String },
    Ask,
}

/// Inputs for one tool-call policy evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyInput {
    pub mode: PermissionMode,
    pub tool_name: String,
    /// Tool arguments (JSON) — used to match `ToolName(argGlob)` patterns.
    pub args: serde_json::Value,
    /// User rules, in order (first match wins).
    #[serde(default)]
    pub rules: Vec<PermissionRule>,
    /// Session-scope patterns approved earlier in this session (auto-approved).
    #[serde(default)]
    pub session_approved_patterns: Vec<String>,
    /// The tool-provided value that `ToolName(argGlob)` patterns match
    /// against (Bash: the command string — mirrors `execution.matchesRule`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_arg: Option<String>,
}

/// Built-in tools that auto-approve in manual mode (default-tool-approve).
pub const DEFAULT_APPROVE_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "ReadMediaFile",
    "SetTodoList",
    "TodoList",
    "TaskList",
    "TaskOutput",
    "WaitFor",
    "CronList",
    "WebSearch",
    "FetchURL",
    "Agent",
    "AgentSwarm",
    "AskUserQuestion",
    "Skill",
    "EnterPlanMode",
    "ExitPlanMode",
    "select_tools",
    "AllDone",
];

/// Evaluate the policy chain for one tool call. Mirrors the TS chain:
/// built-in whitelist → auto/yolo mode → user rules → session history →
/// fallback ask.
pub fn evaluate(input: &PolicyInput) -> PolicyDecision {
    // 1. Built-in whitelist (manual mode still auto-approves these).
    if DEFAULT_APPROVE_TOOLS.contains(&input.tool_name.as_str()) {
        return PolicyDecision::Approve;
    }

    // 2/3. auto / yolo approve everything.
    if input.mode == PermissionMode::Auto || input.mode == PermissionMode::Yolo {
        return PolicyDecision::Approve;
    }

    // 4. User rules (first match wins).
    for rule in &input.rules {
        if rule_matches(rule, input) {
            return match rule.decision {
                RuleDecision::Allow => PolicyDecision::Approve,
                RuleDecision::Deny => PolicyDecision::Deny {
                    reason: rule.reason.clone().unwrap_or_else(|| {
                        format!(
                            "Tool \"{}\" was denied by permission policy.",
                            input.tool_name
                        )
                    }),
                },
                RuleDecision::Ask => PolicyDecision::Ask,
            };
        }
    }

    // 5. Session-approved history (scope=session approvals re-run automatically).
    for pattern in &input.session_approved_patterns {
        if pattern_matches_tool(pattern, &input.tool_name) {
            return PolicyDecision::Approve;
        }
    }

    // 6. Fallback: ask.
    PolicyDecision::Ask
}

/// Match one rule pattern against the tool call. Pattern syntax:
/// `ToolName` (tool name glob) or `ToolName(argGlob)` — the argument glob
/// matches the serialized JSON of the tool arguments.
fn rule_matches(rule: &PermissionRule, input: &PolicyInput) -> bool {
    match parse_pattern(&rule.pattern) {
        Some((tool_pattern, Some(arg_pattern))) => {
            glob_match(&tool_pattern, &input.tool_name)
                && input
                    .match_arg
                    .as_deref()
                    .map(|value| glob_match(&arg_pattern, value))
                    .unwrap_or(false)
        }
        Some((tool_pattern, None)) => glob_match(&tool_pattern, &input.tool_name),
        None => false,
    }
}

/// Session-history patterns are tool-name globs (recordApprovalResult keeps
/// the rule's tool part).
fn pattern_matches_tool(pattern: &str, tool_name: &str) -> bool {
    match parse_pattern(pattern) {
        Some((tool_pattern, _)) => glob_match(&tool_pattern, tool_name),
        None => false,
    }
}

/// Split `ToolName(argGlob)` into (toolPattern, Option<argGlob>). A bare
/// tool name (no parens) matches by tool name only.
fn parse_pattern(pattern: &str) -> Option<(String, Option<String>)> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return None;
    }
    let open = trimmed.find('(');
    let Some(open) = open else {
        return Some((trimmed.to_string(), None));
    };
    if !trimmed.ends_with(')') {
        return None;
    }
    let tool = trimmed[..open].trim().to_string();
    let arg = trimmed[open + 1..trimmed.len() - 1].trim().to_string();
    if tool.is_empty() {
        return None;
    }
    Some((tool, Some(arg)))
}

/// Minimal glob matcher supporting `*` (any run), `?` (one char), `**`
/// (any path-ish run) — a subset of picomatch sufficient for the rule
/// patterns the product uses (tool names and serialized-arg fragments).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.trim();
    if pattern == "*" {
        return true;
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // DP over (pattern idx, text idx); `**` consumes greedily.
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == t[j - 1],
            };
        }
    }
    dp[p.len()][t.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(tool: &str, mode: PermissionMode) -> PolicyInput {
        PolicyInput {
            mode,
            tool_name: tool.to_string(),
            args: serde_json::json!({}),
            rules: vec![],
            session_approved_patterns: vec![],
            match_arg: None,
        }
    }

    #[test]
    fn whitelist_tools_approve_in_manual() {
        assert_eq!(
            evaluate(&input("Read", PermissionMode::Manual)),
            PolicyDecision::Approve
        );
        assert_eq!(
            evaluate(&input("Grep", PermissionMode::Manual)),
            PolicyDecision::Approve
        );
        assert_eq!(
            evaluate(&input("AllDone", PermissionMode::Manual)),
            PolicyDecision::Approve
        );
    }

    #[test]
    fn non_whitelist_falls_back_to_ask_in_manual() {
        assert_eq!(
            evaluate(&input("Bash", PermissionMode::Manual)),
            PolicyDecision::Ask
        );
        assert_eq!(
            evaluate(&input("Edit", PermissionMode::Manual)),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn auto_and_yolo_approve_everything() {
        assert_eq!(
            evaluate(&input("Bash", PermissionMode::Auto)),
            PolicyDecision::Approve
        );
        assert_eq!(
            evaluate(&input("Bash", PermissionMode::Yolo)),
            PolicyDecision::Approve
        );
        assert_eq!(
            evaluate(&input("Edit", PermissionMode::Auto)),
            PolicyDecision::Approve
        );
    }

    #[test]
    fn user_rules_first_match_wins() {
        let mut i = input("Bash", PermissionMode::Manual);
        i.rules = vec![
            PermissionRule {
                decision: RuleDecision::Deny,
                scope: "user".to_string(),
                pattern: "Bash".to_string(),
                reason: Some("no bash".to_string()),
            },
            PermissionRule {
                decision: RuleDecision::Allow,
                scope: "user".to_string(),
                pattern: "Bash(ls*)".to_string(),
                reason: None,
            },
        ];
        assert_eq!(
            evaluate(&i),
            PolicyDecision::Deny {
                reason: "no bash".to_string()
            }
        );
    }

    #[test]
    fn arg_pattern_matches_serialized_args() {
        let mut i = input("Bash", PermissionMode::Manual);
        i.match_arg = Some("ls -la".to_string());
        i.rules = vec![PermissionRule {
            decision: RuleDecision::Allow,
            scope: "user".to_string(),
            pattern: "Bash(ls*)".to_string(),
            reason: None,
        }];
        assert_eq!(evaluate(&i), PolicyDecision::Approve);
    }

    #[test]
    fn session_history_approves() {
        let mut i = input("Bash", PermissionMode::Manual);
        i.session_approved_patterns = vec!["Bash".to_string()];
        assert_eq!(evaluate(&i), PolicyDecision::Approve);
    }

    #[test]
    fn glob_matches() {
        assert!(glob_match("Bash", "Bash"));
        assert!(glob_match("Bash*", "Bash"));
        assert!(glob_match("*ash", "Bash"));
        assert!(glob_match("B?sh", "Bash"));
        assert!(!glob_match("Bash", "BashX"));
        assert!(glob_match("*", "anything"));
        assert!(!glob_match("Bash(ls*)", "Bash"));
    }
}

/// One pending approval the engine pauses on (permission.approval.requested).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub request_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    /// Human action label (Bash: "Run command").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
}

/// User decision for a pending approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "camelCase")]
pub enum ApprovalDecision {
    Approved,
    Rejected {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
    },
    Cancelled,
}

/// Policy configuration passed into the engine for one turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyConfig {
    pub mode: PermissionMode,
    #[serde(default)]
    pub rules: Vec<PermissionRule>,
    #[serde(default)]
    pub session_approved_patterns: Vec<String>,
}
