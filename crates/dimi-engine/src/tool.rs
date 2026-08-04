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
#[derive(Debug, Clone, Default)]
pub struct ToolContext {
    pub cwd: String,
    pub shell: String,
    /// The full assistant-message batch of tool calls this call is part of
    /// (the TS `ToolResolutionContext.toolCalls` equivalent): external
    /// (TS-side) tools need the sibling calls to enforce same-round
    /// validations (AllDone's "must be the only tool call in its round").
    /// `ToolCall` here is the crate's own model — `name` + parsed `arguments`
    /// are what the bridge serializes into the external callback payload.
    pub tool_calls: Vec<ToolCall>,
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

        // An explicit empty `cwd` is invalid (spawn would fail with ENOENT);
        // fall back to the context cwd, mirroring the engine tool_ctx guard.
        let cwd = match args.cwd.as_deref() {
            Some(cwd) if !cwd.is_empty() => cwd,
            _ => ctx.cwd.as_str(),
        };
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

/// Truncate `text` to at most `cap` BYTES without splitting a multi-byte
/// UTF-8 character — the single truncation path for every site that enforces
/// the `DEFAULT_MAX_CHARS` output budget via `String::len()` (i.e. bytes):
/// the backgrounded-bash seed, `append_task_output*`, and the subagent
/// accumulator. Iterating chars keeps the result on a char boundary, so the
/// truncated prefix is always valid UTF-8 and delta == settle holds
/// byte-for-byte.
fn truncate_utf8_to_bytes(text: &str, cap: usize) -> String {
    let mut capped = String::new();
    for ch in text.chars() {
        if capped.len() + ch.len_utf8() > cap {
            break;
        }
        capped.push(ch);
    }
    capped
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
    // F4.4 (review note): each of the two buffers can hold 50k chars, so
    // stdout + stderr combined can reach ~100k — cap the seed at
    // DEFAULT_MAX_CHARS BYTES (the cap the OutputBuffer and the poller's
    // `append_task_output_and_emit` enforce via `String::len()`) BEFORE it
    // seeds `state.output` and becomes the first `task.output` delta.
    // Truncating the seed (not the stream) keeps delta == settle and the TS
    // adapter's byte-offset tail arithmetic intact. A char-based cap would
    // let a 50k-char seed of 3-4-byte chars reach ~150-200k bytes, so the
    // truncation is byte-based — and never splits a multi-byte char.
    foreground_output = truncate_utf8_to_bytes(&foreground_output, DEFAULT_MAX_CHARS);

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
            // Bash tasks enforce their own background deadline; the
            // TaskState deadline defaults to the subagent timeout (unused on
            // this path).
            deadline: std::time::Instant::now() + DEFAULT_SUBAGENT_TIMEOUT,
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
                    // The TaskStop reason threaded from the bridge
                    // (`cancel_task(taskId, reason)` → the per-task cancel
                    // signal); falls back to the TS default when the signal
                    // carried none.
                    state.error = Some(
                        state
                            .cancel
                            .as_ref()
                            .and_then(|cancel| cancel.reason())
                            .unwrap_or_else(|| "Stopped by TaskStop".to_string()),
                    );
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
/// result-builder byte limit (mirrors the TS task service's retained output
/// cap so a chatty background command cannot grow unbounded). The byte cap is
/// enforced by `truncate_utf8_to_bytes` on the remaining budget, so a
/// multi-byte char straddling the boundary is never split and
/// `state.output` stays within `DEFAULT_MAX_CHARS` bytes.
fn append_task_output(state: &mut TaskState, chunk: &[u8]) {
    if state.output.len() >= DEFAULT_MAX_CHARS {
        return;
    }
    let text = String::from_utf8_lossy(chunk);
    let remaining = DEFAULT_MAX_CHARS - state.output.len();
    state
        .output
        .push_str(&truncate_utf8_to_bytes(&text, remaining));
}

/// Append a drained chunk and emit a `task.output` event with exactly the
/// appended delta, so the TS side can stream live output into TaskOutput
/// while the task is still running (TS ProcessTask parity). The delta is the
/// same byte-capped substring `append_task_output` appends, so the sum
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
    let delta: String = truncate_utf8_to_bytes(&text, remaining);
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
                    tool_calls: vec![],
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert!(result.output.contains("hello"), "output: {}", result.output);
    }

