//! Tool effect boundary — slice 1 ships the Bash tool over dimi-exec.
//!
//! Behavior mirrors `agent/tools/os/bash/` (bashTool.ts + bash.ts) for the
//! foreground path: schema validation, `sh -c "cd '<cwd>' && <command>"`,
//! the noninteractive env overlay, 50k/2k truncation, 60s/300s timeouts and
//! the exact result strings the TS implementation produces.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dimi_exec::{ShellSpec, SpawnOptions, spawn};

use crate::events::ToolUpdate;
use crate::llm::LlmClient;

/// Default/max foreground timeout, seconds (bash.ts constants).
pub const DEFAULT_TIMEOUT_S: u64 = 60;
pub const MAX_TIMEOUT_S: u64 = 300;

/// Truncation constants (result-builder.ts).
pub const DEFAULT_MAX_CHARS: usize = 50_000;
pub const DEFAULT_MAX_LINE_LENGTH: usize = 2_000;

/// A tool call the engine is asked to execute.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool execution context.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub cwd: String,
    pub shell: String,
}

/// Tool result fed back to the LLM (and emitted as `tool.result`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub output: String,
    pub is_error: bool,
    /// `stopTurn` — a backgrounded/failed-special result ends the turn.
    pub stop_turn: bool,
    /// Streaming updates emitted before the result (stdout/stderr chunks).
    pub updates: Vec<ToolUpdate>,
}

/// Tool effect boundary.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute one tool call. Async so tools can run nested turns
    /// (subagents), MCP servers and other long-running work.
    async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult;
}

