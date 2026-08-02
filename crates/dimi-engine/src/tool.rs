//! Tool effect boundary — slice 1 ships the Bash tool over dimi-exec.
//!
//! Behavior mirrors `agent/tools/os/bash/` (bashTool.ts + bash.ts) for the
//! foreground path: schema validation, `sh -c "cd '<cwd>' && <command>"`,
//! the noninteractive env overlay, 50k/2k truncation, 60s/300s timeouts and
//! the exact result strings the TS implementation produces.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use dimi_exec::{ShellSpec, SpawnOptions, spawn};

use crate::events::ToolUpdate;

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
#[derive(Debug, Clone, PartialEq)]
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
pub trait ToolExecutor {
    fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult;
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

impl ToolExecutor for BashTool {
    fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolResult {
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

    #[test]
    fn bash_tool_runs_a_real_command() {
        let tool = BashTool;
        let result = tool.execute(
            &ToolCall {
                id: "call_1".to_string(),
                name: "Bash".to_string(),
                arguments: serde_json::json!({"command": "echo hello"}),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
            },
        );
        assert!(!result.is_error, "output: {}", result.output);
        assert!(result.output.contains("hello"), "output: {}", result.output);
    }

    #[test]
    fn bash_tool_reports_nonzero_exit() {
        let tool = BashTool;
        let result = tool.execute(
            &ToolCall {
                id: "call_2".to_string(),
                name: "Bash".to_string(),
                arguments: serde_json::json!({"command": "exit 3"}),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
            },
        );
        assert!(result.is_error);
        assert!(
            result.output.contains("Command failed with exit code: 3."),
            "{}",
            result.output
        );
    }

    #[test]
    fn bash_tool_timeout_kills() {
        let tool = BashTool;
        let result = tool.execute(
            &ToolCall {
                id: "call_3".to_string(),
                name: "Bash".to_string(),
                arguments: serde_json::json!({"command": "sleep 30", "timeout": 1}),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
            },
        );
        assert!(result.is_error);
        assert!(
            result.output.contains("Command killed by timeout"),
            "{}",
            result.output
        );
    }
}