    #[tokio::test]
    async fn bash_tool_runs_with_bash_in_real_cwd() {
        // Regression probe: the production ToolContext uses `default_shell()`
        // (bash on macOS) and a real workspace cwd, not `/bin/sh` + temp.
        let tool = BashTool::default();
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "echo hello"}),
                },
                &ToolContext {
                    cwd: "/Users/zuozijian/projects/k-3720".to_string(),
                    shell: "/bin/bash".to_string(),
                    tool_calls: vec![],
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert!(result.output.contains("hello"), "output: {}", result.output);
    }

    #[tokio::test]
    async fn bash_tool_falls_back_when_explicit_cwd_is_empty() {
        // Regression: an explicit empty `cwd` ("" — e.g. a profile whose
        // session cwd resolved to empty) used to spawn with
        // `current_dir("")`, which fails with ENOENT
        // ("No such file or directory (os error 2)") — every Bash call
        // failed. The empty string must fall back to the context cwd.
        let tool = BashTool::default();
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "echo empty-cwd-ok", "cwd": ""}),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                    tool_calls: vec![],
                },
            )
            .await;
        assert!(!result.is_error, "output: {}", result.output);
        assert!(result.output.contains("empty-cwd-ok"), "output: {}", result.output);
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
                    tool_calls: vec![],
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
                    tool_calls: vec![],
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
                    tool_calls: vec![],
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
                    tool_calls: vec![],
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
                tool_calls: vec![],
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
                tool_calls: vec![],
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
                    tool_calls: vec![],
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
                tool_calls: vec![],
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
                    tool_calls: vec![],
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
                tool_calls: vec![],
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
                    tool_calls: vec![],
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
                tool_calls: vec![],
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
                    tool_calls: vec![],
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
                tool_calls: vec![],
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
    async fn bash_tool_cancel_carries_custom_stop_reason() {
        // F3.5 (adversarial review nit): the TaskStop reason threaded from
        // the bridge (`cancel_task(taskId, reason)` → the per-task cancel
        // signal) must land on the killed settle's error — not the hardcoded
        // "Stopped by TaskStop" fallback. A custom reason here proves the
        // bash kill path propagates it end-to-end.
        let tasks = AgentTasks::new();
        let tool = BashTool::with_tasks(tasks.clone());
        let result = tool
            .execute(
                &ToolCall {
                    id: "call_bg_reason".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        "command": "sleep 30",
                        "timeout": 1,
                        "description": "reason probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                    tool_calls: vec![],
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
        state
            .cancel
            .as_ref()
            .expect("cancel signal")
            .cancel_with_reason(Some("user abort".to_string()));

        let waited = WaitForTool {
            tasks: tasks.clone(),
        }
        .execute(
            &ToolCall {
                id: "call_wr".to_string(),
                name: "WaitFor".to_string(),
                arguments: serde_json::json!({ "agent_id": task_id, "timeout_seconds": 20 }),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
                tool_calls: vec![],
            },
        )
        .await;
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(waited_json["status"], "killed");
        assert_eq!(
            waited_json["error"].as_str(),
            Some("user abort"),
            "the TaskStop reason must reach the killed settle: {}",
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
                    tool_calls: vec![],
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
                tool_calls: vec![],
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
    async fn bash_tool_backgrounded_foreground_seed_is_capped_at_default_max_chars() {
        // F4.4 (review note): stdout and stderr each hold up to 50k chars, so
        // the backgrounded task's seeded `state.output` (stdout + '\n' +
        // stderr) could reach ~100k chars — past the nominal cap. The seed
        // must be truncated to DEFAULT_MAX_CHARS BYTES (matching the
        // byte-based `OutputBuffer` / `append_task_output_and_emit` caps)
        // BEFORE it becomes `state.output` and the first `task.output`
        // delta, keeping the delta == settle invariant the TS adapter's tail
        // arithmetic relies on. (`yes` keeps lines short so the 2k per-line
        // cap does not pre-truncate; each buffer holds 40k chars, combined
        // 80_001.)
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
                    id: "call_bg_cap".to_string(),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({
                        // 40k stdout + 40k stderr + the '\n' separator =
                        // 80_001 chars before the 1s timeout; sleep 2 keeps
                        // it running so it is backgrounded.
                        "command": "yes a | head -c 40000; yes b | head -c 40000 >&2; sleep 2",
                        "timeout": 1,
                        "description": "seed cap probe",
                    }),
                },
                &ToolContext {
                    cwd: std::env::temp_dir().to_string_lossy().to_string(),
                    shell: "/bin/sh".to_string(),
                    tool_calls: vec![],
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
                id: "call_wcap".to_string(),
                name: "WaitFor".to_string(),
                arguments: serde_json::json!({ "agent_id": task_id, "timeout_seconds": 20 }),
            },
            &ToolContext {
                cwd: std::env::temp_dir().to_string_lossy().to_string(),
                shell: "/bin/sh".to_string(),
                tool_calls: vec![],
            },
        )
        .await;
        assert!(!waited.is_error, "output: {}", waited.output);
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
        assert_eq!(
            settled.len(),
            DEFAULT_MAX_CHARS,
            "the seeded foreground output must be capped at DEFAULT_MAX_CHARS bytes: {}",
            settled.len()
        );
        // The seed is stdout + '\n' + stderr truncated to the cap: stdout's
        // 40k 'a' lines, the separator, then the first 9_999 bytes of
        // stderr's 'b' lines (the truncation cuts mid-stream, not at a
        // buffer boundary).
        assert!(
            settled.starts_with("a\na\na"),
            "stdout must lead the seed: {}",
            &settled[..20]
        );
        assert!(
            settled.contains("\nb\nb\nb"),
            "the separator and stderr must follow stdout"
        );
        assert!(settled.ends_with('b'), "stderr fills the final budget");
        assert!(
            !deltas.is_empty() && deltas[0] == settled,
            "the truncated seed must be the first delta: {deltas:?}"
        );
        assert_eq!(
            deltas.concat(),
            settled,
            "streamed deltas must concatenate exactly to the capped settle output"
        );
    }

    #[test]
    fn seed_truncation_is_byte_capped_and_never_splits_a_char() {
        // The seed cap is byte-based (matching OutputBuffer /
        // append_task_output, which compare `String::len()`): under a
        // char-based cap, 50k three-byte chars would reach 150k bytes. The
        // byte cap must still land on a char boundary so the result is valid
        // UTF-8 and delta == settle holds.
        let text = "界".repeat(60_000); // 180_000 bytes
        let capped = truncate_utf8_to_bytes(&text, DEFAULT_MAX_CHARS);
        assert_eq!(
            capped.len(),
            DEFAULT_MAX_CHARS - (DEFAULT_MAX_CHARS % 3),
            "a multi-byte seed must be cut at the char boundary below the cap"
        );
        assert_eq!(capped, "界".repeat(capped.len() / 3));
        // ASCII: bytes == chars, so the full budget is used.
        assert_eq!(
            truncate_utf8_to_bytes(&"a".repeat(DEFAULT_MAX_CHARS + 100), DEFAULT_MAX_CHARS).len(),
            DEFAULT_MAX_CHARS
        );
        // A 3-byte char straddling the boundary must not be split: one byte
        // of budget left is not enough for '界'.
        let mixed = format!("{}{}", "a".repeat(DEFAULT_MAX_CHARS - 1), "界");
        let capped = truncate_utf8_to_bytes(&mixed, DEFAULT_MAX_CHARS);
        assert_eq!(capped.len(), DEFAULT_MAX_CHARS - 1);
        assert!(!capped.ends_with('界'));
        // Within the cap: returned unchanged.
        let short = "界".repeat(10);
        assert_eq!(truncate_utf8_to_bytes(&short, DEFAULT_MAX_CHARS), short);
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
                    tool_calls: vec![],
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
                tool_calls: vec![],
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

    /// Update the LLM-facing definition of an already-registered tool without
    /// replacing its executor (the Rust-native tools register executor-first
    /// and the bridge advertises their defs afterwards). Returns `false` when
    /// no tool with that name is registered.
    pub fn set_def(&mut self, name: &str, def: Option<serde_json::Value>) -> bool {
        if !self.tools.contains_key(name) {
            return false;
        }
        match def {
            Some(def) => {
                self.defs.insert(name.to_string(), def);
            }
            None => {
                self.defs.remove(name);
            }
        }
        true
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

    /// Forward cancellation to the registered executor (P1-2 review): the
    /// engine cancels through the registry (bridge path), so a no-op here
    /// would orphan a running Bash command when the turn is cancelled.
    fn abort(&self, call: &ToolCall) {
        if let Some(tool) = self.tools.get(&call.name) {
            tool.abort(call);
        }
    }
}

#[cfg(test)]
mod tool_registry_tests {
    use super::*;

    #[test]
    fn set_def_advertises_a_def_for_an_executor_registered_without_one() {
        let mut registry = ToolRegistry::new();
        registry.register("Native", Box::new(BashTool::default()));
        assert!(
            registry.tool_defs().is_empty(),
            "executor-only registration advertises nothing"
        );

        assert!(registry.set_def(
            "Native",
            Some(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "Native",
                    "description": "A native tool",
                    "parameters": { "type": "object", "properties": {} },
                }
            })),
        ));
        let defs = registry.tool_defs();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["function"]["name"], "Native");
        assert_eq!(defs[0]["function"]["description"], "A native tool");
        assert_eq!(defs[0]["function"]["parameters"]["type"], "object");

        // Removing the def hides the tool again; unknown names are rejected.
        assert!(registry.set_def("Native", None));
        assert!(registry.tool_defs().is_empty());
        assert!(!registry.set_def("Missing", Some(serde_json::json!({}))));
    }

    #[tokio::test]
    async fn registry_abort_forwards_to_the_registered_tool() {
        // P1-2 (review): turn cancellation calls `tools.abort(call)` — the
        // registry must forward it to the inner executor (e.g. BashTool's
        // process-tree kill) instead of the trait's no-op default, or a
        // cancelled Bash command keeps running as an orphan.
        #[async_trait::async_trait]
        impl ToolExecutor for AbortRecordingTool {
            async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolResult {
                ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: "ran".to_string(),
                    is_error: false,
                    stop_turn: false,
                    updates: vec![],
                }
            }
            fn abort(&self, call: &ToolCall) {
                self.aborted.lock().unwrap().push(call.id.clone());
            }
        }
        struct AbortRecordingTool {
            aborted: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        }

        let aborted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut registry = ToolRegistry::new();
        registry.register(
            "Bash",
            Box::new(AbortRecordingTool {
                aborted: std::sync::Arc::clone(&aborted),
            }),
        );
        registry.abort(&ToolCall {
            id: "call_1".to_string(),
            name: "Bash".to_string(),
            arguments: serde_json::json!({}),
        });
        assert_eq!(
            *aborted.lock().unwrap(),
            vec!["call_1".to_string()],
            "registry.abort must reach the registered tool"
        );
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
    /// Wall-clock deadline for the nested subagent turn (TS
    /// `resolveSubagentTimeoutMs` parity, default 2h): when the worker's
    /// `tokio::time::timeout_at` fires, the task settles `timed_out`.
    pub deadline: std::time::Instant,
}