/// Output buffer mirroring `ToolResultBuilder`: 50k total chars, 2k per
/// line, one `[...truncated]` marker, plus the truncation notice.
#[derive(Debug, Clone)]
pub struct OutputBuffer {
    pub text: String,
    pub truncated: bool,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            truncated: false,
        }
    }

    /// Append one chunk (utf8-decoded); enforces the char/line limits.
    pub fn push(&mut self, chunk: &str) {
        if self.truncated && self.text.len() >= DEFAULT_MAX_CHARS {
            return;
        }
        for ch in chunk.chars() {
            if self.text.len() >= DEFAULT_MAX_CHARS {
                if !self.truncated {
                    self.text.push_str("[...truncated]");
                    self.truncated = true;
                }
                return;
            }
            if ch == '\n' {
                self.text.push(ch);
                continue;
            }
            // Single-line cap: track the current line length.
            let line_start = self.text.rfind('\n').map_or(0, |idx| idx + 1);
            if self.text.len() - line_start >= DEFAULT_MAX_LINE_LENGTH {
                if !self.truncated {
                    self.text.push_str("[...truncated]");
                    self.truncated = true;
                }
                continue;
            }
            self.text.push(ch);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Bash tool over dimi-exec. Foreground path only (slice 1; backgrounding
/// lands with the task domain in a later slice).
#[derive(Debug, Clone, Default)]
pub struct BashTool;

impl BashTool {
    /// Validate the Bash arguments (`command` required non-empty; `cwd`
    /// string; `timeout` positive integer capped at MAX_TIMEOUT_S).
    pub fn validate_args(args: &serde_json::Value) -> Result<BashArgs, String> {
        let obj = args.as_object().ok_or("arguments must be an object")?;
        let command = obj
            .get("command")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("command is required")?;
        let cwd = obj.get("cwd").and_then(|v| v.as_str()).map(str::to_owned);
        let timeout = match obj.get("timeout") {
            None | Some(serde_json::Value::Null) => DEFAULT_TIMEOUT_S,
            Some(v) => {
                let n = v.as_u64().ok_or("timeout must be a positive integer")?;
                if n == 0 {
                    return Err("timeout must be a positive integer".to_string());
                }
                n.min(MAX_TIMEOUT_S)
            }
        };
        Ok(BashArgs {
            command: command.to_owned(),
            cwd,
            timeout,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BashArgs {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout: u64,
}

#[async_trait::async_trait]
impl ToolExecutor for BashTool {
    async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult {
        let args = match BashTool::validate_args(&call.arguments) {
            Ok(args) => args,
            Err(message) => {
                return ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: format!("Invalid args for tool \"{}\": {}", call.name, message),
                    is_error: true,
                    stop_turn: false,
                    updates: vec![],
                };
            }
        };

        let cwd = args.cwd.as_deref().unwrap_or(&ctx.cwd);
        let script = format!("cd '{}' && {}", cwd, args.command);
        // TS bashTool builds noninteractiveEnv over process.env; dimi-exec's
        // `env: Some` is a complete replacement, so inherit and overlay.
        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.insert("NO_COLOR".to_string(), "1".to_string());
        env.insert("TERM".to_string(), "dumb".to_string());
        env.insert(
            "GIT_TERMINAL_PROMPT".to_string(),
            std::env::var("GIT_TERMINAL_PROMPT").unwrap_or_else(|_| "0".to_string()),
        );
        env.insert("SHELL".to_string(), ctx.shell.clone());

        let process = match spawn(
            &ctx.shell,
            &["-c".to_string(), script],
            &SpawnOptions {
                cwd: Some(cwd.to_owned()),
                env: Some(env),
                shell: Some(ShellSpec::Explicit(ctx.shell.clone())),
                detached: Some(true),
                windows_hide: true,
            },
        ) {
            Ok(process) => process,
            Err(error) => {
                return ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: format!("Tool \"{}\" failed: {}", call.name, error.message),
                    is_error: true,
                    stop_turn: false,
                    updates: vec![],
                };
            }
        };

        // Drain stdout/stderr until EOF or timeout.
        let started = Instant::now();
        let timeout = Duration::from_secs(args.timeout);
        let mut stdout = OutputBuffer::new();
        let mut stderr = OutputBuffer::new();
        let mut updates = Vec::new();
        let mut timed_out = false;

        loop {
            let mut any = false;
            while let Some(chunk) = process.try_recv_stdout() {
                any = true;
                let text = String::from_utf8_lossy(&chunk);
                stdout.push(&text);
                updates.push(ToolUpdate::Stdout {
                    text: text.to_string(),
                });
            }
            while let Some(chunk) = process.try_recv_stderr() {
                any = true;
                let text = String::from_utf8_lossy(&chunk);
                stderr.push(&text);
                updates.push(ToolUpdate::Stderr {
                    text: text.to_string(),
                });
            }
            if !any {
                // Check exit without consuming the exit code: wait() blocks,
                // so poll via try_recv semantics through the exit flag.
                if process.exit_code().is_some() {
                    break;
                }
                if started.elapsed() > timeout {
                    timed_out = true;
                    let _ = process.kill(Some(9)); // SIGKILL
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = process.wait();

        if timed_out {
            return ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: format!("Command killed by timeout ({}s)", args.timeout),
                is_error: true,
                stop_turn: false,
                updates,
            };
        }

        let exit_code = process.exit_code();
        let output = build_foreground_output(stdout, stderr, exit_code);
        ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            output,
            is_error: exit_code != Some(0),
            stop_turn: false,
            updates,
        }
    }
}

/// Foreground result string assembly — priority: timeout > killed >
/// exitCode==0 > exitCode≠0 (bashTool.foregroundCompletionResult).
fn build_foreground_output(
    stdout: OutputBuffer,
    stderr: OutputBuffer,
    exit_code: Option<i32>,
) -> String {
    let mut combined = stdout.text.clone();
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&stderr.text);
    }
    if combined.is_empty() {
        match exit_code {
            Some(0) => return "Command executed successfully.".to_string(),
            Some(code) => {
                return format!(
                    "Process exited with code {}\nCommand failed with exit code: {}.",
                    code, code
                );
            }
            None => return "Process exited with an unknown status".to_string(),
        }
    }
    match exit_code {
        Some(0) => combined,
        Some(code) => {
            combined.push_str(&format!("\nCommand failed with exit code: {}.", code));
            combined
        }
        None => combined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_buffer_truncates_total_and_lines() {
        let mut buf = OutputBuffer::new();
        buf.push(&"a".repeat(DEFAULT_MAX_CHARS + 100));
        assert!(buf.truncated);
        assert!(buf.text.contains("[...truncated]"));
        assert!(buf.text.len() <= DEFAULT_MAX_CHARS + "[...truncated]".len());
    }

    #[test]
    fn output_buffer_caps_long_lines() {
        let mut buf = OutputBuffer::new();
        buf.push(&"x".repeat(DEFAULT_MAX_LINE_LENGTH + 50));
        assert!(buf.truncated);
        // The cap applies at the first over-limit char; marker fits.
        assert!(buf.text.contains("[...truncated]"));
    }

    #[test]
    fn validate_args_requires_command() {
        assert!(BashTool::validate_args(&serde_json::json!({})).is_err());
        assert!(BashTool::validate_args(&serde_json::json!({"command": ""})).is_err());
        assert!(BashTool::validate_args(&serde_json::json!({"command": "ls"})).is_ok());
    }

    #[test]
    fn validate_args_caps_timeout() {
        let args = BashTool::validate_args(&serde_json::json!({"command": "ls", "timeout": 99999}))
            .unwrap();
        assert_eq!(args.timeout, MAX_TIMEOUT_S);
        assert!(
            BashTool::validate_args(&serde_json::json!({"command": "ls", "timeout": 0})).is_err()
        );
    }

    #[tokio::test]
    async fn bash_tool_runs_a_real_command() {
        let tool = BashTool;
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "echo hello"}),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert!(result.output.contains("hello"), "output: {}", result.output);
    }

    #[tokio::test]
    async fn bash_tool_reports_nonzero_exit() {
        let tool = BashTool;
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_2".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "exit 3"}),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(result.is_error);
        assert!(
            result.output.contains("Command failed with exit code: 3."),
            "{}",
            result.output
        );
    }

    #[tokio::test]
    async fn bash_tool_timeout_kills() {
        let tool = BashTool;
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_3".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "sleep 30", "timeout": 1}),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(result.is_error);
        assert!(
            result.output.contains("Command killed by timeout"),
            "{}",
            result.output
        );
    }
}

/// `AgentTool` — the Agent tool: runs a nested turn (subagent) and returns
/// its final text as the tool result. Slice 4a: synchronous nested turn;
/// the async task semantics (agent_id + AgentOutput/WaitFor) land in 4c.
pub struct AgentTool {
    pub llm: Box<dyn crate::llm::LlmClient>,
    pub policy: crate::permission::PolicyConfig,
    pub max_steps: Option<u32>,
    pub shell: String,
}

impl std::fmt::Debug for AgentTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentTool")
            .field("max_steps", &self.max_steps)
            .field("shell", &self.shell)
            .finish()
    }
}

#[async_trait::async_trait]
impl ToolExecutor for AgentTool {
    async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult {
        // args: { prompt: string, ... }
        let prompt = call
            .arguments
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if prompt.trim().is_empty() {
            return ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: "Invalid args for tool \"Agent\": prompt is required".to_string(),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            };
        }
        let input = crate::types::EngineTurnInput {
            turn_id: 0,
            messages: vec![crate::types::LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning: None,
            }],
            tools: vec![],
            provider: crate::types::ProviderConfig {
                base_url: "http://nested".to_string(),
                api_key: "nested".to_string(),
                model: "nested".to_string(),
                thinking_effort: None,
            },
            max_steps_per_turn: self.max_steps,
            cwd: Some(ctx.cwd.clone()),
            shell: Some(ctx.shell.clone()),
            context_window: None,
            max_context_tokens: None,
        };
        let mut session = crate::engine::TurnSession::new(input);
        let mut events: Vec<crate::events::EngineEvent> = Vec::new();
        let progress = session
            .run(self.llm.as_ref(), self, &self.policy, &mut |event| {
                events.push(event);
            })
            .await;
        // The subagent's final assistant text is the tool result.
        let text: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                crate::events::EngineEvent::AssistantDelta { delta, .. } => Some(delta.clone()),
                _ => None,
            })
            .collect();
        let output = text.join("");
        match progress {
            crate::engine::TurnProgress::Completed(outcome)
                if outcome.status == crate::types::TurnEndReason::Completed =>
            {
                ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: if output.is_empty() {
                        "Subagent completed with no text output.".to_string()
                    } else {
                        output
                    },
                    is_error: false,
                    stop_turn: false,
                    updates: vec![],
                }
            }
            crate::engine::TurnProgress::Completed(outcome) => ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: outcome
                    .error
                    .clone()
                    .unwrap_or_else(|| "Subagent failed.".to_string()),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            },
            crate::engine::TurnProgress::NeedsApproval(_) => ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: "Subagent paused for approval (not supported in nested turns yet)."
                    .to_string(),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            },
        }
    }
}

