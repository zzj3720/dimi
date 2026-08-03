//! Tool effect boundary — slice 1 ships the Bash tool over dimi-exec.
//!
//! Behavior mirrors `agent/tools/os/bash/` (bashTool.ts + bash.ts) for the
//! foreground path: schema validation, `sh -c "cd <shell-quoted cwd> &&
//! <command>"`, the noninteractive env overlay, 50k/2k truncation,
//! 60s/300s timeouts and the exact result strings the TS implementation
//! produces.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dimi_exec::{ShellSpec, SpawnOptions, spawn};

use crate::events::{EngineEvent, EventSink, ToolUpdate};
use crate::llm::LlmClient;

/// Default/max foreground timeout, seconds (bash.ts constants).
pub const DEFAULT_TIMEOUT_S: u64 = 60;
pub const MAX_TIMEOUT_S: u64 = 300;
/// Default deadline for a command moved to the background by a foreground
/// timeout (bash.ts `DEFAULT_BACKGROUND_TIMEOUT_S`).
pub const DEFAULT_BACKGROUND_TIMEOUT_S: u64 = 600;
/// TaskStop SIGTERM grace, milliseconds. **MUST stay equal to the TS
/// constant** `DEFAULT_KILL_GRACE_MS` in
/// `agent/task/configSection.ts` (`killGracePeriodMs` fallback = 5_000) —
/// the two are only comment-linked; if one side changes, change the other.
/// A cancelled background command gets SIGTERM first and SIGKILL only if it
/// has not exited after the grace — processes that trap SIGTERM keep their
/// cleanup window (TS `terminateWithGrace` parity). The bridge overrides
/// this from the turn input's `killGraceMs` (the runner reads the same
/// config the TS task service reads), so a deployment that raises the TS
/// grace is honored end-to-end.
pub const DEFAULT_KILL_GRACE_MS: u64 = 5_000;

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
/// schema validation, `sh -c "cd <shell-quoted cwd> && <command>"`, the
/// noninteractive env overlay, 50k/2k truncation, 60s/300s timeouts and the
/// exact result strings the TS implementation produces. On a foreground
/// timeout the command is moved to the background instead of being killed
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
    /// Task lifecycle event sink: `task.started` on backgrounding,
    /// `task.settled` from the poller when the command settles.
    events: EventSink,
    /// TaskStop SIGTERM grace (TS `killGracePeriodMs`, default
    /// `DEFAULT_KILL_GRACE_MS`): the poller's cancel path waits this long
    /// between SIGTERM and SIGKILL so a trap keeps its cleanup window.
    kill_grace: Duration,
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

    /// Attach the task lifecycle event sink (the bridge wires it to the
    /// session's per-event callback).
    pub fn with_events(mut self, events: EventSink) -> Self {
        self.events = events;
        self
    }

    /// Override the TaskStop SIGTERM grace (the bridge reads the turn
    /// input's `killGraceMs`, which the runner wires from the TS `task`
    /// config section — TS `killGracePeriodMs` parity).
    pub fn with_kill_grace(mut self, kill_grace: Duration) -> Self {
        self.kill_grace = kill_grace;
        self
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
            events: self.events.clone(),
            kill_grace: self.kill_grace,
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self {
            current: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tasks: AgentTasks::new(),
            auto_background_on_timeout: true,
            events: EventSink::new(),
            kill_grace: Duration::from_millis(DEFAULT_KILL_GRACE_MS),
        }
    }
}