impl AgentTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of tasks still in `running` status (the engine's view of the
    /// TS task service's active detached-task count — `max_running_tasks`
    /// is enforced against it before a subagent launch).
    pub fn running_count(&self) -> usize {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.values().filter(|state| state.status == "running").count()
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
    /// wins; otherwise the newest (largest `started_at`) is returned.
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
    /// The parent turn's tool definitions (`EngineTurnInput.tools`): nested
    /// subagent turns must advertise the same tool set to their LLM, or the
    /// subagent model never learns the tools exist (it then claims there are
    /// no file-reading tools etc.). The nested input is built with these.
    ///
    /// Shared with the napi session: the session's tool registry is populated
    /// AFTER construction (TS hands an empty `tools` array and the bridge
    /// re-syncs the defs before every run/resume), so a snapshot taken at
    /// construction would stay empty forever. The bridge writes the synced
    /// set into this shared cell and the subagent reads the latest defs at
    /// launch time.
    pub tools_defs: std::sync::Arc<std::sync::Mutex<Vec<crate::types::EngineTool>>>,
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
    /// Dedicated LLM client for nested subagent turns, built from the turn
    /// input's `subagent_model` when it differs from the parent's provider
    /// (TS `resolveSubagentBinding` parity — a subagent bound to the
    /// secondary model runs on its own client). `None` = subagents reuse the
    /// parent's client.
    pub subagent_llm: Option<Arc<dyn LlmClient>>,
    /// The resolved subagent provider config (same Some/None condition as
    /// `subagent_llm`): the nested turn's input provider, so its
    /// `ChatRequest.model` / thinking effort carry the subagent binding.
    pub subagent_provider: Option<crate::types::ProviderConfig>,
    /// Allowed `subagent_type` values (TS `subagentAllowlistFor` parity);
    /// `None` = unrestricted.
    pub subagent_allowlist: Option<Vec<String>>,
    /// Per-subagent timeout in milliseconds (TS `resolveSubagentTimeoutMs`);
    /// `None` = `DEFAULT_SUBAGENT_TIMEOUT_MS` (2h).
    pub subagent_timeout_ms: Option<u64>,
    /// Max concurrently running background tasks (TS `task.maxRunningTasks`);
    /// `None` = unlimited.
    pub max_running_tasks: Option<u32>,
}

/// Default per-subagent timeout: 2 hours, same as v1 / TS
/// `DEFAULT_SUBAGENT_TIMEOUT_MS`.
pub const DEFAULT_SUBAGENT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(2 * 60 * 60);

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
    // Tool definitions advertised to the nested LLM (the parent's tool set).
    tools_defs: Vec<crate::types::EngineTool>,
    policy: crate::permission::PolicyConfig,
    max_steps: Option<u32>,
    provider: crate::types::ProviderConfig,
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
        tools: tools_defs,
        active_tools: None,
        provider,
        max_steps_per_turn: max_steps,
        max_retries_per_step: None,
        cwd: Some(cwd),
        shell: Some(shell),
        context_window: None,
        max_context_tokens: None,
        next_agent_id: None,
        kill_grace_ms: None,
        subagent_model: None,
        subagent_allowlist: None,
        subagent_timeout_ms: None,
        max_running_tasks: None,
        completion_review: None,
        origin: dimi_wire::model::TurnOrigin::User { payload: None },
        uses_worker_rejection_guidance: false,
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
            if outcome.status == crate::types::TurnEndReason::Completed
                && outcome.truncated != Some(true) =>
        {
            (text, String::new(), messages)
        }
        crate::engine::TurnProgress::Completed(outcome) => {
            // A provider-truncated nested turn (finish reason length /
            // max_tokens) is a FAILED subagent — TS parity
            // (`runAgentTurn.classifyTurnResult` throws
            // `SUBAGENT_MAX_TOKENS_ERROR` on `result.truncated`).
            let error = if outcome.truncated == Some(true) {
                "Subagent turn failed before completing its final summary: reason=max_tokens"
                    .to_string()
            } else {
                outcome.error.unwrap_or_else(|| "failed".to_string())
            };
            (String::new(), error, messages)
        }
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
                    // TS parity: an Agent call never ends the caller's turn —
                    // the subagent runs in the background and the caller
                    // continues with other work.
                    stop_turn: false,
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

        // TS `subagentAllowlistFor` parity: an explicitly requested
        // `subagent_type` outside the caller's allowlist is rejected before
        // launch (TS `subagentTypeNotAllowedMessage`).
        let subagent_type = call
            .arguments
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !subagent_type.is_empty() {
            if let Some(allowlist) = &self.subagent_allowlist {
                if !allowlist.iter().any(|allowed| allowed == &subagent_type) {
                    let allowed = if allowlist.is_empty() {
                        "none".to_string()
                    } else {
                        allowlist.join(", ")
                    };
                    return ToolResult {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        output: format!(
                            "Subagent type \"{subagent_type}\" is not allowed for this agent. Allowed subagent types: {allowed}."
                        ),
                        is_error: true,
                        stop_turn: false,
                        updates: vec![],
                    };
                }
            }
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

    /// The nested turn's input provider: the resolved subagent model when the
    /// tool has a dedicated subagent client, else the placeholder provider
    /// (the nested turn reuses the parent's client, whose real model config
    /// lives on the parent session; the client ignores the placeholder).
    fn subagent_model_provider(&self) -> crate::types::ProviderConfig {
        self.subagent_provider.clone().unwrap_or(crate::types::ProviderConfig {
            base_url: "http://nested".to_string(),
            api_key: "nested".to_string(),
            model: "nested".to_string(),
            thinking_effort: None,
        })
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
        // TS `task.maxRunningTasks` parity: a launch beyond the running-task
        // cap fails immediately and occupies no slot (the check happens
        // BEFORE the insert).
        if let Some(max_running) = self.max_running_tasks {
            if self.tasks.running_count() >= max_running as usize {
                return ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: "Too many background tasks are already running.".to_string(),
                    is_error: true,
                    stop_turn: false,
                    updates: vec![],
                };
            }
        }
        // TS `SubagentTask.idPrefix = "agent"` parity: the wire task id for a
        // subagent is `agent-<8>` (bash background tasks keep `bash-<8>`).
        let task_id = format!("agent-{}", uuid_v4_short());
        let cancel = std::sync::Arc::new(crate::engine::CancelSignal::new());
        // TS `resolveSubagentTimeoutMs` parity: the nested turn must settle
        // within this deadline or the worker settles the task `timed_out`.
        let timeout = self
            .subagent_timeout_ms
            .map(std::time::Duration::from_millis)
            .unwrap_or(DEFAULT_SUBAGENT_TIMEOUT);
        let deadline = std::time::Instant::now() + timeout;
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
                deadline,
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
        // Subagent model parity: when the turn input resolved a dedicated
        // subagent model (secondary-model binding), the nested turn runs on
        // its own client and its input provider carries the resolved config;
        // otherwise it reuses the parent's client and the placeholder
        // provider (the client ignores the placeholder's model).
        let llm = match &self.subagent_llm {
            Some(subagent_llm) => Arc::clone(subagent_llm),
            None => Arc::clone(&self.llm),
        };
        let nested_provider = self.subagent_model_provider();
        let tools = Arc::clone(&self.tools);
        // Copy the parent's tool definitions out of `&self` before the worker
        // spawn (the nested turn advertises the same tool set to its LLM).
        let tools_defs = self
            .tools_defs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
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
        // with an empty output). Capped at DEFAULT_MAX_CHARS BYTES (F4): the
        // same limit the bash path enforces (`append_task_output_and_emit`), so a
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
            // at DEFAULT_MAX_CHARS BYTES no further delta is appended OR
            // emitted, and a delta that would cross the cap is truncated to
            // the remaining byte budget via `truncate_utf8_to_bytes` (never
            // splitting a multi-byte char) and emitted with exactly the
            // appended substring — so `sum(emitted deltas) == accumulated ==
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
                        let capped: String = truncate_utf8_to_bytes(delta, remaining);
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
            let nested = tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                run_nested_turn(
                    history,
                    prompt,
                    cwd,
                    shell,
                    llm,
                    tools,
                    tools_defs,
                    policy,
                    max_steps,
                    nested_provider,
                    Some(steer_queue),
                    std::sync::Arc::clone(&cancel),
                    on_delta,
                ),
            )
            .await;
            // Session torn down while the nested turn ran (TS
            // `taskService.dispose` parity): skip the settle + notification —
            // the sink is closed, so nothing would reach the runner anyway.
            // Mark the task non-running and drop its full message history so
            // the shared registry cannot hold a permanently-"running" entry
            // (review P1-2: boundedness + privacy).
            if events.is_closed() {
                tasks.update(&task_id_for_worker, |state| {
                    state.status = "killed".to_string();
                    state.error = Some("Agent disposed before the subagent settled".to_string());
                    state.messages = Vec::new();
                });
                return;
            }
            let (_output, error, messages) = match nested {
                Ok(nested) => nested,
                Err(_elapsed) => {
                    // TS `resolveSubagentTimeoutMs` parity: the nested turn
                    // exceeded its deadline. Flip the per-task cancel so the
                    // dropped nested session's own bash pollers observe the
                    // cancellation and kill their processes, then settle the
                    // task `timed_out` (the wire status the TS runner's
                    // notification builder renders as "<task> timed out.").
                    cancel.cancel_with_reason(Some("Subagent timed out".to_string()));
                    tasks.update(&task_id_for_worker, |state| {
                        state.status = "timed_out".to_string();
                        state.error = Some(format!(
                            "Subagent timed out after {} seconds",
                            timeout.as_secs().max(1)
                        ));
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
                    return;
                }
            };
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
                    // The settle error carries the TaskStop reason threaded
                    // from the bridge (fallback: the TS default string).
                    state.status = "killed".to_string();
                    state.error = Some(
                        cancel
                            .reason()
                            .unwrap_or_else(|| "Stopped by TaskStop".to_string()),
                    );
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
            // TS parity: an Agent call never ends the caller's turn — the
            // subagent runs in the background and the caller continues with
            // other work (the old `stop_turn: true` yielded the turn).
            stop_turn: false,
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

/// `WaitForTool` — wait for a background subagent task to finish (or
/// timeout). Always ends the turn after executing (`stop_turn: true`),
/// mirroring the TS `waitForTool.ts` `stopTurn: true`: WaitFor is a
/// deliberate turn stop, and the next turn resumes. NOTE (P1-2): this engine
/// tool waits on a SUBAGENT task by `agent_id` only — the TS user-wait /
/// notification-wake parking semantics are NOT implemented here, and the def
/// the runner advertises says so explicitly.
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
            .trim()
            .to_string();
        let timeout = normalize_wait_timeout(
            call.arguments
                .get("timeout_seconds")
                .and_then(|v| v.as_u64()),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
        // P1-2 (review): the advertised def REQUIRES `agent_id`, so a call
        // with a missing/unknown id is a model error — fail fast instead of
        // parking for the full timeout. The old behavior turned the schema
        // gap into a guaranteed ~60s blind wait: the def had no agent_id, the
        // model called without one, the lookup always missed, and WaitFor
        // blocked the whole turn before returning "Wait expired". A task that
        // EXISTS but is still running keeps the wait loop below (the
        // legitimate timeout path).
        if agent_id.is_empty() {
            return ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: "WaitFor requires a non-empty `agent_id` of a subagent task launched by the Agent tool."
                    .to_string(),
                is_error: true,
                stop_turn: true,
                updates: vec![],
            };
        }
        if self.tasks.find_by_agent_id(&agent_id).is_none() {
            return ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: format!("No subagent task found for agent_id: {agent_id}"),
                is_error: true,
                stop_turn: true,
                updates: vec![],
            };
        }
        loop {
            if let Some(state) = self.tasks.find_by_agent_id(&agent_id) {
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
                        stop_turn: true,
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
                    stop_turn: true,
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
            tool_calls: vec![],
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
        // Agent launches are non-blocking (same-turn continue parity): the
        // launching turn keeps going instead of stopping.
        assert!(!launch.stop_turn);

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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
    async fn waitfor_missing_agent_fails_fast() {
        // P1-2 (review): the WaitFor def the runner advertises REQUIRES
        // `agent_id`, so a call with an unknown agent id (or none at all) is
        // a model error — it must fail immediately instead of parking for
        // the full timeout. The old behavior returned "Wait expired" after a
        // guaranteed ~60s blind wait: the def had no agent_id, the model
        // called without one, and the registry lookup always missed.
        let tasks = AgentTasks::new();
        let wait = WaitForTool { tasks };
        let started = std::time::Instant::now();
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
        let elapsed = started.elapsed();
        assert!(waited.is_error, "output: {}", waited.output);
        assert!(
            waited.output.contains("agent-missing"),
            "output: {}",
            waited.output
        );
        // Fail fast — well under even the 1s timeout the old code would
        // have slept through.
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "missing agent must fail fast, took {elapsed:?}"
        );
        // An error is still a turn stop (TS `stopTurn: true` parity).
        assert!(waited.stop_turn);
    }

    #[tokio::test]
    async fn waitfor_running_task_still_times_out() {
        // The legitimate timeout path survives the P1-2 fast-fail: a task
        // that EXISTS but never leaves `running` still waits out its
        // `timeout_seconds` and reports "Wait expired" (is_error: false).
        let tasks = AgentTasks::new();
        tasks.insert(
            "task-slow".to_string(),
            TaskState {
                agent_id: "agent-slow".to_string(),
                status: "running".to_string(),
                output: String::new(),
                error: None,
                messages: vec![],
                started_at: 1,
                deadline: std::time::Instant::now(),
                cancel: None,
            },
        );
        let wait = WaitForTool { tasks };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": "agent-slow", "timeout_seconds": 1 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error);
        assert!(waited.output.contains("Wait expired"));
        // Timeout is also a turn stop (TS `stopTurn: true` parity).
        assert!(waited.stop_turn);
    }

    #[tokio::test]
    async fn waitfor_stops_the_turn_when_task_completes() {
        // TS parity: waitForTool.ts builds the result with `stopTurn: true` —
        // WaitFor is a deliberate turn stop (wait for the user/task, resume
        // on the next turn), so the engine must end the turn after executing.
        let tasks = AgentTasks::new();
        tasks.insert(
            "task-done".to_string(),
            TaskState {
                agent_id: "agent-done".to_string(),
                status: "completed".to_string(),
                output: "done".to_string(),
                error: None,
                messages: vec![],
                started_at: 1,
                deadline: std::time::Instant::now(),
                cancel: None,},
        );
        let wait = WaitForTool { tasks };
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": "agent-done", "timeout_seconds": 1 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);
        let waited_json: serde_json::Value = serde_json::from_str(&waited.output).unwrap();
        assert_eq!(waited_json["status"], "completed");
        assert_eq!(waited_json["output"], "done");
        assert!(waited.stop_turn);
    }

    #[tokio::test]
    async fn waitfor_rejects_a_task_id_instead_of_agent_id() {
        // P2-4 (final review): the task registry is keyed by task id, but
        // the WaitFor def REQUIRES `agent_id` — passing a task id must fail
        // fast ("No subagent task found for agent_id: ...") instead of
        // accidentally matching the task via the raw key and parking for
        // the full timeout.
        let tasks = AgentTasks::new();
        tasks.insert(
            "task-xyz".to_string(),
            TaskState {
                agent_id: "agent-xyz".to_string(),
                status: "running".to_string(),
                output: String::new(),
                error: None,
                messages: vec![],
                started_at: 1,
                deadline: std::time::Instant::now(),
                cancel: None,
            },
        );
        let wait = WaitForTool { tasks };
        let started = std::time::Instant::now();
        let waited = wait
            .execute(
                &ToolCall {
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": "task-xyz", "timeout_seconds": 1 }),
                },
                &ctx(),
            )
            .await;
        let elapsed = started.elapsed();
        assert!(waited.is_error, "output: {}", waited.output);
        assert!(
            waited.output.contains("task-xyz"),
            "output: {}",
            waited.output
        );
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "task id must fail fast, took {elapsed:?}"
        );
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

    #[test]
    fn default_kill_grace_ms_is_pinned_at_the_ts_default() {
        // F5 (review note): `DEFAULT_KILL_GRACE_MS` here and the TS constant
        // of the same name in `agent/task/configSection.ts` are only
        // comment-linked. The bridge always passes the runner's explicit
        // `killGraceMs` (computed from the TS constant), so this Rust value
        // is only a fallback for direct/nested construction — but if the two
        // ever diverge, this pin (plus the TS-side pin in
        // taskService.test.ts) fails and the mismatch is caught.
        assert_eq!(DEFAULT_KILL_GRACE_MS, 5_000);
    }

    #[tokio::test]
    async fn cross_session_agent_output_resolves_task_launched_by_another_session() {
        // P1-1 (review): the bridge gives every turn session a clone of ONE
        // process-level `AgentTasks`, so a subagent launched by turn N
        // (stop_turn ends that turn immediately) must be resolvable via
        // AgentOutput in turn N+1 — a DIFFERENT session holding a different
        // clone of the SAME registry. Two independent handles over one shared
        // instance: the second session's AgentOutput sees the first session's
        // launched task.
        let shared = AgentTasks::new(); // one shared instance (bridge: shared_agent_tasks)
        let session_a_tasks = shared.clone();
        let session_b_tasks = shared.clone();

        let agent = AsyncAgentTool {
            llm: Arc::new(ScriptedLlmClient::once(vec![
                LlmStreamEvent::Text {
                    delta: "cross-session output".to_string(),
                },
                LlmStreamEvent::Finish {
                    finish_reason: Some("stop".to_string()),
                },
            ])),
            tools: Arc::new(BashTool::default()),
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: session_a_tasks,
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
        assert!(!launch.is_error, "output: {}", launch.output);
        let launch_json: serde_json::Value = serde_json::from_str(&launch.output).unwrap();
        let agent_id = launch_json["agent_id"].as_str().unwrap().to_string();

        // The "next turn": a separate session's AgentOutput tool reads the
        // SAME shared registry and resolves the earlier session's task.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let read = AgentOutputTool { tasks: session_b_tasks }
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
        assert_eq!(read_json["agent_id"], agent_id);
        assert_eq!(read_json["status"], "completed");
        assert_eq!(read_json["output"], "cross-session output");
    }

    #[tokio::test]
    async fn subagent_request_advertises_parent_tools() {
        // Regression: nested subagent turns built their EngineTurnInput with
        // `tools: vec![]`, so the subagent's LLM request carried no tool
        // definitions — the subagent model never learned the tools exist (it
        // then claimed there are no file-reading tools etc.). The nested
        // turn must advertise the parent's tool set.
        use crate::llm::{ChatRequest, LlmClient, LlmError, StreamedTurn};
        use crate::events::EventSink;
        struct RecordingClient(std::sync::Mutex<Vec<Option<Vec<serde_json::Value>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, LlmError> {
                self.0.lock().unwrap().push(request.tools.clone());
                Ok(StreamedTurn {
                    events: vec![
                        LlmStreamEvent::Text {
                            delta: "done".to_string(),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("stop".to_string()),
                        },
                    ],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "done".to_string(),
                        thinking: String::new(),
                    },
                })
            }
        }
        let recorded = std::sync::Arc::new(RecordingClient(std::sync::Mutex::new(Vec::new())));
        let tasks = AgentTasks::new();
        let agent = AsyncAgentTool {
            llm: std::sync::Arc::clone(&recorded) as Arc<dyn LlmClient>,
            tools: Arc::new(BashTool::default()),
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![crate::types::EngineTool {
                name: "Read".to_string(),
                description: "Read a file".to_string(),
                args_schema: serde_json::json!({"type": "object", "properties": {}}),
            }])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
        };
        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({"prompt": "read something"}),
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
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id, "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);

        let requests = recorded.0.lock().unwrap();
        assert!(!requests.is_empty(), "nested turn must issue an LLM request");
        let tools = requests[0].as_ref().expect("nested request must carry tools");
        assert!(
            tools.iter().any(|t| t["name"] == "Read"),
            "nested request must advertise the parent tool set: {tools:?}"
        );
    }

    /// The napi session constructs AsyncAgentTool BEFORE the tool registry is
    /// populated (TS hands an empty `tools` array; the bridge re-syncs the
    /// defs on every run/resume). The shared tools_defs cell must reflect the
    /// synced set at subagent launch time — a construction-time snapshot
    /// would advertise an empty tool set and the subagent model would fall
    /// back to made-up tool formats.
    #[tokio::test]
    async fn subagent_advertises_tools_synced_after_construction() {
        use crate::llm::{ChatRequest, LlmClient, LlmError, StreamedTurn};
        use crate::events::EventSink;
        struct RecordingClient(std::sync::Mutex<Vec<Option<Vec<serde_json::Value>>>>);
        #[async_trait::async_trait]
        impl LlmClient for RecordingClient {
            async fn stream_chat(
                &self,
                request: &ChatRequest,
            ) -> Result<StreamedTurn, LlmError> {
                self.0.lock().unwrap().push(request.tools.clone());
                Ok(StreamedTurn {
                    events: vec![
                        LlmStreamEvent::Text {
                            delta: "done".to_string(),
                        },
                        LlmStreamEvent::Finish {
                            finish_reason: Some("stop".to_string()),
                        },
                    ],
                    assistant: crate::llm::AssistantTurn {
                        tool_calls: vec![],
                        text: "done".to_string(),
                        thinking: String::new(),
                    },
                })
            }
        }
        let recorded = std::sync::Arc::new(RecordingClient(std::sync::Mutex::new(Vec::new())));
        let tasks = AgentTasks::new();
        // Construction-time cell: EMPTY (the napi session receives `tools: []`
        // from TS; the registry is populated later).
        let tools_defs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let agent = AsyncAgentTool {
            llm: std::sync::Arc::clone(&recorded) as Arc<dyn LlmClient>,
            tools: Arc::new(BashTool::default()),
            tools_defs: std::sync::Arc::clone(&tools_defs),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
        };
        // Bridge run()/resume() re-sync: the shared cell now holds the real
        // tool set.
        *tools_defs.lock().unwrap() = vec![crate::types::EngineTool {
            name: "Bash".to_string(),
            description: "Run a command".to_string(),
            args_schema: serde_json::json!({"type": "object", "properties": {}}),
        }];

        let launch = agent
            .execute(
                &ToolCall {
                    id: "call_a".to_string(),
                    name: "Agent".to_string(),
                    arguments: serde_json::json!({"prompt": "read something"}),
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
                    id: "call_w".to_string(),
                    name: "WaitFor".to_string(),
                    arguments: serde_json::json!({ "agent_id": agent_id, "timeout_seconds": 10 }),
                },
                &ctx(),
            )
            .await;
        assert!(!waited.is_error, "output: {}", waited.output);

        let requests = recorded.0.lock().unwrap();
        assert!(!requests.is_empty(), "nested turn must issue an LLM request");
        let tools = requests[0].as_ref().expect("nested request must carry tools");
        assert!(
            tools.iter().any(|t| t["name"] == "Bash"),
            "nested request must advertise the post-construction synced tool set: {tools:?}"
        );
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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

        // TaskStop the subagent mid-run (per-task cancel parity). The custom
        // reason must land on the killed settle's error (F3.5: the bridge
        // threads the TS stop reason through the per-task cancel signal).
        let (_, state) = tasks
            .find_by_agent_id_with_key(&agent_id)
            .expect("subagent task registered");
        state
            .cancel
            .as_ref()
            .expect("subagent has a per-task cancel signal")
            .cancel_with_reason(Some("user abort".to_string()));

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
            waited_json["error"].as_str(),
            Some("user abort"),
            "the TaskStop reason must reach the killed settle: {waited_json}"
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            // One step only: after step 1 (streamed text + Bash tool call)
            // the max-steps guard fails the turn.
            max_steps: Some(1),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
        // The cap is BYTE-based (`String::len()`, like the bash path): the
        // over-budget delta is 2-byte chars, so a char-based take would push
        // the settle output to 49_000 + 2_000 = 51_000 bytes; the byte cap
        // must truncate at the char boundary and stay within
        // DEFAULT_MAX_CHARS BYTES. The cap preserves the delta == settle
        // invariant: a delta that crosses the cap is truncated to the
        // remaining byte budget (appended AND emitted identically), and once
        // the cap is reached later deltas are dropped from BOTH the
        // accumulator and the emitted stream.
        let fill = "x".repeat(DEFAULT_MAX_CHARS - 1_000); // 49_000 ASCII bytes
        let over = "é".repeat(2_000); // 2_000 chars, 4_000 bytes
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: sink,
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
        // Byte-based cap (`String::len`): a char-based take of the 2-byte
        // over-delta would let the settle output reach 51_000 bytes; the
        // byte cap must keep it within DEFAULT_MAX_CHARS BYTES and land on a
        // char boundary (500 × 'é' = 1_000 bytes fills the remaining budget).
        assert!(
            output.len() <= DEFAULT_MAX_CHARS,
            "the settle output must stay within the BYTE cap ({} bytes, cap {})",
            output.len(),
            DEFAULT_MAX_CHARS
        );
        assert_eq!(
            output,
            format!("{fill}{}", "é".repeat(500)),
            "the over-budget multi-byte delta must be truncated to the remaining BYTE budget"
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
                deadline: std::time::Instant::now(),
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map,
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
                deadline: std::time::Instant::now(),
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks: tasks.clone(),
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
            tools_defs: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            policy: policy_auto(),
            max_steps: Some(3),
            shell: "/bin/sh".to_string(),
            tasks,
            steer_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            events: EventSink::new(),
            agent_id_counter: std::sync::atomic::AtomicU64::new(0),
            subagent_llm: None,
            subagent_provider: None,
            subagent_allowlist: None,
            subagent_timeout_ms: None,
            max_running_tasks: None,
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
