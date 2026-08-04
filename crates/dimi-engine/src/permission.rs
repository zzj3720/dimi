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
    /// User rules, in order (deny rules are scanned before ask/allow rules,
    /// mirroring the TS `permissionPolicyService` chain order).
    #[serde(default)]
    pub rules: Vec<PermissionRule>,
    /// Session-scope patterns approved earlier in this session (auto-approved).
    #[serde(default)]
    pub session_approved_patterns: Vec<String>,
    /// The tool-provided value that `ToolName(argGlob)` patterns match
    /// against (Bash: the command string — mirrors `execution.matchesRule`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_arg: Option<String>,
    /// The turn's working directory — the workspace root the git policies
    /// adjudicate against (TS `workspaceContext.workDir` parity).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Candidate file paths the tool call touches, extracted from the tool's
    /// standard arguments (Read/Write/Edit/Grep/Glob `path`) — the Rust
    /// approximation of the TS tools' declared `execution.accesses`, used by
    /// the sensitive-file / git-control-path policies.
    #[serde(default)]
    pub paths: Vec<String>,
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

/// Evaluate the policy chain for one tool call. Mirrors the TS
/// `permissionPolicyService` ordered chain:
/// auto-mode-ask-user-question-deny → user-deny → auto-approve →
/// session-history → user-ask → user-allow → sensitive-file-ask →
/// git-control-path-ask → yolo-approve → default-tool-approve →
/// git-cwd-write-approve → fallback-ask.
pub fn evaluate(input: &PolicyInput) -> PolicyDecision {
    // 1. auto-mode-ask-user-question-deny (TS chain order #1).
    if (input.mode == PermissionMode::Auto || input.mode == PermissionMode::Yolo)
        && input.tool_name == "AskUserQuestion"
    {
        return PolicyDecision::Deny {
            reason: "AskUserQuestion is disabled while auto permission mode is active. \
                     Make a reasonable decision and continue without asking the user."
                .to_string(),
        };
    }

    // 2. user-configured-deny (#2): every deny rule is scanned before any
    //    allow/ask rule — a user deny overrides the whitelist, auto mode and
    //    session history (TS parity).
    for rule in &input.rules {
        if rule.decision != RuleDecision::Deny {
            continue;
        }
        if rule_matches(rule, input) {
            return PolicyDecision::Deny {
                reason: rule.reason.clone().unwrap_or_else(|| {
                    format!(
                        "Tool \"{}\" was denied by permission rule.",
                        input.tool_name
                    )
                }),
            };
        }
    }

    // 3. auto-mode-approve (#3).
    if input.mode == PermissionMode::Auto {
        return PolicyDecision::Approve;
    }

    // 4. session-approval-history (#4): approved-for-session patterns
    //    auto-approve (before user ask/allow rules and the whitelist).
    for pattern in &input.session_approved_patterns {
        if pattern_matches_tool(pattern, &input.tool_name) {
            return PolicyDecision::Approve;
        }
    }

    // 5. user-configured-ask (#5).
    for rule in &input.rules {
        if rule.decision != RuleDecision::Ask {
            continue;
        }
        if rule_matches(rule, input) {
            return PolicyDecision::Ask;
        }
    }

    // 6. user-configured-allow (#6).
    for rule in &input.rules {
        if rule.decision != RuleDecision::Allow {
            continue;
        }
        if rule_matches(rule, input) {
            return PolicyDecision::Approve;
        }
    }

    // 7. sensitive-file-access-ask (#7): a path access to a sensitive file
    //    (env / credentials / SSH keys) asks even for whitelisted tools.
    if input
        .paths
        .iter()
        .any(|path| is_sensitive_file(path))
    {
        return PolicyDecision::Ask;
    }

    // 8. git-control-path-access-ask (#8): accessing `.git` components or a
    //    git control directory asks.
    if git_control_path_access(input) {
        return PolicyDecision::Ask;
    }

    // 9. yolo-mode-approve (#9) — after the deny/sensitive/git layers (TS
    //    chain order: yolo still respects user deny and sensitive-file ask).
    if input.mode == PermissionMode::Yolo {
        return PolicyDecision::Approve;
    }

    // 10. default-tool-approve (#10) — the built-in whitelist. It sits after
    //     the deny/session/sensitive/git layers, so a user deny or a
    //     sensitive path wins over a whitelisted tool (TS parity).
    if DEFAULT_APPROVE_TOOLS.contains(&input.tool_name.as_str()) {
        return PolicyDecision::Approve;
    }

    // 11. git-cwd-write-approve (#11): Write/Edit confined to the workspace
    //     of a git work tree approve (trusted in-repo edits).
    if git_cwd_write_approve(input) {
        return PolicyDecision::Approve;
    }

    // 12. fallback-ask (#12).
    PolicyDecision::Ask
}