#[cfg(test)]
mod agent_tool_tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, ScriptedLlmClient};

    fn policy_auto() -> crate::permission::PolicyConfig {
        crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        }
    }

    #[tokio::test]
    async fn agent_runs_a_nested_turn_and_returns_its_text() {
        // The subagent's LLM answers directly (no tools).
        let sub_llm = ScriptedLlmClient::once(vec![
            LlmStreamEvent::Text {
                delta: "subagent answer".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]);
        let agent = AgentTool {
            llm: Box::new(sub_llm),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
        };
        let result = agent
            .execute(
                &ToolCall {
                    id: "call_agent".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "do a thing" }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert_eq!(result.output, "subagent answer");
    }

    #[tokio::test]
    async fn agent_nested_turn_can_use_bash() {
        // The subagent asks for a Bash call, then answers.
        let sub_llm = ScriptedLlmClient::new(vec![
            vec![
                LlmStreamEvent::ToolCall {
                    tool_call_id: "nested_call".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo nested-ok\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![
                LlmStreamEvent::Text {
                    delta: "ran it: nested-ok".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ]);
        let agent = AgentTool {
            llm: Box::new(sub_llm),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
        };
        let result = agent
            .execute(
                &ToolCall {
                    id: "call_agent".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "run bash" }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert!(result.output.contains("ran it"));
    }

    #[tokio::test]
    async fn agent_requires_prompt() {
        let agent = AgentTool {
            llm: Box::new(ScriptedLlmClient::once(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
        };
        let result = agent
            .execute(
                &ToolCall {
                    id: "call_agent".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({}),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(result.is_error);
        assert!(result.output.contains("prompt is required"));
    }
}

/// `ToolRegistry` — routes tool calls by name to registered executors
/// (the engine's tool set: Bash + Agent + AgentOutput + WaitFor + …) and
/// carries the tool definitions handed to the LLM.
#[derive(Default)]
pub struct ToolRegistry {
    tools: std::collections::HashMap<String, Box<dyn ToolExecutor>>,
    defs: std::collections::HashMap<String, serde_json::Value>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.names())
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, tool: Box<dyn ToolExecutor>) {
        self.register_with_def(name, tool, None);
    }

    /// Register a tool with its LLM-facing definition
    /// (`{name, description, parameters}` — OpenAI `tools` entry).
    pub fn register_with_def(
        &mut self,
        name: impl Into<String>,
        tool: Box<dyn ToolExecutor>,
        def: Option<serde_json::Value>,
    ) {
        let name = name.into();
        if let Some(def) = def {
            self.defs.insert(name.clone(), def);
        }
        self.tools.insert(name, tool);
    }

    /// The tool definitions to send to the LLM (`tools` request field).
    pub fn tool_defs(&self) -> Vec<serde_json::Value> {
        let mut defs: Vec<serde_json::Value> = self.defs.values().cloned().collect();
        defs.sort_by(|a, b| {
            a.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .cmp(
                    b.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or(""),
                )
        });
        defs
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }
}

#[async_trait::async_trait]
impl ToolExecutor for ToolRegistry {
    async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult {
        match self.tools.get(&call.name) {
            Some(tool) => tool.execute(call, ctx).await,
            None => ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: format!("Tool \"{}\" not found", call.name),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            },
        }
    }
}

/// Shared subagent task registry (slice 4c async semantics).
#[derive(Debug, Clone, Default)]
pub struct AgentTasks {
    inner: Arc<std::sync::Mutex<std::collections::HashMap<String, TaskState>>>,
}

/// One background subagent task.
#[derive(Debug, Clone)]
pub struct TaskState {
    pub agent_id: String,
    pub status: String, // running | completed | failed
    pub output: String,
    pub error: Option<String>,
}

impl AgentTasks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, task_id: String, state: TaskState) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.insert(task_id, state);
    }

    pub fn update<F>(&self, task_id: &str, f: F)
    where
        F: FnOnce(&mut TaskState),
    {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = inner.get_mut(task_id) {
            f(state);
        }
    }

    pub fn get(&self, task_id: &str) -> Option<TaskState> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.get(task_id).cloned()
    }

    /// Look up by agent id (the wire key the tools receive).
    pub fn find_by_agent_id(&self, agent_id: &str) -> Option<TaskState> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner
            .values()
            .find(|state| state.agent_id == agent_id)
            .cloned()
    }
}

/// `AgentTool` (async) — launches the subagent in the background and returns
/// immediately with `agent_id` / `task_id`; `AgentOutput` and `WaitFor` read
/// the task state. Mirrors the async-subagent semantics on main.
pub struct AsyncAgentTool {
    pub llm: Arc<dyn LlmClient>,
    pub tools: Arc<dyn ToolExecutor>,
    pub policy: crate::permission::PolicyConfig,
    pub max_steps: Option<u32>,
    pub shell: String,
    pub tasks: AgentTasks,
    /// agent_id → steering queue (shared with the napi session).
    pub steer_map: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<std::sync::Mutex<Vec<crate::types::LlmMessage>>>>>>,
}

impl std::fmt::Debug for AsyncAgentTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncAgentTool")
            .field("max_steps", &self.max_steps)
            .finish()
    }
}

