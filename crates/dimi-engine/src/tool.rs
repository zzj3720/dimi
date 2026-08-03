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
/// Default deadline for a command moved to the background by a foreground
/// timeout (bash.ts `DEFAULT_BACKGROUND_TIMEOUT_S`).
pub const DEFAULT_BACKGROUND_TIMEOUT_S: u64 = 600;

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

    /// Best-effort cancellation of an in-flight call (turn cancel). The
    /// default is a no-op; tools that own processes kill them here.
    fn abort(&self, _call: &ToolCall) {}
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

/// Bash tool over dimi-exec. Mirrors `bashTool.ts` for the foreground path:
/// schema validation, `sh -c "cd '<cwd>' && <command>"`, the noninteractive
/// env overlay, 50k/2k truncation, 60s/300s timeouts and the exact result
/// strings the TS implementation produces. On a foreground timeout the
/// command is moved to the background instead of being killed
/// (`bashAutoBackgroundOnTimeout` default true): it is registered in
/// `tasks` (the same registry AgentOutput/WaitFor read) and a spawned
/// poller keeps draining it until it exits, so the session can observe the
/// eventual completion. `current` holds the running process so `abort`
/// (turn cancellation) can kill it; backgrounded processes survive the tool
/// future being dropped.
pub struct BashTool {
    current: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dimi_exec::ExecProcess>>>>,
    /// Shared task registry (AgentOutput/WaitFor read the same `AgentTasks`).
    /// Backgrounded-on-timeout commands register here with a `bash-<id>` key.
    tasks: AgentTasks,
    /// TS `bashAutoBackgroundOnTimeout` config (default true): when false a
    /// foreground timeout SIGKILLs the command instead of backgrounding it.
    auto_background_on_timeout: bool,
}

impl BashTool {
    /// A Bash tool whose backgrounded-on-timeout tasks land in `tasks` —
    /// the registry the session's AgentOutput/WaitFor tools read.
    pub fn with_tasks(tasks: AgentTasks) -> Self {
        Self {
            tasks,
            ..Self::default()
        }
    }

    /// A Bash tool that SIGKILLs on timeout instead of backgrounding — the
    /// TS behavior when `bashAutoBackgroundOnTimeout` is false or the Task
    /// tools are not active (no registry can surface the background task).
    pub fn kill_on_timeout() -> Self {
        Self {
            auto_background_on_timeout: false,
            ..Self::default()
        }
    }
}

impl std::fmt::Debug for BashTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BashTool")
            .field(
                "running",
                &self
                    .current
                    .lock()
                    .map(|guard| guard.is_some())
                    .unwrap_or(false),
            )
            .field(
                "auto_background_on_timeout",
                &self.auto_background_on_timeout,
            )
            .finish()
    }
}

impl Clone for BashTool {
    fn clone(&self) -> Self {
        Self {
            current: std::sync::Arc::clone(&self.current),
            tasks: self.tasks.clone(),
            auto_background_on_timeout: self.auto_background_on_timeout,
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self {
            current: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tasks: AgentTasks::new(),
            auto_background_on_timeout: true,
        }
    }
}

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
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
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
            description,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BashArgs {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout: u64,
    /// Optional task description (`description` arg; falls back to the
    /// TS `foregroundDescription` preview for the task metadata).
    pub description: Option<String>,
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
            Ok(process) => std::sync::Arc::new(process),
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
        *self
            .current
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(process.clone());

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
                    break;
                }
                // Yield to the runtime so cooperative cancellation (the
                // run loop's tokio::select) can fire while the command runs.
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        if timed_out && self.auto_background_on_timeout {
            // TS parity (`bashAutoBackgroundOnTimeout` default true): the
            // command is moved to the background instead of being killed.
            // The process handle is Arc-cloned into a spawned poller, so it
            // survives this future being dropped; `current` is cleared so a
            // later abort (turn cancellation) leaves the background task
            // alone. The task is registered in `tasks` for AgentOutput /
            // WaitFor to read.
            *self.current.lock().unwrap_or_else(|p| p.into_inner()) = None;
            return backgrounded_result(
                &self.tasks,
                call,
                &process,
                &args,
                stdout,
                stderr,
                updates,
            );
        }