/// Candidate file paths a tool call touches, extracted from the tool's
/// standard arguments (the Rust approximation of the TS tools' declared
/// `execution.accesses` — Read/Write/Edit/Grep/Glob all take `path`).
pub fn extract_access_paths(tool_name: &str, args: &serde_json::Value) -> Vec<String> {
    if !matches!(tool_name, "Read" | "ReadMediaFile" | "Write" | "Edit" | "Grep" | "Glob") {
        return Vec::new();
    }
    match args.get("path").and_then(|v| v.as_str()) {
        Some(path) if !path.is_empty() => vec![path.to_string()],
        _ => Vec::new(),
    }
}

const SENSITIVE_DOT_VARIANT_SUFFIXES: &[&str] = &[
    ".bak", ".backup", ".copy", ".disabled", ".key", ".old", ".orig", ".pem", ".save", ".tmp",
];

/// Port of TS `isSensitiveFile` (tool/path-access.ts): env files, credential
/// stores and SSH private keys (with the documented exemptions).
fn is_sensitive_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let comparable_name = name.to_lowercase();
    let comparable_path = path.to_lowercase();

    if matches!(
        comparable_name.as_str(),
        ".env.example" | ".env.sample" | ".env.template" | "id_rsa.pub" | "id_ed25519.pub" | "id_ecdsa.pub"
    ) {
        return false;
    }
    if matches!(
        comparable_name.as_str(),
        ".env" | "id_rsa" | "id_ed25519" | "id_ecdsa" | "credentials"
    ) {
        return true;
    }
    if comparable_name.starts_with(".env.") {
        return true;
    }
    for prefix in ["id_rsa", "id_ed25519", "id_ecdsa", "credentials"] {
        if comparable_name == prefix {
            return true;
        }
        if comparable_name.len() > prefix.len() && comparable_name.starts_with(prefix) {
            let suffix = &comparable_name[prefix.len()..];
            let next = suffix.chars().next().unwrap_or('\0');
            if next == '-' || next == '_' {
                return true;
            }
            if next == '.' && SENSITIVE_DOT_VARIANT_SUFFIXES.contains(&suffix) {
                return true;
            }
        }
    }
    for (dir, file) in [(".aws", "credentials"), (".gcp", "credentials")] {
        let suffix = format!("{dir}/{file}");
        if comparable_path.ends_with(&format!("/{suffix}"))
            || comparable_path.contains(&format!("/{suffix}/"))
        {
            return true;
        }
    }
    false
}

/// `isWithinDirectory` (posix): the target is inside `dir` (or equals it).
fn is_within_directory(target: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    target == dir || target.starts_with(&format!("{dir}/"))
}

/// Any path component (under or equal to `cwd`) named `.git` — the TS
/// `hasGitPathComponent` check. Conservative simplification: components are
/// matched on the whole path, not only the cwd-relative tail.
fn has_git_path_component(target: &str) -> bool {
    target
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(".git"))
}

/// Walk up from `cwd` looking for a `.git` directory/file — the TS
/// `findLocalGitWorkTreeMarker` probe (bounded walk, no symlink following).
fn find_git_worktree_marker(cwd: &str) -> Option<String> {
    let mut current = cwd.trim_end_matches('/').to_string();
    for _ in 0..256 {
        if current.is_empty() {
            return None;
        }
        let dot_git = format!("{current}/.git");
        if std::path::Path::new(&dot_git).exists() {
            return Some(dot_git);
        }
        let parent = match current.rfind('/') {
            Some(idx) if idx > 0 => current[..idx].to_string(),
            Some(_) => "/".to_string(),
            None => return None,
        };
        if parent == current {
            return None;
        }
        current = parent;
    }
    None
}