async fn run_nested_turn(
    prompt: String,
    cwd: String,
    shell: String,
    llm: Arc<dyn LlmClient>,
    tools: Arc<dyn ToolExecutor>,
    policy: crate::permission::PolicyConfig,
    max_steps: Option<u32>,
    steer: Option<Arc<std::sync::Mutex<Vec<crate::types::LlmMessage>>>>,
) -> (String, String) {
    let input = crate::types::EngineTurnInput {
        turn_id: 0,
        messages: vec![crate::types::LlmMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(prompt),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning: None,
        }],
        tools: vec![],
        provider: crate::types::ProviderConfig {
            base_url: "http://nested".to_string(),
            api_key: "nested".to_string(),
            model: "nested".to_string(),
            thinking_effort: None,
        },
        max_steps_per_turn: max_steps,
        cwd: Some(cwd),
        shell: Some(shell),
        context_window: None,
        max_context_tokens: None,
    };
    let mut session = crate::engine::TurnSession::with_steer(input, steer);
    let mut events: Vec<crate::events::EngineEvent> = Vec::new();
    let progress = session
        .run(llm.as_ref(), tools.as_ref(), &policy, &mut |event| {
            events.push(event);
        })
        .await;
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            crate::events::EngineEvent::AssistantDelta { delta, .. } => Some(delta.clone()),
            _ => None,
        })
        .collect();
    match progress {
        crate::engine::TurnProgress::Completed(outcome)
            if outcome.status == crate::types::TurnEndReason::Completed =>
        {
            (text, String::new())
        }
        crate::engine::TurnProgress::Completed(outcome) => (
            String::new(),
            outcome.error.unwrap_or_else(|| "failed".to_string()),
        ),
        crate::engine::TurnProgress::NeedsApproval(_) => {
            (String::new(), "approval pending in nested turn".to_string())
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for AsyncAgentTool {
    async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult {
        let prompt = call
            .arguments
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if prompt.trim().is_empty() {
            return ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: "Invalid args for tool \"Agent\": prompt is required".to_string(),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            };
        }
        let description = call
            .arguments
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Running subagent")
            .to_string();
        let agent_id = format!("agent-{}", uuid_v4_short());
        let task_id = format!("task-{}", uuid_v4_short());
        self.tasks.insert(
            task_id.clone(),
            TaskState {
                agent_id: agent_id.clone(),
                status: "running".to_string(),
                output: String::new(),
                error: None,
            },
        );

        let tasks = self.tasks.clone();
        let task_id_for_worker = task_id.clone();
        let prompt_for_worker = prompt;
        let cwd = ctx.cwd.clone();
        let shell = self.shell.clone();
        let llm = Arc::clone(&self.llm);
        let tools = Arc::clone(&self.tools);
        let policy = self.policy.clone();
        let max_steps = self.max_steps;
        let steer_queue: Arc<std::sync::Mutex<Vec<crate::types::LlmMessage>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        self.steer_map
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(agent_id.clone(), Arc::clone(&steer_queue));
        tokio::spawn(async move {
            let (output, error) = run_nested_turn(
                prompt_for_worker,
                cwd,
                shell,
                llm,
                tools,
                policy,
                max_steps,
                Some(steer_queue),
            )
            .await;
            tasks.update(&task_id_for_worker, |state| {
                if error.is_empty() {
                    state.status = "completed".to_string();
                    state.output = output;
                } else {
                    state.status = "failed".to_string();
                    state.error = Some(error);
                }
            });
        });

        ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            output: serde_json::to_string(&serde_json::json!({
                "agent_id": agent_id,
                "task_id": task_id,
                "description": description,
                "status": "running",
                "automatic_notification": true,
                "next_step": "Use WaitFor or AgentOutput to check on the subagent.",
            }))
            .unwrap_or_default(),
            is_error: false,
            stop_turn: true, // the turn yields while the subagent runs
            updates: vec![],
        }
    }
}