impl BashTool {
    /// Validate the Bash arguments (`command` required non-empty; `cwd`
    /// string; `timeout` positive integer capped at MAX_TIMEOUT_S;
    /// `run_in_background` unsupported — a clear error instead of silently
    /// running in the foreground).
    pub fn validate_args(args: &serde_json::Value) -> Result<BashArgs, String> {
        let obj = args.as_object().ok_or("arguments must be an object")?;
        let command = obj
            .get("command")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("command is required")?;
        if obj.get("run_in_background").and_then(|v| v.as_bool()) == Some(true) {
            return Err("run_in_background is not supported by this engine".to_string());
        }
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

/// `bashTool.ts` `shellQuote` mirror: single-quote escaping (`'` → `'\''`)
/// so a cwd containing quotes cannot break the `cd` script.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
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
        // TS `spawn`: `cd ${shellQuote(shellCwd)} && ${command}`.
        let script = format!("cd {} && {}", shell_quote(cwd), args.command);
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
                &self.events,
                call,
                &process,
                &args,
                stdout,
                stderr,
                updates,
                self.kill_grace,
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
    events: &EventSink,
    call: &ToolCall,
    process: &Arc<dimi_exec::ExecProcess>,
    args: &BashArgs,
    stdout: OutputBuffer,
    stderr: OutputBuffer,
    updates: Vec<ToolUpdate>,
    kill_grace: Duration,
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

    // Per-task cancel (TaskStop parity): the poller checks it on every pass
    // and kills the process instead of waiting for the deadline/exit.
    let cancel = std::sync::Arc::new(crate::engine::CancelSignal::new());

    tasks.insert(
        task_id.clone(),
        TaskState {
            agent_id: task_id.clone(),
            status: "running".to_string(),
            output: foreground_output.clone(),
            error: None,
            messages: vec![],
            started_at: now_nanos(),
            cancel: Some(std::sync::Arc::clone(&cancel)),
        },
    );
    events.emit(EngineEvent::TaskStarted {
        task_id: task_id.clone(),
        agent_id: task_id.clone(),
        kind: "bash".to_string(),
        description: description.clone(),
        pid: Some(process.pid() as i64),
        parent_tool_call_id: None,
    });
    // The foreground output (produced before the timeout) is the FIRST
    // `task.output` delta, emitted before the poller starts. The TS
    // adapter's settle-tail arithmetic relies on the streamed deltas being
    // a true prefix of the settle output (`state.output` is seeded with
    // exactly this string) — dropping the prefix duplicates the tail and
    // drops the foreground in TaskOutput's retained buffer.
    if !foreground_output.is_empty() {
        events.emit(EngineEvent::TaskOutput {
            task_id: task_id.clone(),
            delta: foreground_output.clone(),
        });
    }

    // Detached poller: the process handle is Arc-cloned, so the task
    // survives this tool future being dropped (the spawn is `detached`).
    let tasks = tasks.clone();
    let events = events.clone();
    let worker_task_id = task_id.clone();
    let poller_process = Arc::clone(process);
    tokio::spawn(async move {
        // Mirror the foreground drain loop: keep draining until a quiet
        // pass observes the exit flag (or the background deadline fires).
        let started = Instant::now();
        let background_timeout = Duration::from_secs(DEFAULT_BACKGROUND_TIMEOUT_S);
        loop {
            if task_is_cancelled(&tasks, &worker_task_id) {
                // TaskStop parity: TS `terminateWithGrace` — SIGTERM first,
                // escalate to SIGKILL after the grace. A process that traps
                // SIGTERM gets its cleanup window; one that ignores it (or
                // outlives the grace) is force-killed. On Windows the kill
                // signal is ignored (taskkill /T /F) so the grace loop just
                // waits out the already-dead tree.
                let _ = poller_process.kill(Some(15)); // SIGTERM
                let grace_deadline = Instant::now() + kill_grace;
                loop {
                    // Keep draining during the grace so a SIGTERM trap's
                    // cleanup output lands in the settle output (F4).
                    drain_task_output(&tasks, &events, &worker_task_id, &poller_process);
                    if poller_process.exit_code().is_some() {
                        break;
                    }
                    if Instant::now() >= grace_deadline {
                        let _ = poller_process.kill(Some(9)); // SIGKILL
                        let _ = poller_process.wait();
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                let _ = poller_process.wait();
                // Grace drain (TS `waitForStreamDrain`, 250ms): the exit
                // flag can land before the reader threads flush the final
                // pipe chunks (trap cleanup), so drain a little longer.
                let drain_deadline = Instant::now() + Duration::from_millis(250);
                loop {
                    let any = drain_task_output(&tasks, &events, &worker_task_id, &poller_process);
                    if !any || Instant::now() >= drain_deadline {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                tasks.update(&worker_task_id, |state| {
                    state.status = "killed".to_string();
                    state.error = Some("Stopped by TaskStop".to_string());
                });
                let settled = tasks.get(&worker_task_id).expect("backgrounded bash task must be registered");
                events.emit(EngineEvent::TaskSettled {
                    task_id: worker_task_id,
                    agent_id: settled.agent_id,
                    kind: "bash".to_string(),
                    status: settled.status,
                    output: settled.output,
                    error: settled.error,
                    exit_code: None,
                });
                return;
            }
            if !drain_task_output(&tasks, &events, &worker_task_id, &poller_process) {
                if poller_process.exit_code().is_some() {
                    break;
                }
                if started.elapsed() > background_timeout {
                    // TS `detachTimeoutMs` (bashTaskTimeoutS, default 600s):
                    // a backgrounded command that overruns its own deadline
                    // is killed.
                    let _ = poller_process.kill(Some(9));
                    let _ = poller_process.wait();
                    // Post-kill drain (TS `waitForStreamDrain`, 250ms): the
                    // process may have written between the last poll drain
                    // and the SIGKILL; the TS implementation drains after
                    // the kill too, so do the same to keep the settle output
                    // (and the delta stream) from losing the final tail.
                    let drain_deadline = Instant::now() + Duration::from_millis(250);
                    loop {
                        let any =
                            drain_task_output(&tasks, &events, &worker_task_id, &poller_process);
                        if !any || Instant::now() >= drain_deadline {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    tasks.update(&worker_task_id, |state| {
                        state.status = "timed_out".to_string();
                        state.error = Some(format!(
                            "Command killed by timeout ({}s)",
                            DEFAULT_BACKGROUND_TIMEOUT_S
                        ));
                    });
                    let settled = tasks.get(&worker_task_id).expect("backgrounded bash task must be registered");
                    events.emit(EngineEvent::TaskSettled {
                        task_id: worker_task_id,
                        agent_id: settled.agent_id,
                        kind: "bash".to_string(),
                        status: settled.status,
                        output: settled.output,
                        error: settled.error,
                        exit_code: None,
                    });
                    return;
                }
                if events.is_closed() {
                    // Session torn down (TS `taskService.dispose` parity): kill
                    // the background command; the sink is closed so no settle is
                    // emitted and nothing fires into the disposed runner.
                    let _ = poller_process.kill(Some(9));
                    let _ = poller_process.wait();
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
            let any = drain_task_output(&tasks, &events, &worker_task_id, &poller_process);
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
        let settled = tasks.get(&worker_task_id).expect("backgrounded bash task must be registered");
        events.emit(EngineEvent::TaskSettled {
            task_id: worker_task_id,
            agent_id: settled.agent_id,
            kind: "bash".to_string(),
            status: settled.status,
            output: settled.output,
            error: settled.error,
            exit_code: code.map(|c| c as i64),
        });
    });

    let metadata = format!(
        "Command timed out ({}s); it is still running in the background.\n\
         task_id: {task_id}\n\
         pid: {}\n\
         description: {description}\n\
         status: running\n\
         automatic_notification: true\n\
         next_step: The task now runs in the background. You will be automatically \
         notified when it completes — do NOT wait or poll; continue with your current work.",
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

/// Append a drained chunk and emit a `task.output` event with exactly the
/// appended delta, so the TS side can stream live output into TaskOutput
/// while the task is still running (TS ProcessTask parity). The delta is the
/// same `take(remaining)` substring `append_task_output` appends, so the sum
/// of deltas equals the final settle output byte-for-byte.
fn append_task_output_and_emit(tasks: &AgentTasks, events: &EventSink, task_id: &str, chunk: &[u8]) {
    let Some(state) = tasks.get(task_id) else {
        return;
    };
    if state.output.len() >= DEFAULT_MAX_CHARS {
        return;
    }
    let text = String::from_utf8_lossy(chunk);
    let remaining = DEFAULT_MAX_CHARS - state.output.len();
    let delta: String = text.chars().take(remaining).collect();
    if delta.is_empty() {
        return;
    }
    tasks.update(task_id, |state| append_task_output(state, chunk));
    events.emit(EngineEvent::TaskOutput {
        task_id: task_id.to_string(),
        delta,
    });
}

/// Drain every currently-available stdout/stderr chunk into the task state
/// and emit a `task.output` delta per chunk. Returns whether any chunk was
/// drained. Never sleeps — callers own the pacing (the main poll loop, the
/// post-exit grace drain, and the SIGTERM-grace drain all use it).
fn drain_task_output(
    tasks: &AgentTasks,
    events: &EventSink,
    task_id: &str,
    process: &Arc<dimi_exec::ExecProcess>,
) -> bool {
    let mut any = false;
    while let Some(chunk) = process.try_recv_stdout() {
        any = true;
        append_task_output_and_emit(tasks, events, task_id, &chunk);
    }
    while let Some(chunk) = process.try_recv_stderr() {
        any = true;
        append_task_output_and_emit(tasks, events, task_id, &chunk);
    }
    any
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
    async fn bash_tool_escapes_single_quotes_in_cwd() {
        // A cwd containing a single quote must not break the `cd` script
        // (TS `shellQuote` parity: `'` → `'\''`).
        let dir = std::env::temp_dir().join("dimi-bash-quote-it's");
        std::fs::create_dir_all(&dir).expect("create quote dir");
        let tool = BashTool::default();
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_q".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "pwd"}),
                },
                &ToolContext {
                    cwd: dir.to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!result.is_error, "output: {}", result.output);
        assert!(
            result.output.contains("it's"),
            "cd must land in the quoted dir: {}",
            result.output
        );
    }

    #[test]
    fn validate_args_rejects_run_in_background() {
        // The schema keeps `run_in_background` for model compatibility, but
        // the executor must not silently run in the foreground.
        let error = BashTool::validate_args(&serde_json::json!({
            "command": "ls",
            "run_in_background": true,
        }))
        .unwrap_err();
        assert!(
            error.contains("run_in_background"),
            "error must be explicit: {error}"
        );
        assert!(
            BashTool::validate_args(&serde_json::json!({
                "command": "ls",
                "run_in_background": false,
            }))
            .is_ok()
        );
    }

    #[test]
    fn shell_quote_mirrors_ts() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("a'b'c"), "'a'\\''b'\\''c'");
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
        // The completion notification is actually delivered (P1-7): the
        // metadata says automatic_notification: true and stops telling the
        // model to poll.
        assert!(
            result.output.contains("automatic_notification: true"),
            "{}",
            result.output
        );
        assert!(
            result.output.contains("You will be automatically notified when it completes"),
            "{}",
            result.output
        );
        assert!(
            result.output.contains("do NOT wait or poll"),
            "{}",
            result.output
        );

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

    #[tokio::test]
    async fn bash_tool_streams_task_output_before_settle() {
        // TS ProcessTask parity: output produced while the backgrounded
        // command still runs is streamed as `task.output` events (so the TS
        // adapter can show live TaskOutput), and the mid chunk arrives
        // BEFORE the settle — not only at settle.
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = EventSink::new();
        let recorded = std::sync::Arc::clone(&events);
        sink.set(std::sync::Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        }));
        let tasks = AgentTasks::new();
        let tool = BashTool {
            tasks: tasks.clone(),
            events: sink,
            ..BashTool::default()
        };
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg_stream".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        "command": "sleep 2; echo mid; sleep 2; echo done",
                        "timeout": 1,
                        "description": "stream probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
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
                id: "call_ws".to_string(),
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

        let emitted = events.lock().unwrap();
        let mid = emitted.iter().position(|event| {
            matches!(event, EngineEvent::TaskOutput { delta, .. } if delta.contains("mid"))
        });
        let done = emitted.iter().position(|event| {
            matches!(event, EngineEvent::TaskOutput { delta, .. } if delta.contains("done"))
        });
        let settled = emitted
            .iter()
            .position(|event| matches!(event, EngineEvent::TaskSettled { .. }));
        assert!(
            mid.is_some(),
            "mid must be streamed as task.output before settle: {emitted:?}"
        );
        assert!(done.is_some(), "done must be streamed: {emitted:?}");
        assert!(
            mid.unwrap() < settled.expect("task.settled emitted"),
            "mid must arrive before the settle: {emitted:?}"
        );
    }

    #[tokio::test]
    async fn bash_tool_cancel_sends_sigterm_before_sigkill() {
        // TS `terminateWithGrace` parity: TaskStop sends SIGTERM first; a
        // command that traps SIGTERM gets its cleanup window and exits before
        // any SIGKILL is needed.
        let dir = std::env::temp_dir().join(format!("dimi-bash-term-grace-{}", uuid_v4_short()));
        std::fs::create_dir_all(&dir).expect("create marker dir");
        let marker = dir.join("cleanup-ran");
        let tasks = AgentTasks::new();
        let tool = BashTool::with_tasks(tasks.clone());
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg_term".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        "command": format!(
                            "trap 'echo cleaned > \"{}\"; exit 0' TERM; sleep 30",
                            marker.display()
                        ),
                        "timeout": 1,
                        "description": "term grace probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        let task_id = result
            .output
            .lines()
            .find_map(|line| line.strip_prefix("task_id: "))
            .map(str::trim)
            .expect("task_id in result output")
            .to_string();

        // TaskStop parity: flip the per-task cancel signal.
        let state = tasks.get(&task_id).expect("backgrounded task registered");
        state.cancel.expect("cancel signal").cancel();

        let waited = WaitForTool {
            tasks: tasks.clone(),
        }
        .execute(
            &ToolCall {
                id: "call_wt".to_string(),
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
        assert_eq!(waited_json["status"], "killed");
        assert_eq!(
            waited_json["error"].as_str(),
            Some("Stopped by TaskStop"),
            "{}",
            waited.output
        );
        assert!(
            marker.exists(),
            "SIGTERM must be delivered before any SIGKILL (trap cleanup ran)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn bash_tool_cancel_escalates_to_sigkill_after_grace() {
        // A command that ignores SIGTERM (`trap '' TERM`) survives the grace
        // window and is force-killed with SIGKILL — the settle still lands as
        // "killed" (TS `terminateWithGrace` escalation).
        let tasks = AgentTasks::new();
        let tool = BashTool::with_tasks(tasks.clone());
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg_kill".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        "command": "trap '' TERM; sleep 30",
                        "timeout": 1,
                        "description": "sigkill escalation probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        let task_id = result
            .output
            .lines()
            .find_map(|line| line.strip_prefix("task_id: "))
            .map(str::trim)
            .expect("task_id in result output")
            .to_string();

        let state = tasks.get(&task_id).expect("backgrounded task registered");
        state.cancel.expect("cancel signal").cancel();

        // 20s > the 5s grace: WaitFor resolves only after the SIGKILL lands.
        let waited = WaitForTool {
            tasks: tasks.clone(),
        }
        .execute(
            &ToolCall {
                id: "call_wk".to_string(),
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
        assert_eq!(waited_json["status"], "killed");
        assert_eq!(
            waited_json["error"].as_str(),
            Some("Stopped by TaskStop"),
            "{}",
            waited.output
        );
    }

    #[tokio::test]
    async fn bash_tool_backgrounded_foreground_output_is_the_first_delta() {
        // F1 (review P1): a command that printed BEFORE the timeout is
        // backgrounded with its foreground output as the FIRST `task.output`
        // delta (emitted before the poller starts), so the streamed deltas
        // concatenate byte-for-byte to the settle output — the invariant the
        // TS adapter's settle-tail arithmetic relies on. Without the prefix,
        // TaskOutput's retained buffer would drop `start` and duplicate the
        // post-timeout tail (`middle\niddle\n`).
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = EventSink::new();
        let recorded = std::sync::Arc::clone(&events);
        sink.set(std::sync::Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        }));
        let tasks = AgentTasks::new();
        let tool = BashTool {
            tasks: tasks.clone(),
            events: sink,
            ..BashTool::default()
        };
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg_fg".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        // `start` lands before the 1s timeout; `middle` after
                        // (sleep 2) — both must survive into the settle.
                        "command": "echo start; sleep 2; echo middle; sleep 1",
                        "timeout": 1,
                        "description": "foreground prefix probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
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
                id: "call_wfg".to_string(),
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
        assert_eq!(waited_json["status"], "completed");

        let emitted = events.lock().unwrap();
        let deltas: Vec<String> = emitted
            .iter()
            .filter_map(|event| match event {
                EngineEvent::TaskOutput { task_id: id, delta } if id == &task_id => {
                    Some(delta.clone())
                }
                _ => None,
            })
            .collect();
        let settled = emitted
            .iter()
            .find_map(|event| match event {
                EngineEvent::TaskSettled {
                    task_id: id,
                    output,
                    ..
                } if id == &task_id => Some(output.clone()),
                _ => None,
            })
            .expect("task.settled emitted");
        assert!(
            !deltas.is_empty() && deltas[0].contains("start"),
            "foreground output must be the first delta: {deltas:?}"
        );
        assert!(
            deltas.iter().any(|delta| delta.contains("middle")),
            "post-timeout output must be streamed too: {deltas:?}"
        );
        assert_eq!(
            deltas.concat(),
            settled,
            "streamed deltas must concatenate exactly to the settle output"
        );
    }

    #[tokio::test]
    async fn bash_tool_cancel_uses_configured_grace_and_drains_trap_output() {
        // F3 + F4 (review nits): the SIGTERM grace is configurable
        // (TS `killGracePeriodMs` parity — here shortened to 300ms so the
        // test cannot take the 5s default), and output produced during the
        // grace (a SIGTERM trap's cleanup) is drained into the settle
        // output instead of being dropped.
        let dir = std::env::temp_dir().join(format!("dimi-bash-drain-grace-{}", uuid_v4_short()));
        std::fs::create_dir_all(&dir).expect("create marker dir");
        let marker = dir.join("trap-cleanup-ran");
        let tasks = AgentTasks::new();
        let tool = BashTool {
            tasks: tasks.clone(),
            kill_grace: Duration::from_millis(300),
            ..BashTool::default()
        };
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg_drain".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        "command": format!(
                            "trap 'echo cleanup-out; echo cleaned > \"{}\"; exit 0' TERM; sleep 30",
                            marker.display()
                        ),
                        "timeout": 1,
                        "description": "grace drain probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        let task_id = result
            .output
            .lines()
            .find_map(|line| line.strip_prefix("task_id: "))
            .map(str::trim)
            .expect("task_id in result output")
            .to_string();

        let state = tasks.get(&task_id).expect("backgrounded task registered");
        state.cancel.expect("cancel signal").cancel();
        let cancel_at = std::time::Instant::now();

        let waited = WaitForTool {
            tasks: tasks.clone(),
        }
        .execute(
            &ToolCall {
                id: "call_wd".to_string(),
                name: "WaitFor".to_string(),
                arguments: serde_json::json!({ "agent_id": task_id, "timeout_seconds": 10 }),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
            },
        )
        .await;
        let elapsed = cancel_at.elapsed();
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(waited_json["status"], "killed");
        assert_eq!(
            waited_json["error"].as_str(),
            Some("Stopped by TaskStop"),
            "{}",
            waited.output
        );
        assert!(
            waited_json["output"]
                .as_str()
                .map(|output| output.contains("cleanup-out"))
                .unwrap_or(false),
            "trap cleanup output must be drained during the grace: {}",
            waited.output
        );
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "custom 300ms grace must not wait the 5s default: {elapsed:?}"
        );
        assert!(
            marker.exists(),
            "SIGTERM trap cleanup must run within the configured grace"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
            next_agent_id: None,
            kill_grace_ms: None,
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
    pub status: String, // running | completed | failed | killed | timed_out
    pub output: String,
    pub error: Option<String>,
    /// Full message history of the subagent's latest turn (resume carries
    /// it into the next turn).
    pub messages: Vec<crate::types::LlmMessage>,
    /// Monotonic launch timestamp (nanos) — disambiguates tasks that share
    /// an agent id (resume): the newest wins for lookups.
    pub started_at: u128,
    /// Per-task cancel (TaskStop parity): the bridge's `cancel_task` flips
    /// it; the bash poller / subagent worker observes it, stops the work and
    /// settles the task with status "killed".
    pub cancel: Option<std::sync::Arc<crate::engine::CancelSignal>>,
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
    /// Task lifecycle event sink: `task.started` on launch, `task.settled`
    /// from the spawned worker when the nested turn completes.
    pub events: EventSink,
    /// Subagent agent-id counter (`agent-<n>`), seeded from the turn input's
    /// `next_agent_id` (TS `nextAvailableAgentId` parity) so ids stay
    /// monotonic across turns within a session and never collide with
    /// TS-assigned ids after a server restart.
    pub agent_id_counter: std::sync::atomic::AtomicU64,
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
    cancel: std::sync::Arc<crate::engine::CancelSignal>,
    // Forward the nested turn's assistant text as it streams (the subagent
    // worker uses this to emit `task.output` deltas for live TaskOutput).
    on_delta: Option<Arc<dyn Fn(&str) + Send + Sync>>,
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
        next_agent_id: None,
        kill_grace_ms: None,
    };
    // The nested turn's own cancellation: the parent session's `cancel_task`
    // (TaskStop parity) flips the signal, so a TaskStop mid-subagent reaches
    // the nested turn instead of letting it run to completion.
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut session = crate::engine::TurnSession::with_steer_and_cancel(input, steer, cancel, finished);
    let mut events: Vec<crate::events::EngineEvent> = Vec::new();
    let progress = session
        .run(llm.as_ref(), tools.as_ref(), &policy, &mut |event| {
            if let crate::events::EngineEvent::AssistantDelta { delta, .. } = &event {
                if let Some(callback) = &on_delta {
                    callback(delta);
                }
            }
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

        let agent_id = self.next_agent_id();
        self.launch_subagent(call, ctx, prompt, description, agent_id, Vec::new())
            .await
    }
}

impl AsyncAgentTool {
    /// The next agent id: `agent-<n>` with a monotonically increasing counter
    /// seeded from the turn input's `next_agent_id` (TS
    /// `nextAvailableAgentId` parity). Each turn session's tool seeds from
    /// the runner's computed next id, so ids stay monotonic across turns
    /// within a session and never collide with TS-assigned ids (which
    /// continue from persisted session metadata across restarts).
    fn next_agent_id(&self) -> String {
        use std::sync::atomic::Ordering;
        format!("agent-{}", self.agent_id_counter.fetch_add(1, Ordering::Relaxed))
    }

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
        // TS `SubagentTask.idPrefix = "agent"` parity: the wire task id for a
        // subagent is `agent-<8>` (bash background tasks keep `bash-<8>`).
        let task_id = format!("agent-{}", uuid_v4_short());
        let cancel = std::sync::Arc::new(crate::engine::CancelSignal::new());
        self.tasks.insert(
            task_id.clone(),
            TaskState {
                agent_id: agent_id.clone(),
                status: "running".to_string(),
                output: String::new(),
                error: None,
                messages: history.clone(),
                started_at: now_nanos(),
                cancel: Some(std::sync::Arc::clone(&cancel)),
            },
        );
        self.events.emit(EngineEvent::TaskStarted {
            task_id: task_id.clone(),
            agent_id: agent_id.clone(),
            kind: "agent".to_string(),
            description: description.clone(),
            pid: None,
            parent_tool_call_id: Some(call.id.clone()),
        });

        let tasks = self.tasks.clone();
        let events = self.events.clone();
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
        // The streamed assistant text accumulated by the worker (P1): every
        // forwarded delta is appended here, so the settle output — in the
        // killed/failed branches too — is exactly the concatenation of the
        // `task.output` deltas the TS adapter streamed (the adapter's
        // documented byte-for-byte invariant; previously only the success
        // branch carried the text and a TaskStopped/failed subagent settled
        // with an empty output). Capped at DEFAULT_MAX_CHARS (F4): the same
        // limit the bash path enforces (`append_task_output_and_emit`), so a
        // chatty subagent cannot grow `state.output` / AgentOutput / WaitFor
        // JSON without bound on the agent path.
        let accumulated_output: Arc<std::sync::Mutex<String>> =
            Arc::new(std::sync::Mutex::new(String::new()));
        tokio::spawn(async move {
            // Live output (F2): forward the nested turn's assistant deltas
            // as `task.output` events so TaskOutput shows the subagent's
            // text while it still runs, and accumulate them for the settle
            // output. The success/killed/failed branches all settle with
            // this accumulated text, so the TS adapter's delta-prefix
            // invariant holds byte-for-byte on every path. The cap mirrors
            // `append_task_output_and_emit`: once the accumulated string is
            // at DEFAULT_MAX_CHARS no further delta is appended OR emitted,
            // and a delta that would cross the cap is truncated to the
            // remaining budget and emitted with exactly the appended
            // substring — so `sum(emitted deltas) == accumulated ==
            // settle output` still holds byte-for-byte.
            let on_delta = {
                let events = events.clone();
                let task_id = task_id_for_worker.clone();
                let accumulated = Arc::clone(&accumulated_output);
                Some(std::sync::Arc::new(
                    move |delta: &str| {
                        if delta.is_empty() {
                            return;
                        }
                        let mut acc = accumulated
                            .lock()
                            .unwrap_or_else(|p| p.into_inner());
                        if acc.len() >= DEFAULT_MAX_CHARS {
                            return;
                        }
                        let remaining = DEFAULT_MAX_CHARS - acc.len();
                        let capped: String = delta.chars().take(remaining).collect();
                        if capped.is_empty() {
                            return;
                        }
                        acc.push_str(&capped);
                        events.emit(EngineEvent::TaskOutput {
                            task_id: task_id.clone(),
                            delta: capped,
                        });
                    },
                ) as std::sync::Arc<dyn Fn(&str) + Send + Sync>)
            };
            let (_output, error, messages) = run_nested_turn(
                history,
                prompt,
                cwd,
                shell,
                llm,
                tools,
                policy,
                max_steps,
                Some(steer_queue),
                std::sync::Arc::clone(&cancel),
                on_delta,
            )
            .await;
            // Session torn down while the nested turn ran (TS
            // `taskService.dispose` parity): skip the settle + notification —
            // the sink is closed, so nothing would reach the runner anyway.
            if events.is_closed() {
                return;
            }
            let output = accumulated_output
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            tasks.update(&task_id_for_worker, |state| {
                state.messages = messages;
                state.output = output;
                if cancel.is_cancelled() {
                    // TaskStop parity: a per-task cancel settles "killed"
                    // (TS `terminateWithGrace` final status), not "failed".
                    state.status = "killed".to_string();
                    state.error = Some("Stopped by TaskStop".to_string());
                } else if error.is_empty() {
                    state.status = "completed".to_string();
                } else {
                    state.status = "failed".to_string();
                    state.error = Some(error);
                }
            });
            let settled = tasks
                .get(&task_id_for_worker)
                .expect("launched subagent task must be registered");
            events.emit(EngineEvent::TaskSettled {
                task_id: task_id_for_worker,
                agent_id: settled.agent_id,
                kind: "agent".to_string(),
                status: settled.status,
                output: settled.output,
                error: settled.error,
                exit_code: None,
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

/// A unique id suffix: 8 chars from a UUID v4 — the TS task-id shape
/// (`<kind>-<8 chars>`, `generateTaskId`), with proper cross-thread
/// uniqueness.
fn uuid_v4_short() -> String {
    let simple = uuid::Uuid::new_v4().simple().to_string();
    simple[..8].to_string()
}

/// Whether a background task was cancelled via the bridge's per-task cancel
/// (TaskStop parity): the bash poller checks this on every quiet pass and
/// kills the process instead of waiting for the deadline/exit.
fn task_is_cancelled(tasks: &AgentTasks, task_id: &str) -> bool {
    tasks
        .get(task_id)
        .and_then(|state| state.cancel)
        .map(|cancel| cancel.is_cancelled())
        .unwrap_or(false)
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

/// TS `WAIT_DEFAULT_SECONDS` / `WAIT_MAX_SECONDS` (waitForTool.ts schema):
/// the TS schema rejects values above the max; the engine clamps instead
/// (Bash timeout cap parity) so a huge value cannot pin the turn forever.
pub const WAIT_DEFAULT_SECONDS: u64 = 60;
pub const WAIT_MAX_SECONDS: u64 = 1_800;

/// `timeout_seconds` normalization: default 60, capped at 1800 (the TS
/// schema's max).
fn normalize_wait_timeout(value: Option<u64>) -> u64 {
    value.unwrap_or(WAIT_DEFAULT_SECONDS).min(WAIT_MAX_SECONDS)
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
        let timeout = normalize_wait_timeout(
            call.arguments
                .get("timeout_seconds")
                .and_then(|v| v.as_u64()),
        );
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
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
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
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
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

    #[test]
    fn waitfor_timeout_is_capped_at_the_ts_max() {
        // TS `WaitForInputSchema` bounds timeout_seconds at WAIT_MAX_SECONDS
        // (1800); the engine clamps instead of rejecting so a huge value
        // cannot pin the turn forever.
        assert_eq!(normalize_wait_timeout(None), WAIT_DEFAULT_SECONDS);
        assert_eq!(normalize_wait_timeout(Some(10)), 10);
        assert_eq!(normalize_wait_timeout(Some(1_800)), WAIT_MAX_SECONDS);
        assert_eq!(normalize_wait_timeout(Some(999_999)), WAIT_MAX_SECONDS);
        assert_eq!(normalize_wait_timeout(Some(u64::MAX)), WAIT_MAX_SECONDS);
    }


    #[tokio::test]
    async fn subagent_emits_task_started_and_settled_events() {
        use crate::events::EventSink;
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = EventSink::new();
        let recorded = std::sync::Arc::clone(&events);
        sink.set(std::sync::Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        }));
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![
                LlmStreamEvent::Text {
                    delta: "sub done".to_string(),
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
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "work" }),
                },
                &ctx(),
            )
            .await;
        assert!(!launch.is_error, "output: {}", launch.output);
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();
        // TS lifecycle format: agent-<n>.
        assert!(
            agent_id.starts_with("agent-") && agent_id["agent-".len()..].chars().all(|c| c.is_ascii_digit()),
            "agent_id must be agent-<n>: {agent_id}"
        );

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

        let emitted = events.lock().unwrap();
        let started = emitted
            .iter()
            .find(|e| matches!(e, crate::events::EngineEvent::TaskStarted { .. }))
            .expect("task.started emitted");
        let (task_id, kind) = match started {
            crate::events::EngineEvent::TaskStarted {
                task_id, kind, ..
            } => (task_id.clone(), kind.clone()),
            _ => unreachable!(),
        };
        assert_eq!(kind, "agent");
        let settled = emitted
            .iter()
            .find(|e| matches!(e, crate::events::EngineEvent::TaskSettled { .. }))
            .expect("task.settled emitted");
        match settled {
            crate::events::EngineEvent::TaskSettled {
                task_id: settled_id,
                status,
                output,
                ..
            } => {
                assert_eq!(settled_id, &task_id);
                assert_eq!(status, "completed");
                assert!(output.contains("sub done"), "output: {output}");
            }
            _ => unreachable!(),
        }
    }

    #[tokio::test]
    async fn subagent_streams_assistant_deltas_as_task_output() {
        // F2 (review nit): the subagent worker forwards the nested turn's
        // assistant deltas as `task.output` events while the subagent still
        // runs, so TaskOutput shows live subagent output instead of staying
        // empty until settle. The deltas concatenate to the settle output
        // (same invariant the bash poller maintains).
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = EventSink::new();
        let recorded = std::sync::Arc::clone(&events);
        sink.set(std::sync::Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        }));
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![
                LlmStreamEvent::Text {
                    delta: "part one ".to_string(),
                },
                LlmStreamEvent::Text {
                    delta: "part two".to_string(),
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
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a_stream".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "stream work" }),
                },
                &ctx(),
            )
            .await;
        assert!(!launch.is_error, "output: {}", launch.output);
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();

        let wait = WaitForTool {
            tasks: tasks.clone(),
        };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_ws2".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id, "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);

        let emitted = events.lock().unwrap();
        let task_id = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskStarted { task_id, kind, .. }
                    if kind == "agent" =>
                {
                    Some(task_id.clone())
                }
                _ => None,
            })
            .expect("subagent task.started emitted");
        let deltas: Vec<String> = emitted
            .iter()
            .filter_map(|event| match event {
                crate::events::EngineEvent::TaskOutput {
                    task_id: id,
                    delta,
                } if id == &task_id => Some(delta.clone()),
                _ => None,
            })
            .collect();
        let settled = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskSettled {
                    task_id: id,
                    output,
                    ..
                } if id == &task_id => Some(output.clone()),
                _ => None,
            })
            .expect("subagent task.settled emitted");
        let settled_pos = emitted
            .iter()
            .position(|event| {
                matches!(event, crate::events::EngineEvent::TaskSettled { .. })
            })
            .expect("task.settled position");
        assert!(
            !deltas.is_empty(),
            "assistant text must stream as task.output: {emitted:?}"
        );
        assert_eq!(
            deltas.concat(),
            "part one part two",
            "deltas must carry the nested turn's assistant text: {deltas:?}"
        );
        assert!(
            emitted.iter().take(settled_pos).any(|event| {
                matches!(event, crate::events::EngineEvent::TaskOutput { .. })
            }),
            "task.output must arrive before task.settled: {emitted:?}"
        );
        assert_eq!(
            deltas.concat(),
            settled,
            "streamed deltas must concatenate exactly to the settle output"
        );
    }

    #[tokio::test]
    async fn subagent_cancelled_mid_stream_settles_killed_with_streamed_output() {
        // P1 regression (adversarial review): a subagent that streamed text
        // and is TaskStopped mid-run settles "killed" carrying the streamed
        // output, so the wire `task.terminated` outputTail / completion
        // notification are not empty while the live TaskOutput showed the
        // text — the delta == settle invariant holds on the killed path too.
        use crate::llm::{ChatRequest, LlmClient, LlmError, StreamedTurn};
        use std::sync::atomic::Ordering;

        // Call 1 streams text + a Bash tool call (so the nested turn
        // continues to a second step); call 2 blocks until the test cancels
        // the nested turn (TaskStop parity).
        struct StreamThenBlockClient {
            calls: std::sync::atomic::AtomicU64,
            second_started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        }
        #[async_trait::async_trait]
        impl LlmClient for StreamThenBlockClient {
            async fn stream_chat(&self, _request: &ChatRequest) -> Result<StreamedTurn, LlmError> {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    return Ok(StreamedTurn {
                        events: vec![
                            LlmStreamEvent::Text {
                                delta: "part one ".to_string(),
                            },
                            LlmStreamEvent::Text {
                                delta: "part two".to_string(),
                            },
                            LlmStreamEvent::ToolCall {
                                tool_call_id: "call_bash".to_string(),
                                name: Some("Bash".to_string()),
                                arguments_part: Some(
                                    "{\"command\":\"echo subagent-step\"}".to_string(),
                                ),
                            },
                            LlmStreamEvent::Finish {
                                finish_reason: Some("tool_calls".to_string()),
                            },
                        ],
                        assistant: crate::llm::AssistantTurn {
                            tool_calls: vec![crate::types::LlmToolCall {
                                id: "call_bash".to_string(),
                                call_type: Some("function".to_string()),
                                function: crate::types::LlmToolCallFunction {
                                    name: "Bash".to_string(),
                                    arguments: "{\"command\":\"echo subagent-step\"}".to_string(),
                                },
                            }],
                            text: "part one part two".to_string(),
                            thinking: String::new(),
                        },
                    });
                }
                // Second call: signal the test that the nested turn is in
                // flight, then block until it is cancelled (TaskStop).
                if let Some(tx) = self.second_started.lock().await.take() {
                    let _ = tx.send(());
                }
                std::future::pending::<()>().await;
                unreachable!("blocking client never returns");
            }
        }

        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = EventSink::new();
        let recorded = std::sync::Arc::clone(&events);
        sink.set(std::sync::Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        }));
        let (second_tx, second_rx) = tokio::sync::oneshot::channel::<()>();
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: Arc::new(StreamThenBlockClient {
                calls: std::sync::atomic::AtomicU64::new(0),
                second_started: tokio::sync::Mutex::new(Some(second_tx)),
            }),
            tools: Arc::new(BashTool::default()),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a_cancel".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "stream then stop" }),
                },
                &ctx(),
            )
            .await;
        assert!(!launch.is_error, "output: {}", launch.output);
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();

        // Wait until the nested turn is blocked on its second LLM call — by
        // then the first call's deltas have streamed (and accumulated).
        tokio::time::timeout(std::time::Duration::from_secs(5), second_rx)
            .await
            .expect("nested turn must reach the blocking second call")
            .expect("nested turn's sender must still be live");

        // TaskStop the subagent mid-run (per-task cancel parity).
        let (_, state) = tasks
            .find_by_agent_id_with_key(&agent_id)
            .expect("subagent task registered");
        state
            .cancel
            .as_ref()
            .expect("subagent has a per-task cancel signal")
            .cancel();

        let wait = WaitForTool {
            tasks: tasks.clone(),
        };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_wc".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id, "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(
            waited_json["status"], "killed",
            "TaskStop must settle the subagent as killed: {waited_json}"
        );
        assert_eq!(
            waited_json["output"], "part one part two",
            "the settle output must carry the streamed text: {waited_json}"
        );

        // Wire-level: task.output deltas concatenate byte-for-byte to the
        // task.settled output (the P1 invariant on the killed path).
        let emitted = events.lock().unwrap();
        let task_id = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskStarted { task_id, kind, .. }
                    if kind == "agent" =>
                {
                    Some(task_id.clone())
                }
                _ => None,
            })
            .expect("subagent task.started emitted");
        let deltas: Vec<String> = emitted
            .iter()
            .filter_map(|event| match event {
                crate::events::EngineEvent::TaskOutput {
                    task_id: id,
                    delta,
                } if id == &task_id => Some(delta.clone()),
                _ => None,
            })
            .collect();
        let (settled_status, settled) = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskSettled {
                    task_id: id,
                    status,
                    output,
                    ..
                } if id == &task_id => Some((status.clone(), output.clone())),
                _ => None,
            })
            .expect("subagent task.settled emitted");
        assert_eq!(settled_status, "killed");
        assert_eq!(
            deltas.concat(),
            settled,
            "streamed deltas must concatenate exactly to the killed settle output: {deltas:?} / {settled:?}"
        );
    }

    #[tokio::test]
    async fn subagent_fails_after_streaming_settles_failed_with_streamed_output() {
        // P1 regression (failed branch): a subagent that streamed text and
        // then fails (the max-steps guard trips on the second step) settles
        // "failed" carrying the streamed output.
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = EventSink::new();
        let recorded = std::sync::Arc::clone(&events);
        sink.set(std::sync::Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        }));
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![
                LlmStreamEvent::Text {
                    delta: "part one ".to_string(),
                },
                LlmStreamEvent::Text {
                    delta: "part two".to_string(),
                },
                LlmStreamEvent::ToolCall {
                    tool_call_id: "call_bash_fail".to_string(),
                    name: Some("Bash".to_string()),
                    arguments_part: Some("{\"command\":\"echo subagent-step\"}".to_string()),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ])),
            tools: Arc::new(BashTool::default()),
            policy: policy_auto(),
            // One step only: after step 1 (streamed text + Bash tool call)
            // the max-steps guard fails the turn.
            max_steps: Some(1),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a_fail".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "stream then fail" }),
                },
                &ctx(),
            )
            .await;
        assert!(!launch.is_error, "output: {}", launch.output);
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();

        let wait = WaitForTool {
            tasks: tasks.clone(),
        };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_wf".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id, "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(
            waited_json["status"], "failed",
            "the subagent must settle as failed: {waited_json}"
        );
        assert_eq!(
            waited_json["output"], "part one part two",
            "the settle output must carry the streamed text: {waited_json}"
        );

        // Wire-level: task.output deltas concatenate byte-for-byte to the
        // task.settled output (the P1 invariant on the failed path).
        let emitted = events.lock().unwrap();
        let task_id = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskStarted { task_id, kind, .. }
                    if kind == "agent" =>
                {
                    Some(task_id.clone())
                }
                _ => None,
            })
            .expect("subagent task.started emitted");
        let deltas: Vec<String> = emitted
            .iter()
            .filter_map(|event| match event {
                crate::events::EngineEvent::TaskOutput {
                    task_id: id,
                    delta,
                } if id == &task_id => Some(delta.clone()),
                _ => None,
            })
            .collect();
        let (settled_status, settled) = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskSettled {
                    task_id: id,
                    status,
                    output,
                    ..
                } if id == &task_id => Some((status.clone(), output.clone())),
                _ => None,
            })
            .expect("subagent task.settled emitted");
        assert_eq!(settled_status, "failed");
        assert_eq!(
            deltas.concat(),
            settled,
            "streamed deltas must concatenate exactly to the failed settle output: {deltas:?} / {settled:?}"
        );
    }

    #[tokio::test]
    async fn subagent_output_accumulator_caps_at_default_max_chars() {
        // F4 (adversarial review): the agent-path output accumulator must cap
        // at DEFAULT_MAX_CHARS the same way the bash path does, so a chatty
        // subagent cannot grow `state.output` / WaitFor JSON without bound.
        // The cap preserves the delta == settle invariant: a delta that
        // crosses the cap is truncated to the remaining budget (appended AND
        // emitted identically), and once the cap is reached later deltas are
        // dropped from BOTH the accumulator and the emitted stream.
        let fill = "x".repeat(DEFAULT_MAX_CHARS - 1_000);
        let over = "y".repeat(2_000);
        let after_cap = "z".repeat(500);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = EventSink::new();
        let recorded = std::sync::Arc::clone(&events);
        sink.set(std::sync::Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        }));
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![
                LlmStreamEvent::Text {
                    delta: fill.clone(),
                },
                LlmStreamEvent::Text {
                    delta: over.clone(),
                },
                LlmStreamEvent::Text {
                    delta: after_cap.clone(),
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
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a_cap".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({ "prompt": "stream a lot" }),
                },
                &ctx(),
            )
            .await;
        assert!(!launch.is_error, "output: {}", launch.output);
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();

        let wait = WaitForTool {
            tasks: tasks.clone(),
        };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_wc".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id, "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(
            waited_json["status"], "completed",
            "the subagent must complete: {waited_json}"
        );
        let output = waited_json["output"].as_str().unwrap();
        assert_eq!(
            output.chars().count(),
            DEFAULT_MAX_CHARS,
            "the settle output must be capped at DEFAULT_MAX_CHARS ({} chars, not {})",
            DEFAULT_MAX_CHARS,
            output.chars().count()
        );
        assert_eq!(
            output,
            format!("{fill}{}", "y".repeat(1_000)),
            "the over-budget delta must be truncated to the remaining budget"
        );
        assert!(
            !output.contains('z'),
            "deltas past the cap must not reach the settle output"
        );

        // Wire invariant under the cap: the emitted `task.output` deltas
        // still concatenate byte-for-byte to the settle output, and the
        // deltas past the cap were not emitted at all.
        let emitted = events.lock().unwrap();
        let task_id = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskStarted { task_id, kind, .. }
                    if kind == "agent" =>
                {
                    Some(task_id.clone())
                }
                _ => None,
            })
            .expect("subagent task.started emitted");
        let deltas: Vec<String> = emitted
            .iter()
            .filter_map(|event| match event {
                crate::events::EngineEvent::TaskOutput {
                    task_id: id,
                    delta,
                } if id == &task_id => Some(delta.clone()),
                _ => None,
            })
            .collect();
        let settled = emitted
            .iter()
            .find_map(|event| match event {
                crate::events::EngineEvent::TaskSettled {
                    task_id: id,
                    output,
                    ..
                } if id == &task_id => Some(output.clone()),
                _ => None,
            })
            .expect("subagent task.settled emitted");
        assert_eq!(
            deltas.len(),
            2,
            "only the deltas within the budget are emitted: {deltas:?}"
        );
        assert_eq!(
            deltas.concat(),
            settled,
            "capped deltas must still concatenate exactly to the capped settle output: {deltas:?} / {settled:?}"
        );
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
                cancel: None,
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
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
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
                cancel: None,
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
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
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
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
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