        if timed_out {
            let _ = process.kill(Some(9)); // SIGKILL
        }
        let _ = process.wait();
        *self
            .current
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;

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

    /// Kill the running command (turn cancellation): SIGKILL the process
    /// tree, mirroring the TS bashTool's cancel path.
    fn abort(&self, _call: &ToolCall) {
        if let Some(process) = self
            .current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            let _ = process.kill(Some(9));
        }
    }
}

/// `bashTool.backgroundStartedResult(..., 'foreground_detached')` for the
/// timeout path: the command keeps running as a registered background task.
/// The result reports the timeout, is NOT an error (TS `isError: false`),
/// and hands the model the `task_id` that AgentOutput/WaitFor accept.
fn backgrounded_result(
    tasks: &AgentTasks,
    call: &ToolCall,
    process: &Arc<dimi_exec::ExecProcess>,
    args: &BashArgs,
    stdout: OutputBuffer,
    stderr: OutputBuffer,
    updates: Vec<ToolUpdate>,
) -> ToolResult {
    let task_id = format!("bash-{}", uuid_v4_short());
    let description = match &args.description {
        Some(description) if !description.trim().is_empty() => description.clone(),
        _ => format!("Bash: {}", command_preview(&args.command)),
    };
    // The foreground output collected before the timeout (TS appends it
    // under `foreground_output:`; the exit-code suffix is unknown because
    // the command is still running).
    let mut foreground_output = stdout.text.clone();
    if !stderr.is_empty() {
        if !foreground_output.is_empty() {
            foreground_output.push('\n');
        }
        foreground_output.push_str(&stderr.text);
    }

    tasks.insert(
        task_id.clone(),
        TaskState {
            agent_id: task_id.clone(),
            status: "running".to_string(),
            output: foreground_output.clone(),
            error: None,
            messages: vec![],
            started_at: now_nanos(),
        },
    );

    // Detached poller: the process handle is Arc-cloned, so the task
    // survives this tool future being dropped (the spawn is `detached`).
    let tasks = tasks.clone();
    let worker_task_id = task_id.clone();
    let poller_process = Arc::clone(process);
    tokio::spawn(async move {
        // Mirror the foreground drain loop: keep draining until a quiet
        // pass observes the exit flag (or the background deadline fires).
        let started = Instant::now();
        let background_timeout = Duration::from_secs(DEFAULT_BACKGROUND_TIMEOUT_S);
        loop {
            let mut any = false;
            while let Some(chunk) = poller_process.try_recv_stdout() {
                any = true;
                tasks.update(&worker_task_id, |state| append_task_output(state, &chunk));
            }
            while let Some(chunk) = poller_process.try_recv_stderr() {
                any = true;
                tasks.update(&worker_task_id, |state| append_task_output(state, &chunk));
            }
            if !any {
                if poller_process.exit_code().is_some() {
                    break;
                }
                if started.elapsed() > background_timeout {
                    // TS `detachTimeoutMs` (bashTaskTimeoutS, default 600s):
                    // a backgrounded command that overruns its own deadline
                    // is killed.
                    let _ = poller_process.kill(Some(9));
                    let _ = poller_process.wait();
                    tasks.update(&worker_task_id, |state| {
                        state.status = "timed_out".to_string();
                        state.error = Some(format!(
                            "Command killed by timeout ({}s)",
                            DEFAULT_BACKGROUND_TIMEOUT_S
                        ));
                    });
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let _ = poller_process.wait();
        // Grace drain (TS `waitForStreamDrain`, 250ms): the waiter thread
        // can set the exit flag before the reader threads flush the final
        // pipe chunks, so drain a little longer to capture the tail.
        let drain_deadline = Instant::now() + Duration::from_millis(250);
        loop {
            let mut any = false;
            while let Some(chunk) = poller_process.try_recv_stdout() {
                any = true;
                tasks.update(&worker_task_id, |state| append_task_output(state, &chunk));
            }
            while let Some(chunk) = poller_process.try_recv_stderr() {
                any = true;
                tasks.update(&worker_task_id, |state| append_task_output(state, &chunk));
            }
            if !any || Instant::now() >= drain_deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let code = poller_process.exit_code();
        // TS ProcessTask settle: exit 0 → completed, otherwise failed.
        tasks.update(&worker_task_id, |state| {
            if code == Some(0) {
                state.status = "completed".to_string();
            } else {
                state.status = "failed".to_string();
                state.error = Some(format!("Process exited with code {}", code.unwrap_or(-1)));
            }
        });
    });

    let metadata = format!(
        "Command timed out ({}s); it is still running in the background.\n\
         task_id: {task_id}\n\
         pid: {}\n\
         description: {description}\n\
         status: running\n\
         automatic_notification: false\n\
         next_step: The task now runs in the background; read its output/status with \
         AgentOutput(agent_id=\"{task_id}\") or WaitFor(agent_id=\"{task_id}\") once it \
         completes, and continue with your current work.",
        args.timeout,
        process.pid(),
    );
    let output = if foreground_output.is_empty() {
        metadata
    } else {
        format!("{metadata}\n\nforeground_output:\n{foreground_output}")
    };
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        output,
        is_error: false,
        stop_turn: false,
        updates,
    }
}

/// Append a decoded output chunk to the task state, capped at the
/// result-builder char limit (mirrors the TS task service's retained output
/// cap so a chatty background command cannot grow unbounded).
fn append_task_output(state: &mut TaskState, chunk: &[u8]) {
    if state.output.len() >= DEFAULT_MAX_CHARS {
        return;
    }
    let text = String::from_utf8_lossy(chunk);
    let remaining = DEFAULT_MAX_CHARS - state.output.len();
    state
        .output
        .push_str(&text.chars().take(remaining).collect::<String>());
}

/// TS `foregroundDescription` preview: the command truncated at 60 chars.
fn command_preview(command: &str) -> String {
    let chars: Vec<char> = command.chars().collect();
    if chars.len() <= 60 {
        return command.to_string();
    }
    let preview: String = chars[..60].iter().collect();
    format!("{preview}…")
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
        let tool = BashTool::default();
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
        let tool = BashTool::default();
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
    async fn bash_tool_timeout_kills_when_auto_background_disabled() {
        // `bashAutoBackgroundOnTimeout: false` — the kill path.
        let tool = BashTool {
            auto_background_on_timeout: false,
            ..BashTool::default()
        };
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

    #[tokio::test]
    async fn bash_tool_timeout_backgrounds_and_task_completes() {
        // Default (`bashAutoBackgroundOnTimeout` true): a command that
        // outlives its timeout is moved to the background, NOT killed. The
        // result reports the timeout without is_error, hands over a task
        // handle, and AgentOutput/WaitFor later read the outcome.
        let tasks = AgentTasks::new();
        let tool = BashTool::with_tasks(tasks.clone());
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "sleep 4; echo bg-done", "timeout": 1}),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert!(
            result
                .output
                .contains("Command timed out (1s); it is still running in the background."),
            "{}",
            result.output
        );
        let task_id = result
            .output
            .lines()
            .find_map(|line| line.strip_prefix("task_id: "))
            .map(str::trim)
            .expect("task_id in result output")
            .to_string();
        assert!(task_id.starts_with("bash-"), "{task_id}");
        assert!(
            result.output.contains("status: running"),
            "{}",
            result.output
        );
        assert!(result.output.contains("AgentOutput"), "{}", result.output);
        assert!(result.output.contains("WaitFor"), "{}", result.output);

        // The task is registered and still running (sleep 4 >> 1s timeout).
        let state = tasks.get(&task_id).expect("backgrounded task registered");
        assert_eq!(state.status, "running");

        // WaitFor polls until the background command completes.
        let waited = WaitForTool {
            tasks: tasks.clone(),
        }
        .execute(
            &ToolCall {
                id: "call_w".to_string(),
                name: "WaitFor".to_string(),
                arguments: serde_json::json!({ "agent_id": task_id, "timeout_seconds": 20 }),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
            },
        )
        .await;
        assert!(!waited.is_error, "output: {}", waited.output);
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(waited_json["status"], "completed");
        assert!(
            waited_json["output"].as_str().unwrap().contains("bg-done"),
            "{}",
            waited.output
        );

        // AgentOutput surfaces the same task state.
        let read = AgentOutputTool {
            tasks: tasks.clone(),
        }
        .execute(
            &ToolCall {
                id: "call_o".to_string(),
                name: "AgentOutput".to_string(),
                arguments: serde_json::json!({ "agent_id": task_id }),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
            },
        )
        .await;
        assert!(!read.is_error, "output: {}", read.output);
        let read_json: serde_json::Value = serde_json::from_str(&read.output).unwrap();
        assert_eq!(read_json["status"], "completed");
        assert!(read_json["output"].as_str().unwrap().contains("bg-done"));
    }

    #[tokio::test]
    async fn bash_tool_timeout_backgrounded_output_is_not_lost() {
        // The output the command produces AFTER the timeout must land in
        // the task state (the poller keeps draining), and a nonzero exit
        // settles the task as failed.
        let tasks = AgentTasks::new();
        let tool = BashTool::with_tasks(tasks.clone());
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg2".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        "command": "sleep 2; echo after-bg; exit 4",
                        "timeout": 1,
                        "description": "late output probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert!(
            result.output.contains("description: late output probe"),
            "{}",
            result.output
        );
        let task_id = result
            .output
            .lines()
            .find_map(|line| line.strip_prefix("task_id: "))
            .map(str::trim)
            .expect("task_id in result output")
            .to_string();

        let waited = WaitForTool {
            tasks: tasks.clone(),
        }
        .execute(
            &ToolCall {
                id: "call_w2".to_string(),
                name: "WaitFor".to_string(),
                arguments: serde_json::json!({ "agent_id": task_id, "timeout_seconds": 20 }),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
            },
        )
        .await;
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(waited_json["status"], "failed");
        assert!(
            waited_json["output"].as_str().unwrap().contains("after-bg"),
            "post-timeout output must be captured: {}",
            waited.output
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
    /// Full message history of the subagent's latest turn (resume carries
    /// it into the next turn).
    pub messages: Vec<crate::types::LlmMessage>,
    /// Monotonic launch timestamp (nanos) — disambiguates tasks that share
    /// an agent id (resume): the newest wins for lookups.
    pub started_at: u128,
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

    /// Look up by agent id (the wire key the tools receive). When multiple
    /// tasks share an agent id (resume spawns a new task), the running one
    /// wins; otherwise the first match is returned.
    pub fn find_by_agent_id(&self, agent_id: &str) -> Option<TaskState> {
        self.find_by_agent_id_with_key(agent_id).map(|(_, state)| state)
    }

    /// Look up by agent id, also returning the task id (map key). The
    /// running task wins; otherwise the newest (largest `started_at`).
    pub fn find_by_agent_id_with_key(&self, agent_id: &str) -> Option<(String, TaskState)> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut best: Option<(String, TaskState)> = None;
        for (task_id, state) in inner.iter() {
            if state.agent_id != agent_id {
                continue;
            }
            if state.status == "running" {
                return Some((task_id.clone(), state.clone()));
            }
            let is_newer = best
                .as_ref()
                .map(|(_, current)| state.started_at > current.started_at)
                .unwrap_or(true);
            if is_newer {
                best = Some((task_id.clone(), state.clone()));
            }
        }
        best
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
    history: Vec<crate::types::LlmMessage>,
    prompt: String,
    cwd: String,
    shell: String,
    llm: Arc<dyn LlmClient>,
    tools: Arc<dyn ToolExecutor>,
    policy: crate::permission::PolicyConfig,
    max_steps: Option<u32>,
    steer: Option<Arc<std::sync::Mutex<Vec<crate::types::LlmMessage>>>>,
) -> (String, String, Vec<crate::types::LlmMessage>) {
    let mut messages = history;
    messages.push(crate::types::LlmMessage {
        role: "user".to_string(),
        content: serde_json::Value::String(prompt),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning: None,
    });
    let input = crate::types::EngineTurnInput {
        turn_id: 0,
        messages,
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
    let messages = session.messages().to_vec();
    match progress {
        crate::engine::TurnProgress::Completed(outcome)
            if outcome.status == crate::types::TurnEndReason::Completed =>
        {
            (text, String::new(), messages)
        }
        crate::engine::TurnProgress::Completed(outcome) => (
            String::new(),
            outcome.error.unwrap_or_else(|| "failed".to_string()),
            messages,
        ),
        crate::engine::TurnProgress::NeedsApproval(_) => (
            String::new(),
            "approval pending in nested turn".to_string(),
            messages,
        ),
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
        let resume = call
            .arguments
            .get("resume")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        // `Agent(resume=agent_id, prompt=…)` (TS parity): the prompt steers
        // the subagent when its turn is still running, or starts a fresh
        // turn carrying its history when it is idle.
        if let Some(agent_id) = &resume {
            let Some((task_id, state)) = self.tasks.find_by_agent_id_with_key(agent_id) else {
                return ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: format!("No subagent with agent_id \"{agent_id}\" was found."),
                    is_error: true,
                    stop_turn: false,
                    updates: vec![],
                };
            };
            if state.status == "running" {
                let steered = self
                    .steer_map
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(agent_id)
                    .map(|queue| {
                        queue
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push(crate::types::LlmMessage {
                                role: "user".to_string(),
                                content: serde_json::Value::String(prompt),
                                name: None,
                                tool_call_id: None,
                                tool_calls: None,
                                reasoning: None,
                            })
                    })
                    .is_some();
                if !steered {
                    return ToolResult {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        output: format!(
                            "Subagent \"{agent_id}\" is running but has no steer queue."
                        ),
                        is_error: true,
                        stop_turn: false,
                        updates: vec![],
                    };
                }
                return ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: serde_json::to_string(&serde_json::json!({
                        "agent_id": agent_id,
                        "task_id": task_id,
                        "description": description,
                        "status": "running",
                        "steered": true,
                        "next_step": "Use WaitFor or AgentOutput to check on the subagent.",
                    }))
                    .unwrap_or_default(),
                    is_error: false,
                    stop_turn: true,
                    updates: vec![],
                };
            }
            // Idle: resume carries the subagent's history into the new turn.
            let history = state.messages.clone();
            return self.launch_subagent(
                call,
                ctx,
                prompt,
                description,
                agent_id.clone(),
                history,
            )
            .await;
        }

        let agent_id = format!("agent-{}", uuid_v4_short());
        self.launch_subagent(call, ctx, prompt, description, agent_id, Vec::new())
            .await
    }
}

impl AsyncAgentTool {
    /// Spawn the background nested turn and return the launch result.
    async fn launch_subagent(
        &self,
        call: &ToolCall,
        ctx: &ToolContext,
        prompt: String,
        description: String,
        agent_id: String,
        history: Vec<crate::types::LlmMessage>,
    ) -> ToolResult {
        let task_id = format!("task-{}", uuid_v4_short());
        self.tasks.insert(
            task_id.clone(),
            TaskState {
                agent_id: agent_id.clone(),
                status: "running".to_string(),
                output: String::new(),
                error: None,
                messages: history.clone(),
                started_at: now_nanos(),
            },
        );

        let tasks = self.tasks.clone();
        let task_id_for_worker = task_id.clone();
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
            let (output, error, messages) = run_nested_turn(
                history,
                prompt,
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
                state.messages = messages;
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

/// Monotonic launch timestamp for `TaskState` (nanos since the epoch).
fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
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
            tools: Arc::new(BashTool::default()),
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
            tools: Arc::new(BashTool::default()),
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

    #[tokio::test]
    async fn resume_steers_a_running_subagent() {
        let tasks = AgentTasks::new();
        tasks.insert(
            "task-running".to_string(),
            TaskState {
                agent_id: "agent-running".to_string(),
                status: "running".to_string(),
                output: String::new(),
                error: None,
                messages: vec![],
                started_at: 1,
            },
        );
        let steer_queue: Arc<std::sync::Mutex<Vec<crate::types::LlmMessage>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let steer_map: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<std::sync::Mutex<Vec<crate::types::LlmMessage>>>>>> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        steer_map
            .lock()
            .unwrap()
            .insert("agent-running".to_string(), Arc::clone(&steer_queue));
        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![])),
            tools: Arc::new(BashTool::default()),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map,
        };
        let result = agent
            .execute(
                &ToolCall {
                    id: "call_resume".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({
                        "resume": "agent-running",
                        "prompt": "go left instead",
                    }),
                },
                &ctx(),
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        let json: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(json["status"], "running");
        assert_eq!(json["steered"], true);
        assert_eq!(json["agent_id"], "agent-running");
        // The prompt landed in the running subagent's steer queue.
        let queued = steer_queue.lock().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].content, serde_json::Value::String("go left instead".to_string()));
    }

    #[tokio::test]
    async fn resume_idle_starts_a_new_turn_with_history() {
        let tasks = AgentTasks::new();
        tasks.insert(
            "task-done".to_string(),
            TaskState {
                agent_id: "agent-done".to_string(),
                status: "completed".to_string(),
                output: "old answer".to_string(),
                error: None,
                messages: vec![crate::types::LlmMessage {
                    role: "user".to_string(),
                    content: serde_json::Value::String("old".to_string()),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning: None,
                }],
                started_at: 1,
            },
        );
        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![
                LlmStreamEvent::Text {
                    delta: "continued answer".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ])),
            tools: Arc::new(BashTool::default()),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        };
        let result = agent
            .execute(
                &ToolCall {
                    id: "call_resume".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({
                        "resume": "agent-done",
                        "prompt": "continue",
                    }),
                },
                &ctx(),
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        let json: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(json["agent_id"], "agent-done");
        let task_id = json["task_id"].as_str().unwrap().to_string();

        // Wait for the resumed turn, then verify it carried the history.
        let wait = WaitForTool {
            tasks: tasks.clone(),
        };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": "agent-done", "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(waited_json["status"], "completed");
        assert_eq!(waited_json["output"], "continued answer");

        let state = tasks.get(&task_id).expect("resumed task exists");
        let history_texts: Vec<&str> = state
            .messages
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect();
        assert!(
            history_texts.contains(&"old"),
            "history must carry into the resumed turn: {history_texts:?}"
        );
        assert!(history_texts.contains(&"continue"));
    }

    #[tokio::test]
    async fn resume_unknown_agent_is_an_error() {
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![])),
            tools: Arc::new(BashTool::default()),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks,
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        };
        let result = agent
            .execute(
                &ToolCall {
                    id: "call_resume".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({
                        "resume": "agent-ghost",
                        "prompt": "hello",
                    }),
                },
                &ctx(),
            )
            .await;
        assert!(result.is_error);
        assert!(result.output.contains("agent-ghost"));
    }
}