fn uuid_v4_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// `AgentOutputTool` — read a background subagent's current output/status.
pub struct AgentOutputTool {
    pub tasks: AgentTasks,
}

#[async_trait::async_trait]
impl ToolExecutor for AgentOutputTool {
    async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
        let agent_id = call
            .arguments
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let state = self
            .tasks
            .get(&agent_id)
            .or_else(|| self.tasks.find_by_agent_id(&agent_id));
        match state {
            Some(state) => ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: serde_json::to_string(&serde_json::json!({
                    "agent_id": state.agent_id,
                    "status": state.status,
                    "output": state.output,
                    "error": state.error,
                }))
                .unwrap_or_default(),
                is_error: false,
                stop_turn: false,
                updates: vec![],
            },
            None => ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: format!("No subagent found for agent_id: {agent_id}"),
                is_error: true,
                stop_turn: false,
                updates: vec![],
            },
        }
    }
}

/// `WaitForTool` — wait for a background subagent to finish (or timeout).
pub struct WaitForTool {
    pub tasks: AgentTasks,
}

#[async_trait::async_trait]
impl ToolExecutor for WaitForTool {
    async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
        let agent_id = call
            .arguments
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timeout = call
            .arguments
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(60);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
        loop {
            if let Some(state) = self
                .tasks
                .get(&agent_id)
                .or_else(|| self.tasks.find_by_agent_id(&agent_id))
            {
                if state.status != "running" {
                    return ToolResult {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        output: serde_json::to_string(&serde_json::json!({
                            "agent_id": state.agent_id,
                            "status": state.status,
                            "output": state.output,
                            "error": state.error,
                        }))
                        .unwrap_or_default(),
                        is_error: false,
                        stop_turn: false,
                        updates: vec![],
                    };
                }
            }
            if std::time::Instant::now() >= deadline {
                return ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: format!("Wait expired for agent_id: {agent_id}"),
                    is_error: false,
                    stop_turn: false,
                    updates: vec![],
                };
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

#[cfg(test)]
mod async_agent_tests {
    use super::*;
    use crate::llm::{LlmStreamEvent, ScriptedLlmClient};

    fn policy_auto() -> crate::permission::PolicyConfig {
        crate::permission::PolicyConfig {
            mode: crate::permission::PermissionMode::Auto,
            rules: vec![],
            session_approved_patterns: vec![],
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            shell: "/bin/sh".to_string(),
        }
    }

    #[tokio::test]
    async fn agent_returns_immediately_and_task_completes() {
        let sub_llm = Arc::new(ScriptedLlmClient::once(vec![
            LlmStreamEvent::Text {
                delta: "background result".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]));
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: sub_llm,
            tools: Arc::new(BashTool),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "background work" }),
                },
                &ctx(),
            )
            .await;
        assert!(!launch.is_error);
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();
        assert_eq!(launch_json["status"], "running");
        assert!(launch.stop_turn);