/// Git-control-path access ask (TS `git-control-path-access-ask`): a direct
/// `.git` path component, or a path inside the nearest git control dir.
fn git_control_path_access(input: &PolicyInput) -> bool {
    if input.paths.is_empty() {
        return false;
    }
    if input
        .paths
        .iter()
        .any(|path| has_git_path_component(path))
    {
        return true;
    }
    let Some(cwd) = input.cwd.as_deref() else {
        return false;
    };
    let Some(marker) = find_git_worktree_marker(cwd) else {
        return false;
    };
    input
        .paths
        .iter()
        .any(|path| is_within_directory(path, &marker))
}

/// Git-cwd-write approve (TS `git-cwd-write-approve`): Write/Edit confined
/// to the workspace of a git work tree approve.
fn git_cwd_write_approve(input: &PolicyInput) -> bool {
    if !matches!(input.tool_name.as_str(), "Write" | "Edit") {
        return false;
    }
    if input.paths.is_empty() {
        return false;
    }
    let Some(cwd) = input.cwd.as_deref() else {
        return false;
    };
    if !input.paths.iter().all(|path| is_within_directory(path, cwd)) {
        return false;
    }
    find_git_worktree_marker(cwd).is_some()
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
            cwd: None,
            paths: vec![],
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

    #[test]
    fn user_deny_overrides_whitelist_and_auto_mode() {
        // P1-3 (review): the TS chain runs user-configured-deny BEFORE
        // auto-approve and the default whitelist — a user deny of a
        // whitelisted tool must win, in every mode.
        let mut i = input("Read", PermissionMode::Manual);
        i.rules = vec![PermissionRule {
            decision: RuleDecision::Deny,
            scope: "user".to_string(),
            pattern: "Read".to_string(),
            reason: Some("no reads".to_string()),
        }];
        assert_eq!(
            evaluate(&i),
            PolicyDecision::Deny {
                reason: "no reads".to_string()
            }
        );
        // Auto mode still honors the user deny (TS chain order #2 before #3).
        i.mode = PermissionMode::Auto;
        assert_eq!(
            evaluate(&i),
            PolicyDecision::Deny {
                reason: "no reads".to_string()
            }
        );
        // Deny rules are scanned before allow rules, regardless of order.
        let mut j = input("Bash", PermissionMode::Manual);
        j.match_arg = Some("rm -rf /".to_string());
        j.rules = vec![
            PermissionRule {
                decision: RuleDecision::Allow,
                scope: "user".to_string(),
                pattern: "Bash".to_string(),
                reason: None,
            },
            PermissionRule {
                decision: RuleDecision::Deny,
                scope: "user".to_string(),
                pattern: "Bash(rm*)".to_string(),
                reason: Some("no rm".to_string()),
            },
        ];
        assert_eq!(
            evaluate(&j),
            PolicyDecision::Deny {
                reason: "no rm".to_string()
            }
        );
    }

    #[test]
    fn deny_default_message_matches_ts() {
        let mut i = input("Bash", PermissionMode::Manual);
        i.rules = vec![PermissionRule {
            decision: RuleDecision::Deny,
            scope: "user".to_string(),
            pattern: "Bash".to_string(),
            reason: None,
        }];
        assert_eq!(
            evaluate(&i),
            PolicyDecision::Deny {
                reason: "Tool \"Bash\" was denied by permission rule.".to_string()
            }
        );
    }

    #[test]
    fn auto_mode_denies_ask_user_question() {
        // P1-4 (review): auto-mode-ask-user-question-deny is the chain's
        // first node.
        assert_eq!(
            evaluate(&input("AskUserQuestion", PermissionMode::Auto)),
            PolicyDecision::Deny {
                reason: "AskUserQuestion is disabled while auto permission mode is active. \
                         Make a reasonable decision and continue without asking the user."
                    .to_string()
            }
        );
        assert_eq!(
            evaluate(&input("AskUserQuestion", PermissionMode::Yolo)),
            PolicyDecision::Deny {
                reason: "AskUserQuestion is disabled while auto permission mode is active. \
                         Make a reasonable decision and continue without asking the user."
                    .to_string()
            }
        );
        // Manual mode still falls back to ask for AskUserQuestion (it is
        // whitelisted → default-tool-approve).
        assert_eq!(
            evaluate(&input("AskUserQuestion", PermissionMode::Manual)),
            PolicyDecision::Approve
        );
    }

    #[test]
    fn sensitive_file_access_asks_even_for_whitelisted_tools() {
        // P1-4 (review): sensitive-file-access-ask runs before the default
        // whitelist, so reading `.env` with Read asks in manual mode.
        let mut i = input("Read", PermissionMode::Manual);
        i.paths = vec!["/workspace/.env".to_string()];
        assert_eq!(evaluate(&i), PolicyDecision::Ask);

        // Exemptions do not trigger the ask.
        i.paths = vec!["/workspace/.env.example".to_string()];
        assert_eq!(evaluate(&i), PolicyDecision::Approve);

        // Non-sensitive paths keep the whitelist approval.
        i.paths = vec!["/workspace/src/main.ts".to_string()];
        assert_eq!(evaluate(&i), PolicyDecision::Approve);

        // SSH private keys are sensitive; public keys are exempt.
        let mut k = input("Read", PermissionMode::Manual);
        k.paths = vec!["/root/.ssh/id_ed25519".to_string()];
        assert_eq!(evaluate(&k), PolicyDecision::Ask);
        k.paths = vec!["/root/.ssh/id_ed25519.pub".to_string()];
        assert_eq!(evaluate(&k), PolicyDecision::Approve);
    }

    #[test]
    fn git_control_path_access_asks() {
        // P1-4 (review): accessing `.git` internals asks even for
        // whitelisted tools.
        let mut i = input("Read", PermissionMode::Manual);
        i.paths = vec!["/repo/.git/config".to_string()];
        i.cwd = Some("/repo".to_string());
        assert_eq!(evaluate(&i), PolicyDecision::Ask);

        // A path inside the git control dir also asks.
        i.paths = vec!["/repo/.git/objects/ab/cdef".to_string()];
        assert_eq!(evaluate(&i), PolicyDecision::Ask);

        // Ordinary repo files do not.
        i.paths = vec!["/repo/src/main.ts".to_string()];
        assert_eq!(evaluate(&i), PolicyDecision::Approve);
    }

    #[test]
    fn git_cwd_write_approves_workspace_edits_in_a_git_tree() {
        // P1-4 (review): git-cwd-write-approve sits after the whitelist —
        // Write/Edit inside a git work tree approve in manual mode.
        let tmp = std::env::temp_dir().join(format!("dimi-perm-{}", std::process::id()));
        let repo = tmp.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let repo_str = repo.to_string_lossy().to_string();

        let mut i = input("Write", PermissionMode::Manual);
        i.cwd = Some(repo_str.clone());
        i.paths = vec![format!("{repo_str}/src/new.ts")];
        assert_eq!(evaluate(&i), PolicyDecision::Approve);

        // Non-git directories keep the fallback ask.
        let mut j = input("Write", PermissionMode::Manual);
        j.cwd = Some(tmp.to_string_lossy().to_string());
        j.paths = vec![tmp.join("a.txt").to_string_lossy().to_string()];
        assert_eq!(evaluate(&j), PolicyDecision::Ask);

        // Escaping the workspace is not approved.
        let mut k = input("Write", PermissionMode::Manual);
        k.cwd = Some(repo_str.clone());
        k.paths = vec!["/tmp/escape.txt".to_string()];
        assert_eq!(evaluate(&k), PolicyDecision::Ask);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn session_history_precedes_allow_rules_and_whitelist() {
        // P1-3 (review): session-approval-history runs before user ask/allow
        // rules — an approved-for-session pattern wins over an ask rule.
        let mut i = input("Bash", PermissionMode::Manual);
        i.session_approved_patterns = vec!["Bash".to_string()];
        i.rules = vec![PermissionRule {
            decision: RuleDecision::Ask,
            scope: "user".to_string(),
            pattern: "Bash".to_string(),
            reason: None,
        }];
        assert_eq!(evaluate(&i), PolicyDecision::Approve);
    }

    #[test]
    fn extract_paths_covers_file_tools_only() {
        assert_eq!(
            extract_access_paths(
                "Read",
                &serde_json::json!({ "path": "/workspace/a.txt" })
            ),
            vec!["/workspace/a.txt".to_string()]
        );
        assert_eq!(
            extract_access_paths("Bash", &serde_json::json!({ "command": "cat .env" })),
            Vec::<String>::new()
        );
        assert_eq!(
            extract_access_paths("Write", &serde_json::json!({ "path": "/x" })),
            vec!["/x".to_string()]
        );
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