        // WaitFor polls until the background turn completes.
        let wait = WaitForTool {
            tasks: tasks.clone(),
        };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id, "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(waited_json["status"], "completed");
        assert_eq!(waited_json["output"], "background result");
    }

    #[tokio::test]
    async fn agent_output_reads_running_task() {
        let sub_llm = Arc::new(ScriptedLlmClient::once(vec![
            LlmStreamEvent::Text {
                delta: "partial".to_string(),
            },
            LlmStreamEvent::Finish {
                finish_reason: Some("stop".to_string()),
            },
        ]));
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: sub_llm,
            tools: Arc::new(BashTool),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "bg" }),
                },
                &ctx(),
            )
            .await;
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();
        // Give the background turn a moment, then read its output.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let output = AgentOutputTool { tasks };
        let read = output
            .execute(
                &ToolCall {
                    id: "call_o".to_string(),
                    name: "AgentOutput".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id }),
                },
                &ctx(),
            )
            .await;
        assert!(!read.is_error, "output: {}", read.output);
        let read_json: serde_json::Value = serde_json::from_str(&read.output).unwrap();
        assert_eq!(read_json["status"], "completed");
        assert_eq!(read_json["output"], "partial");
    }

    #[tokio::test]
    async fn waitfor_times_out_for_missing_task() {
        let tasks = AgentTasks::new();
        let wait = WaitForTool { tasks };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": "agent-missing", "timeout_seconds": 1 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error);
        assert!(waited.output.contains("Wait expired"));
    }
}
