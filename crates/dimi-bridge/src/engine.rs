//! `RustEngine` — the M3 swap-in socket: one turn of the Rust orchestration
//! core exposed to Node.
//!
//! `start_turn` runs a full turn and returns the collected engine event
//! batch plus the outcome (the synchronous differential-suite surface).
//! `RustTurnSession` is the in-flight-turn surface: it streams every engine
//! event to the TS side as it is emitted (per-event `ThreadsafeFunction`
//! registered via `setOnEvent`) while `run`/`resume` resolve with only the
//! final progress — the TS adapter publishes each event on the existing
//! event bus as it arrives, so the transcript projection/broadcast layers
//! keep working unchanged.
//!
//! LLM injection: `scripted_segments` (JSON array of segments) selects the
//! scripted client for the differential suite; `null` selects the real
//! OpenAI-compatible client (transport lands in the slice-1 tail).

use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Status, Unknown};
use napi_derive::napi;

use dimi_engine::aimux::{AimuxLlmClient, openai_model};
use dimi_engine::events::{EngineEvent, EventSink};
use dimi_engine::llm::{LlmClient, LlmStreamEvent, ScriptedLlmClient};
use dimi_engine::permission::{ApprovalDecision, PolicyConfig};
use dimi_engine::tool::{
    AgentOutputTool, AgentTasks, AsyncAgentTool, BashTool, ToolExecutor, ToolRegistry, WaitForTool,
};
use dimi_engine::types::EngineTurnInput;

use crate::wire_error;

/// Process-level subagent registries, scoped per agent (code-review P1-1/P1-3
/// fix): every `RustTurnSession` of the same agent shares ONE `SessionRegistry`
/// (subagent tasks + steering queues). The Agent tool no longer hardcodes
/// `stop_turn: true` — launches are non-blocking (same-turn continue), but the
/// launching turn still ends while the subagent runs (WaitFor stops the turn,
/// or a plain text reply ends it), and the NEXT turn runs in a NEW session —
/// a per-session registry would make the launched subagent invisible to
/// AgentOutput / WaitFor / Agent(resume). Scoping by an explicit registry id
/// (the TS runner passes a per-agent uuid) keeps different agents (and
/// different sessions in a server) from seeing each other's subagents — agent
/// ids like `agent-0` are only unique within one agent, so a process-wide
/// table would leak tasks across sessions. The runner calls
/// `drop_task_registry` when the agent scope is disposed.
struct SessionRegistry {
    tasks: AgentTasks,
    steer_map: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<
                String,
                std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>>,
            >,
        >,
    >,
}

static REGISTRIES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<SessionRegistry>>>,
> = std::sync::OnceLock::new();

fn session_registry(registry_id: &str) -> std::sync::Arc<SessionRegistry> {
    let map = REGISTRIES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .entry(registry_id.to_string())
        .or_insert_with(|| {
            std::sync::Arc::new(SessionRegistry {
                tasks: AgentTasks::new(),
                steer_map: std::sync::Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
            })
        })
        .clone()
}

/// Release an agent's subagent registry (tasks + steering queues) when its
/// scope is disposed. In-flight tasks settle against a registry that is no
/// longer shared; the task states are dropped with the map.
#[napi]
pub fn drop_task_registry(registry_id: String) {
    let map = REGISTRIES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.remove(&registry_id);
}

#[napi]
pub struct RustEngine {
    inner: dimi_engine::Engine,
}

#[napi]
impl RustEngine {
    #[napi(constructor)]
    pub fn new(max_steps_per_turn: Option<i32>) -> Self {
        Self {
            inner: dimi_engine::Engine {
                max_steps_per_turn: max_steps_per_turn.map(|n| n.max(0) as u32),
                max_retries_per_step: None,
                shell: dimi_exec::env::default_shell(),
            },
        }
    }

    /// Run one turn. `input_json` is an `EngineTurnInput` document;
    /// `scripted_segments_json` (optional) is a JSON array of LLM event
    /// segments for the differential suite — when absent the aimux-backed
    /// client is used. Returns an `EngineEventBatch` document
    /// (`{ events: [...], outcome: {...} }`).
    #[napi]
    pub async fn start_turn(
        &self,
        input_json: String,
        scripted_segments_json: Option<String>,
    ) -> napi::Result<String> {
        let input: EngineTurnInput = serde_json::from_str(&input_json).map_err(wire_error)?;

        let llm: Box<dyn LlmClient> = match scripted_segments_json {
            Some(segments_json) => {
                let segments: Vec<Vec<LlmStreamEvent>> =
                    serde_json::from_str(&segments_json).map_err(wire_error)?;
                Box::new(ScriptedLlmClient::new(segments))
            }
            None => Box::new(AimuxLlmClient {
                model: openai_model(&input.provider),
            }),
        };
        let tools: Box<dyn ToolExecutor> = Box::new(BashTool::kill_on_timeout());
        let policy = dimi_engine::permission::PolicyConfig {
            mode: dimi_engine::permission::PermissionMode::Manual,
            rules: vec![],
            session_approved_patterns: vec![],
        };

        let mut events: Vec<EngineEvent> = Vec::new();
        let outcome = self
            .inner
            .run_turn(
                &input,
                llm.as_ref(),
                tools.as_ref(),
                &policy,
                &mut |event| events.push(event),
            )
            .await;

        let batch = dimi_engine::events::EngineEventBatch { events, outcome };
        serde_json::to_string(&batch).map_err(wire_error)
    }
}

/// `RustTurnSession` — an in-flight Rust-engine turn with approval support.
///
/// `run()` advances the turn until it completes or needs an approval;
/// `resume(decisionJson)` continues after the user's decision. The event
/// batch JSON carries `progress`: `{status:"completed", outcome}` or
/// `{status:"needsApproval", approval}`.
type ToolCallback = ThreadsafeFunction<String, Unknown<'static>, String, Status, false>;

/// Per-event streaming sink: every engine event is pushed through this
/// callback as a JSON string, in emission order, as it happens (registered
/// via `set_on_event` before `run`/`resume`). `MaxQueueSize` is bounded so
/// the forwarder's Blocking calls block only when the napi queue is actually
/// full (with `0` = unbounded, Node's Blocking mode would wait forever).
type EventCallback = ThreadsafeFunction<
    String,
    Unknown<'static>,
    String,
    Status,
    false,
    false,
    { EVENT_TSFN_QUEUE_CAP },
>;

/// The event queue's capacity: the engine can run ahead of the JS event loop
/// by at most this many events before emitters block (backpressure). 4096
/// covers a chatty turn (assistant deltas, tool progress, task output) while
/// bounding memory.
const EVENT_QUEUE_CAP: usize = 4096;

/// The napi ThreadsafeFunction's own queue capacity: the forwarder submits
/// one event at a time (Blocking mode), so this only bounds how far the JS
/// event loop can lag behind the forwarder before it blocks — the Rust queue
/// then backs up and emitters apply backpressure.
const EVENT_TSFN_QUEUE_CAP: usize = 1024;

/// Bounded FIFO for engine events en route to the TS side (F6 fix).
///
/// The bridge previously forwarded every engine event with a NonBlocking
/// ThreadsafeFunction call and ignored the returned `Status`: under queue
/// pressure a dropped `task.output` would silently shift the TS adapter's
/// byte-offset tail arithmetic and corrupt TaskOutput's retained buffer. All
/// engine events now flow through this bounded queue into a dedicated
/// forwarding thread that calls the ThreadsafeFunction in **Blocking** mode:
///
/// - **No silent drops.** `send` waits for queue space and the forwarder
///   waits for the napi queue, so the only events discarded are the ones
///   emitted after the session is closed — the teardown behavior the old
///   `EventSink::close` path already had. Under sustained pressure the
///   system buffers (bounded) and then blocks, never dropping mid-stream.
/// - **Bounded memory.** The queue holds at most `cap` events.
/// - **FIFO.** One queue, one forwarder — event order is preserved.
///
/// The forwarder is a dedicated **std thread**, not a tokio task: a Blocking
/// TSFN call must never occupy a tokio worker that engine workers (bash
/// pollers / subagent workers) run on — on a single-worker runtime a tokio
/// forwarder blocked on the napi queue would deadlock the engine.
struct EngineEventChannel {
    queue: std::sync::Mutex<std::collections::VecDeque<EngineEvent>>,
    cap: usize,
    not_empty: std::sync::Condvar,
    not_full: std::sync::Condvar,
    /// Wakes `wait_caught_up` waiters when `delivered` advances.
    caught_up: std::sync::Condvar,
    /// Events pushed into the queue (monotonic). Guarded by `queue` (like
    /// `delivered`): `wait_caught_up` evaluates the `delivered == pushed`
    /// predicate under the queue mutex, so every counter mutation must
    /// happen under that same mutex or the check-then-wait pattern loses
    /// notifications.
    pushed: std::sync::atomic::AtomicU64,
    /// Events submitted to the ThreadsafeFunction's queue by the forwarder
    /// (monotonic; submission is guaranteed, the JS callback executing is
    /// not — Blocking TSFN calls return once the item is queued).
    /// `run`/`resume` wait until `delivered == pushed` before resolving, so
    /// every turn event is submitted to the TSFN queue BEFORE the
    /// `run`/`resume` promise continuation — the ordering the old
    /// direct-push path had. Guarded by `queue`.
    delivered: std::sync::atomic::AtomicU64,
    /// Events whose JS callback has finished handling the event. The
    /// forwarder only knows that an event was queued into the TSFN; the
    /// runner acknowledges after `handleEngineEvent` returns so approval
    /// progress cannot overtake context mirroring.
    observed: std::sync::atomic::AtomicU64,
    closed: std::sync::atomic::AtomicBool,
}

impl EngineEventChannel {
    fn new(cap: usize) -> Self {
        Self {
            queue: std::sync::Mutex::new(std::collections::VecDeque::new()),
            cap,
            not_empty: std::sync::Condvar::new(),
            not_full: std::sync::Condvar::new(),
            caught_up: std::sync::Condvar::new(),
            pushed: std::sync::atomic::AtomicU64::new(0),
            delivered: std::sync::atomic::AtomicU64::new(0),
            observed: std::sync::atomic::AtomicU64::new(0),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn pushed_count(&self) -> u64 {
        self.pushed.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn delivered_count(&self) -> u64 {
        self.delivered.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn observed_count(&self) -> u64 {
        self.observed.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Push one event, blocking until there is room (or the channel is
    /// closed). Returns `false` only when closed — the teardown path, never
    /// queue pressure.
    fn send(&self, event: EngineEvent) -> bool {
        let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        while queue.len() >= self.cap && !self.is_closed() {
            queue = self
                .not_full
                .wait(queue)
                .unwrap_or_else(|p| p.into_inner());
        }
        if self.is_closed() {
            return false;
        }
        queue.push_back(event);
        self.pushed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.not_empty.notify_one();
        true
    }

    /// Pop the oldest event, blocking until one is available (or the channel
    /// is closed). Returns `None` when closed and empty — the forwarder's
    /// exit signal.
    fn recv(&self) -> Option<EngineEvent> {
        let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        while queue.is_empty() && !self.is_closed() {
            queue = self
                .not_empty
                .wait(queue)
                .unwrap_or_else(|p| p.into_inner());
        }
        let event = queue.pop_front();
        if event.is_some() {
            self.not_full.notify_one();
        }
        event
    }

    /// The forwarder calls this after each event is submitted to the
    /// ThreadsafeFunction (successfully or not — a `Closing` status means
    /// the session is tearing down and `wait_caught_up` is already released
    /// by `close`).
    ///
    /// Takes the queue mutex so the counter update is mutually exclusive
    /// with `wait_caught_up`'s predicate evaluation: if the final
    /// `mark_delivered` could land between the predicate check and the
    /// `caught_up.wait()` registration, its notify would be lost and
    /// `run`/`resume` would hang forever (lost-wakeup; `send`/`recv` already
    /// mutate their predicate state under the queue mutex — this closes the
    /// one path that did not).
    fn mark_delivered(&self) {
        let _queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        self.delivered
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.caught_up.notify_all();
    }

    /// Block until every pushed event has been submitted to the
    /// ThreadsafeFunction (or the channel closed). Called by `run`/`resume`
    /// after the engine loop finishes, before the promise resolves.
    ///
    /// This is a blocking std-condvar wait: call it from a non-tokio thread
    /// (`spawn_blocking` in `run`/`resume`) so it never occupies a tokio
    /// worker that engine workers (bash pollers / subagent workers) run on.
    fn wait_caught_up(&self) {
        let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        while self.delivered_count() < self.pushed_count() && !self.is_closed() {
            queue = self
                .caught_up
                .wait(queue)
                .unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Mark one event as fully handled by the JS callback and wake an
    /// in-flight `wait_for_events` call.
    fn mark_observed(&self) {
        let _queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        self.observed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.caught_up.notify_all();
    }

    /// Block until all events that existed when the caller captured `target`
    /// have finished running on the JS side. Closing releases the waiter
    /// during agent teardown, where the callbacks are intentionally ignored.
    fn wait_observed(&self, target: u64) {
        let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        while self.observed_count() < target && !self.is_closed() {
            queue = self
                .caught_up
                .wait(queue)
                .unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Stop the channel: wakes every blocked sender and the forwarder; they
    /// observe `closed` and exit. Called from session `close()` / `Drop`.
    ///
    /// Takes the queue mutex like `send`/`recv`/`mark_delivered` so the
    /// `closed` state is mutually exclusive with every condvar predicate
    /// check — a `close` notifying in the window between a waiter's
    /// predicate evaluation and its `wait()` registration must not be lost
    /// either.
    fn close(&self) {
        let _queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        self.closed.store(true, std::sync::atomic::Ordering::Relaxed);
        self.not_empty.notify_all();
        self.not_full.notify_all();
        self.caught_up.notify_all();
    }
}

/// A TS-registered tool: the engine calls the napi callback, the TS side
/// executes the tool and completes the call via `completeToolCall`.
#[derive(Default)]
struct ExternalCallStore {
    /// Results received while the Rust waiter is still alive.
    pending: std::collections::HashMap<String, String>,
    /// Requests whose waiter is still alive. Completions for unknown request
    /// ids are ignored, which makes a callback that arrives after a dropped
    /// step harmless without retaining unbounded tombstones.
    active: std::collections::HashSet<String>,
}

impl ExternalCallStore {
    fn begin(&mut self, request_id: &str) {
        self.active.insert(request_id.to_string());
    }

    fn complete(&mut self, request_id: &str, result_json: String) {
        if self.active.contains(request_id) {
            self.pending.insert(request_id.to_string(), result_json);
        }
    }

    fn take(&mut self, request_id: &str) -> Option<String> {
        self.pending.remove(request_id)
    }

    fn abandon(&mut self, request_id: &str) {
        self.active.remove(request_id);
        self.pending.remove(request_id);
    }
}

struct ExternalCallGuard {
    store: std::sync::Arc<std::sync::Mutex<ExternalCallStore>>,
    request_id: String,
    armed: bool,
}

impl ExternalCallGuard {
    fn new(
        store: std::sync::Arc<std::sync::Mutex<ExternalCallStore>>,
        request_id: String,
    ) -> Self {
        store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .begin(&request_id);
        Self {
            store,
            request_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        self.store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .abandon(&self.request_id);
    }
}

impl Drop for ExternalCallGuard {
    fn drop(&mut self) {
        if self.armed {
            self.store
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .abandon(&self.request_id);
        }
    }
}

struct BridgeExternalTool {
    callback: ToolCallback,
    pending: std::sync::Arc<std::sync::Mutex<ExternalCallStore>>,
    next_request_id: std::sync::atomic::AtomicU64,
}

/// Bridge-local deadline for an external (TS-side) tool result, seconds. TS
/// applies no blanket timeout to external tool execution (MCP tools carry
/// their own configurable `toolTimeoutMs`); this guard only prevents a
/// dropped TS callback (a completion that never arrives) from hanging the
/// turn forever.
const EXTERNAL_TOOL_DEADLINE_S: u64 = 120;

impl BridgeExternalTool {
    async fn execute_inner(
        &self,
        call: &dimi_engine::tool::ToolCall,
        ctx: &dimi_engine::tool::ToolContext,
        step_number: Option<u32>,
    ) -> dimi_engine::tool::ToolResult {
        let request_id = format!(
            "ext-{}",
            self.next_request_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut guard = ExternalCallGuard::new(
            std::sync::Arc::clone(&self.pending),
            request_id.clone(),
        );
        let payload = serde_json::to_string(&serde_json::json!({
            "requestId": request_id,
            // The LLM's streamed tool-call id — the wire `tool.result` must
            // carry it (the fold matches `tool.result` against `tool.call`
            // by this id; "ext-N" is only the completion-slot key).
            "toolCallId": call.id,
            "name": call.name,
            "arguments": call.arguments,
            "step": step_number,
            // The full assistant-message batch this call is part of: the TS
            // side builds `ToolResolutionContext.toolCalls` from it, so
            // external tools see their same-round siblings (AllDone's
            // mixed-use / "only tool call in its round" guard needs them).
            "toolCalls": ctx.tool_calls.iter().map(|sibling| serde_json::json!({
                "id": sibling.id,
                "name": sibling.name,
                "arguments": sibling.arguments,
            })).collect::<Vec<_>>(),
        }))
        .unwrap_or_default();
        let _ = self
            .callback
            .call(payload, ThreadsafeFunctionCallMode::NonBlocking);
        // Poll for the TS side's completion.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(EXTERNAL_TOOL_DEADLINE_S);
        loop {
            let result_json = self
                .pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take(&request_id);
            if let Some(result_json) = result_json {
                    guard.disarm();
                    let mut parsed: dimi_engine::tool::ToolResult =
                        serde_json::from_str(&result_json).unwrap_or_else(|_| {
                            dimi_engine::tool::ToolResult {
                                tool_call_id: call.id.clone(),
                                tool_name: call.name.clone(),
                                output: format!(
                                    "external tool returned invalid result: {result_json}"
                                ),
                                is_error: true,
                                stop_turn: false,
                                updates: vec![],
                            }
                        });
                    // The wire tool.result must reference the LLM's call id.
                    parsed.tool_call_id = call.id.clone();
                    return parsed;
            }
            if std::time::Instant::now() >= deadline {
                return dimi_engine::tool::ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: format!(
                        "Tool \"{}\" timed out waiting for the external result",
                        call.name
                    ),
                    is_error: true,
                    stop_turn: false,
                    updates: vec![],
                };
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for BridgeExternalTool {
    async fn execute(
        &self,
        call: &dimi_engine::tool::ToolCall,
        ctx: &dimi_engine::tool::ToolContext,
    ) -> dimi_engine::tool::ToolResult {
        self.execute_inner(call, ctx, None).await
    }

    async fn execute_with_step(
        &self,
        call: &dimi_engine::tool::ToolCall,
        ctx: &dimi_engine::tool::ToolContext,
        step_number: u32,
    ) -> dimi_engine::tool::ToolResult {
        self.execute_inner(call, ctx, Some(step_number)).await
    }
}

/// `RustTurnSession` — an in-flight Rust-engine turn with approval support.
#[napi]
pub struct RustTurnSession {
    inner: napi::tokio::sync::Mutex<dimi_engine::engine::TurnSession>,
    llm: std::sync::Arc<dyn LlmClient>,
    tools: std::sync::Arc<napi::tokio::sync::Mutex<ToolRegistry>>,
    /// LLM-visible tool whitelist (`EngineTurnInput.active_tools`): re-applied
    /// on every run/resume tool re-sync so filtered defs never leak into a
    /// request. `None` = all registered defs are advertised.
    active_tools: Option<Vec<String>>,
    /// Live policy: session-scope approvals recorded mid-turn are appended
    /// here so the SAME turn's resumed batch honors them (P1-6 review — TS
    /// session-approval-history reads live; a frozen snapshot would re-ask).
    policy: std::sync::Arc<std::sync::Mutex<PolicyConfig>>,
    /// Native-tool PreToolUse gate (A2 review): the TS runner vetoes native
    /// tool calls before they execute, mirroring the TS pipeline the engine
    /// bypasses.
    tool_gate: std::sync::Arc<ToolGate>,
    /// TS tool call completions keyed by request id.
    pending_external: std::sync::Arc<std::sync::Mutex<ExternalCallStore>>,
    /// Subagent steering queues keyed by agent id.
    steer_map: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>>>>>,
    /// This turn's own steering queue (drained into the next request).
    steer_queue: std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>>,
    /// Cooperative cancellation (TS RPC cancel).
    cancel: std::sync::Arc<dimi_engine::engine::CancelSignal>,
    /// The currently active engine step's cancellation signal. The TS
    /// runner uses this to cancel one step while keeping the turn alive.
    active_step_cancel: std::sync::Arc<
        std::sync::Mutex<
            Option<std::sync::Arc<dimi_engine::engine::CancelSignal>>,
        >,
    >,
    /// Set by the engine when the turn ends (every finish path): `steer`
    /// refuses to queue into a dead turn, so the TS runner falls back to
    /// starting a new turn instead of silently dropping the steer.
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Bounded engine-event queue drained by the dedicated forwarding thread
    /// (see `EngineEventChannel`): every event emitted by `run`/`resume` and
    /// by the tools' task-event sink flows through it, in emission order, to
    /// the TS-side callback. `Arc` so the sink callback and the forwarder
    /// share it across `run`/`resume` calls.
    event_channel: std::sync::Arc<EngineEventChannel>,
    /// Whether the forwarding thread was started (`set_on_event` spawns it
    /// once per session; the TS side registers the callback once before the
    /// first run).
    forwarder_started: std::sync::atomic::AtomicBool,
    /// Task lifecycle event sink handed to the tools (Bash / async subagent
    /// tools): `set_on_event` points it at the same bounded queue, so
    /// `task.started` / `task.settled` emitted from spawned workers/pollers
    /// ride the session's event stream.
    event_sink: EventSink,
    /// Shared background-task registry (Bash / AgentOutput / WaitFor / the
    /// subagent tool): retained so `cancel_task` (TaskStop parity) can flip a
    /// task's cancel signal.
    tasks: AgentTasks,
    /// Shared subagent tool-definition cell: the AsyncAgentTool reads it at
    /// subagent launch time. The tool registry is populated after session
    /// construction, so this cell is written on every run/resume re-sync —
    /// a snapshot taken at construction would stay empty forever.
    subagent_tools_defs: std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::EngineTool>>>,
}

/// Session teardown (TS `taskService.dispose` parity): once the runner
/// closes the session (or the napi object is dropped — the runner holds
/// every session until its own dispose, so Drop only fires at teardown),
/// the EventSink stops forwarding late task settles and the in-flight turn
/// is cancelled, so background workers/pollers cannot fire into a disposed
/// runner.
impl Drop for RustTurnSession {
    fn drop(&mut self) {
        self.event_sink.close();
        self.event_channel.close();
        self.cancel.cancel();
    }
}

/// Native-tool PreToolUse gate (A2 architecture review).
///
/// The TS toolExecutor runs PreToolUse hooks (veto) for EVERY tool — native
/// and external. The Rust engine executes native tools (Bash/Agent/…) inside
/// the engine, bypassing the TS pipeline, so without a gate a user-configured
/// command-level veto hook silently never applies to Bash. This bridge lets
/// the TS runner veto a native call before it executes:
///
/// 1. `LockedRegistry::execute` sends a gate request through the callback and
///    awaits the verdict via a oneshot;
/// 2. the runner triggers `PreToolUse` and answers `complete_tool_gate`;
/// 3. a block verdict short-circuits the call with an error result.
///
/// The gate applies to every registry call (native + external); the runner
/// answers `allow` immediately for external tools, which already run their
/// own PreToolUse inside the external-tool callback.
#[derive(Default)]
struct ToolGate {
    /// TS-side gate callback (fire-and-forget request → `complete_tool_gate`).
    callback: std::sync::Mutex<Option<ToolCallback>>,
    /// Pending gate requests awaiting the TS verdict.
    pending: std::sync::Mutex<
        std::collections::HashMap<String, tokio::sync::oneshot::Sender<String>>,
    >,
    /// Monotonic gate request id.
    next_id: std::sync::atomic::AtomicU64,
}

impl ToolGate {
    fn request_id(&self) -> String {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("gate-{id}")
    }
}

struct ToolGateRequestGuard {
    gate: std::sync::Arc<ToolGate>,
    request_id: String,
}

impl ToolGateRequestGuard {
    fn new(
        gate: std::sync::Arc<ToolGate>,
        request_id: String,
        sender: tokio::sync::oneshot::Sender<String>,
    ) -> Self {
        gate.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(request_id.clone(), sender);
        Self { gate, request_id }
    }
}

impl Drop for ToolGateRequestGuard {
    fn drop(&mut self) {
        self.gate
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.request_id);
    }
}

/// Mutex-wrapped registry implementing ToolExecutor, with the native-tool
/// PreToolUse gate (A2).
struct LockedRegistry {
    registry: std::sync::Arc<napi::tokio::sync::Mutex<ToolRegistry>>,
    gate: std::sync::Arc<ToolGate>,
}

impl LockedRegistry {
    fn with_gate(
        registry: std::sync::Arc<napi::tokio::sync::Mutex<ToolRegistry>>,
        gate: std::sync::Arc<ToolGate>,
    ) -> Self {
        Self { registry, gate }
    }

    /// Ask the TS runner for a PreToolUse verdict; `None` when no gate is
    /// registered (no hooks configured — allow) or the runner never answers.
    async fn pre_gate(
        &self,
        call: &dimi_engine::tool::ToolCall,
        step_number: Option<u32>,
    ) -> Option<dimi_engine::tool::ToolResult> {
        let request_id = self.gate.request_id();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _request_guard = ToolGateRequestGuard::new(
            std::sync::Arc::clone(&self.gate),
            request_id.clone(),
            tx,
        );
        let payload = serde_json::json!({
            "requestId": request_id,
            "toolName": call.name,
            "arguments": call.arguments,
            "step": step_number,
        });
        // The TSFN is not Clone — call while holding the guard (Blocking mode
        // blocks only until the callback is scheduled, not until it runs).
        // The guard must not survive the `rx.await` below (std guards are
        // !Send), so the call happens inside a block that ends before it.
        {
            let guard = self.gate.callback.lock().unwrap_or_else(|p| p.into_inner());
            let Some(callback) = guard.as_ref() else {
                return None;
            };
            let status = callback.call(payload.to_string(), ThreadsafeFunctionCallMode::Blocking);
            if status == Status::Closing || status == Status::Unknown {
                return None;
            }
        }
        match rx.await {
            Ok(verdict_json) => {
                let verdict: serde_json::Value =
                    serde_json::from_str(&verdict_json).unwrap_or(serde_json::json!({ "decision": "allow" }));
                match verdict.get("decision").and_then(|v| v.as_str()) {
                    Some("block") => {
                        let reason = verdict
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Tool was blocked by a PreToolUse hook.")
                            .to_string();
                        Some(dimi_engine::tool::ToolResult {
                            tool_call_id: call.id.clone(),
                            tool_name: call.name.clone(),
                            output: reason,
                            is_error: true,
                            stop_turn: false,
                            updates: vec![],
                        })
                    }
                    _ => None,
                }
            }
            Err(_) => None,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for LockedRegistry {
    async fn execute(
        &self,
        call: &dimi_engine::tool::ToolCall,
        ctx: &dimi_engine::tool::ToolContext,
    ) -> dimi_engine::tool::ToolResult {
        // A2: PreToolUse veto for native tools (the runner answers allow for
        // external tools, which gate themselves inside their callback).
        if let Some(blocked) = self.pre_gate(call, None).await {
            return blocked;
        }
        let registry = self.registry.lock().await;
        registry.execute(call, ctx).await
    }

    async fn execute_with_step(
        &self,
        call: &dimi_engine::tool::ToolCall,
        ctx: &dimi_engine::tool::ToolContext,
        step_number: u32,
    ) -> dimi_engine::tool::ToolResult {
        // Keep the native PreToolUse gate on the same path while preserving
        // the step number for bridge external tools.
        if let Some(blocked) = self.pre_gate(call, Some(step_number)).await {
            return blocked;
        }
        let registry = self.registry.lock().await;
        registry.execute_with_step(call, ctx, step_number).await
    }

    /// Forward cancellation through the registry (P1-2 review): the engine's
    /// cancel path calls `abort` on the executor it was given — the bridge
    /// gives it a `LockedRegistry`, so without forwarding, a cancelled Bash
    /// command would keep running as an orphan.
    ///
    /// Locking note (P2-9 review): `abort` runs inside the engine's
    /// `tokio::select!` cancel branch, AFTER the in-flight `execute` future
    /// (which holds the registry lock for the whole tool call) has been
    /// dropped by the select — that drop is what releases the lock, so
    /// `try_lock` succeeds here for the direct-Bash cancel case. Do not
    /// switch the tool execution to `tokio::spawn` (a spawned future is not
    /// dropped by the select) without revisiting this.
    fn abort(&self, call: &dimi_engine::tool::ToolCall) {
        let registry = self.registry.try_lock();
        if let Ok(registry) = registry {
            registry.abort(call);
        }
    }
}

fn progress_json(progress: dimi_engine::engine::TurnProgress) -> napi::Result<String> {
    let progress = match progress {
        dimi_engine::engine::TurnProgress::Completed(outcome) => {
            serde_json::json!({ "status": "completed", "outcome": outcome })
        }
        dimi_engine::engine::TurnProgress::NeedsApproval(approval) => {
            serde_json::json!({ "status": "needsApproval", "approval": approval })
        }
    };
    // Events are streamed through the `set_on_event` callback as they are
    // emitted; the response carries only the final progress.
    serde_json::to_string(&serde_json::json!({ "events": [], "progress": progress }))
        .map_err(wire_error)
}

fn make_client(
    input: &EngineTurnInput,
    scripted_segments_json: Option<String>,
) -> napi::Result<Box<dyn LlmClient>> {
    match scripted_segments_json {
        Some(segments_json) => {
            let segments: Vec<Vec<LlmStreamEvent>> =
                serde_json::from_str(&segments_json).map_err(wire_error)?;
            Ok(Box::new(ScriptedLlmClient::new(segments)))
        }
        None => Ok(Box::new(AimuxLlmClient {
            model: openai_model(&input.provider),
        })),
    }
}

/// Convert the registry's LLM-facing defs into `EngineTool` (the engine's
/// request `tools` field). A def that cannot be converted is discarded with
/// a warning (def name + reason) instead of being silently dropped; the
/// returned list is unchanged otherwise. `active_tools` (TS
/// `activeToolNames` parity) narrows the result to the whitelisted names —
/// defs outside it are hidden from the LLM while their executors stay
/// registered (the model just never sees them, so it never calls them).
/// `None` = every registered def is advertised.
fn engine_tools(
    registry: &ToolRegistry,
    active_tools: Option<&[String]>,
) -> Vec<dimi_engine::types::EngineTool> {
    let mut tools = Vec::new();
    for def in registry.tool_defs() {
        let Some(function) = def.get("function") else {
            eprintln!(
                "[dimi-bridge] warn: dropping invalid tool def (missing \"function\"): {def}"
            );
            continue;
        };
        let Some(name) = function.get("name").and_then(|n| n.as_str()) else {
            eprintln!(
                "[dimi-bridge] warn: dropping invalid tool def (missing string \"function.name\"): {def}"
            );
            continue;
        };
        // The whitelist hides non-listed defs from the LLM (execution
        // registration is untouched — the filtered model never calls them).
        if let Some(active) = active_tools {
            if !active.iter().any(|n| n == name) {
                continue;
            }
        }
        let description = function
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        let args_schema = function
            .get("parameters")
            .cloned()
            .unwrap_or(serde_json::json!({"type":"object","properties":{}}));
        match serde_json::from_value(serde_json::json!({
            "name": name,
            "description": description,
            "argsSchema": args_schema,
        })) {
            Ok(tool) => tools.push(tool),
            Err(error) => eprintln!(
                "[dimi-bridge] warn: dropping invalid tool def \"{name}\" (conversion failed: {error})"
            ),
        }
    }
    tools
}

#[napi]
impl RustTurnSession {
    #[napi(constructor)]
    pub fn new(
        input_json: String,
        policy_json: String,
        scripted_segments_json: Option<String>,
        registry_id: String,
    ) -> napi::Result<Self> {
        let input: EngineTurnInput = serde_json::from_str(&input_json).map_err(wire_error)?;
        let policy: PolicyConfig = serde_json::from_str(&policy_json).map_err(wire_error)?;
        // Live policy (P1-6): the engine re-reads it on every run/resume, so
        // `add_session_approval` recorded mid-turn takes effect immediately.
        let policy = std::sync::Arc::new(std::sync::Mutex::new(policy));
        let llm: std::sync::Arc<dyn LlmClient> =
            std::sync::Arc::from(make_client(&input, scripted_segments_json)?);
        let registry = session_registry(&registry_id);
        let tasks = registry.tasks.clone();
        let steer_map = registry.steer_map.clone();
        let event_sink = EventSink::new();
        let mut registry = ToolRegistry::new();
        // Shared subagent tool-defs cell: AsyncAgentTool reads it at subagent
        // launch time. The tool registry is populated after session
        // construction (TS hands an empty `tools` array), so this cell is
        // seeded/updated on every run/resume re-sync — a snapshot taken here
        // would stay empty forever.
        let subagent_tools_defs =
            std::sync::Arc::new(std::sync::Mutex::new(input.tools.clone()));
        // The Bash def mirrors the TS tool's advertised contract
        // (`BASH_PARAMETERS` minus `stdin_mode`/`disable_timeout`, which only
        // apply to the TS-only TaskInput/background paths): same description
        // (rendered with the TS constants, background paragraphs adapted to
        // the engine's capabilities), same properties, same descriptions,
        // same required. `run_in_background` stays in the schema for model
        // compatibility; the executor rejects it with a clear error.
        registry.register_with_def(
            "Bash",
            Box::new(
                BashTool::with_tasks(tasks.clone())
                    .with_events(event_sink.clone())
                    .with_kill_grace(std::time::Duration::from_millis(
                        input.kill_grace_ms.unwrap_or(dimi_engine::tool::DEFAULT_KILL_GRACE_MS),
                    )),
            ),
            Some(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "Bash",
                    "description": "Execute a bash command. Use this for shell semantics — pipes, env, processes, git, package managers, build/test runners, anything genuinely interactive or multi-step.\n\n**Translate these to a dedicated tool instead:**\n- `cat` / `head` / `tail` (known path) → `Read`\n- `sed` / `awk` (in-place edit) → `Edit`\n- `echo > file` / `cat <<EOF` → `Write`\n- `find` / recursive `ls` to locate files by name pattern → `Glob` (plain `ls <known-directory>` is fine for listing a directory)\n- `grep` / `rg` (search file contents) → `Grep`\n- `echo` / `printf` (talk to the user) → just output text directly\n\nThe dedicated tools render in the per-tool permission UI and keep raw stdout out of the conversation; that is why they are worth reaching for whenever one fits.\n\n**Output:**\nThe stdout and stderr will be combined and returned as a string. The output may be truncated if it is too long. If the command exits non-zero, the output ends with a `Command failed with exit code: N` line; a command killed by its timeout or interrupted by the user ends with its own message instead.\n\nBackground execution (`run_in_background=true`) is not supported by this engine. Do not set it.\n\n**Guidelines for safety and security:**\n- Each shell tool call will be executed in a fresh shell environment. The shell variables, current working directory changes, and the shell history is not preserved between calls. To run a command in a particular directory, pass the `cwd` argument (or use absolute paths) rather than relying on a `cd` from an earlier call.\n- The tool call will return after the command is finished. You shall not use this tool to execute an interactive command or a command that may run forever. For possibly long-running foreground commands, set the `timeout` argument in seconds. Foreground commands default to 60s and allow up to 300s. When a foreground command hits its timeout it is moved to the background instead of being killed, and you will be automatically notified when it completes.\n- Avoid using `..` to access files or directories outside of the working directory.\n- Avoid modifying files outside of the working directory unless explicitly instructed to do so.\n- Never run commands that require superuser privileges unless explicitly instructed to do so.\n\n**Guidelines for efficiency:**\n- Use `&&` to chain commands that genuinely depend on each other, e.g. `npm install && npm test`. Independent read-only commands (separate `git show`, `ls`, or status checks) should be issued as separate parallel Bash calls in one response, not chained into a single call — chaining serializes their execution and mixes their output. Do not stitch outputs together with `echo` separators.\n- Use `;` to run commands sequentially regardless of success/failure\n- Use `||` for conditional execution (run second command only if first fails)\n- Use pipe operations (`|`) and redirections (`>`, `>>`) to chain input and output between commands\n- Always quote file paths containing spaces with double quotes (e.g., cd \"/path with spaces/\")\n- Compose multi-step logic in a single call with `if` / `case` / `for` / `while` control flows.\n\n**Commands available:**\nThe following common command categories are usually available. Availability still depends on the host, so when in doubt run `which <command>` first to confirm a command exists before relying on it.\n- Navigation and inspection: `ls`, `pwd`, `cd`, `stat`, `file`, `du`, `df`, `tree`\n- File and directory management: `cp`, `mv`, `rm`, `mkdir`, `touch`, `ln`, `chmod`, `chown`\n- Text and data processing: `wc`, `sort`, `uniq`, `cut`, `tr`, `diff`, `xargs`\n- Archives and compression: `tar`, `gzip`, `gunzip`, `zip`, `unzip`\n- Networking and transfer: `curl`, `wget`, `ping`, `ssh`, `scp`\n- Version control: `git`; for GitHub-hosted work (PRs, issues, CI runs, API queries) prefer the `gh` CLI when installed — it carries the user's GitHub auth and can return structured JSON\n- Process and system: `ps`, `kill`, `top`, `env`, `date`, `uname`, `whoami`\n- Language and package toolchains: `node`, `npm`, `pnpm`, `yarn`, `python`, `pip` (use whichever the project actually relies on)",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "description": "The command to execute." },
                            "cwd": { "type": "string", "description": "The working directory in which to run the command. When omitted, the command runs in the session's working directory." },
                            "timeout": { "type": "number", "description": "Optional timeout in seconds for the command to execute. Foreground default 60s, max 300s." },
                            "description": { "type": "string", "description": "A short description for the background task. Required when run_in_background is true." },
                            "run_in_background": { "type": "boolean", "description": "Whether to run the command as a background task. Not supported by this engine; do not set it to true." }
                        },
                        "required": ["command"],
                        "additionalProperties": false
                    }
                }
            })),
        );
        let tool_gate = std::sync::Arc::new(ToolGate::default());
        let tools = std::sync::Arc::new(napi::tokio::sync::Mutex::new(registry));
        {
            let mut registry = tools
                .try_lock()
                .map_err(|_| napi::Error::from_reason("registry busy"))?;
            // Subagents execute through the same registry (all registered
            // tools — Bash, external TS tools, and the async tools).
            let subagent_tools: std::sync::Arc<dyn ToolExecutor> =
                std::sync::Arc::new(LockedRegistry::with_gate(
                    std::sync::Arc::clone(&tools),
                    std::sync::Arc::clone(&tool_gate),
                ));
            // Subagent model parity (TS `resolveSubagentBinding`): when the
            // runner resolved a subagent model that differs from the parent's
            // provider, build a dedicated aimux client for nested turns so
            // they run on the bound model instead of inheriting the parent's.
            // `None` = subagents reuse the parent's client (scripted segments
            // under test are also reused, keeping the differential harness
            // deterministic).
            let (subagent_llm, subagent_provider): (
                Option<std::sync::Arc<dyn LlmClient>>,
                Option<dimi_engine::types::ProviderConfig>,
            ) = match &input.subagent_model {
                Some(model) if model != &input.provider => (
                    Some(std::sync::Arc::new(AimuxLlmClient {
                        model: openai_model(model),
                    })),
                    Some(model.clone()),
                ),
                _ => (None, None),
            };
            registry.register(
                "Agent",
                Box::new(AsyncAgentTool {
                    llm: std::sync::Arc::clone(&llm),
                    tools: subagent_tools,
                    tools_defs: std::sync::Arc::clone(&subagent_tools_defs),
                    policy: policy.lock().unwrap_or_else(|p| p.into_inner()).clone(),
                    max_steps: input.max_steps_per_turn,
                    shell: input.shell.clone().unwrap_or_else(dimi_exec::env::default_shell),
                    tasks: tasks.clone(),
                    steer_map: std::sync::Arc::clone(&steer_map),
                    events: event_sink.clone(),
                    agent_id_counter: std::sync::atomic::AtomicU64::new(
                        input.next_agent_id.unwrap_or(0),
                    ),
                    subagent_llm,
                    subagent_provider,
                    subagent_allowlist: input.subagent_allowlist.clone(),
                    subagent_timeout_ms: input.subagent_timeout_ms,
                    max_running_tasks: input.max_running_tasks,
                }),
            );
            registry.register(
                "AgentOutput",
                Box::new(AgentOutputTool {
                    tasks: tasks.clone(),
                }),
            );
            registry.register("WaitFor", Box::new(WaitForTool { tasks: tasks.clone() }));
        }
        // Expose the registry's tool definitions to the LLM (initial set;
        // re-synced before every run/resume so tools registered mid-session
        // become visible to the model).
        let mut input = input;
        let active_tools = input.active_tools.clone();
        {
            let registry = tools
                .try_lock()
                .map_err(|_| napi::Error::from_reason("registry busy"))?;
            input.tools = engine_tools(&registry, active_tools.as_deref());
        }
        let steer_queue: std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let cancel = std::sync::Arc::new(dimi_engine::engine::CancelSignal::new());
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let inner = dimi_engine::engine::TurnSession::with_steer_and_cancel(
            input,
            Some(std::sync::Arc::clone(&steer_queue)),
            std::sync::Arc::clone(&cancel),
            std::sync::Arc::clone(&finished),
        );
        let active_step_cancel = inner.active_step_cancel_handle();
        Ok(Self {
            inner: napi::tokio::sync::Mutex::new(inner),
            llm,
            tools,
            active_tools,
            policy,
            tool_gate,
            pending_external: std::sync::Arc::new(std::sync::Mutex::new(
                ExternalCallStore::default(),
            )),
            steer_map,
            steer_queue,
            cancel,
            active_step_cancel,
            finished,
            event_channel: std::sync::Arc::new(EngineEventChannel::new(EVENT_QUEUE_CAP)),
            forwarder_started: std::sync::atomic::AtomicBool::new(false),
            event_sink,
            tasks,
            subagent_tools_defs,
        })
    }

    /// Cancel the running turn: the engine stops at the next step boundary
    /// (or races the in-flight LLM/tool await) and finishes as `cancelled`.
    #[napi]
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Cancel only the currently active step. The engine emits
    /// `turn.step.interrupted` and continues the same turn with the next
    /// step; `false` means the session is between steps or already finished.
    #[napi]
    pub fn cancel_step(&self) -> bool {
        let active = self
            .active_step_cancel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(signal) = active.as_ref() else {
            return false;
        };
        if signal.is_cancelled() {
            return false;
        }
        signal.cancel();
        true
    }

    /// Record a session-scope approval (P1-6 review): the engine's policy is
    /// re-read on every run/resume, so a pattern approved for the session
    /// mid-turn is honored by the SAME turn's remaining batch — the runner
    /// calls this before resuming, matching the TS session-approval-history
    /// policy which reads live (a frozen snapshot would re-ask).
    #[napi]
    pub fn add_session_approval(&self, pattern: String) {
        let mut policy = self.policy.lock().unwrap_or_else(|p| p.into_inner());
        if !policy.session_approved_patterns.contains(&pattern) {
            policy.session_approved_patterns.push(pattern);
        }
    }

    /// Register the native-tool PreToolUse gate (A2 review): every registry
    /// tool call is first announced through this callback
    /// (`{requestId, toolName, arguments}` JSON); the runner answers with
    /// `completeToolGate(requestId, {decision:'allow'|'block', reason?})`.
    /// A block short-circuits the call with an error result.
    #[napi]
    pub fn set_tool_gate(&self, callback: ToolCallback) {
        *self.tool_gate.callback.lock().unwrap_or_else(|p| p.into_inner()) = Some(callback);
    }

    /// Answer a pending gate request (see `set_tool_gate`).
    #[napi]
    pub fn complete_tool_gate(&self, request_id: String, verdict_json: String) {
        let sender = self
            .tool_gate
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request_id);
        if let Some(sender) = sender {
            let _ = sender.send(verdict_json);
        }
    }

    /// Cancel a background task (TaskStop parity): flips the task's cancel
    /// signal — carrying the TS stop reason, so the worker/poller settles
    /// "killed" with the actual reason on the wire — the subagent worker /
    /// bash poller observes it, stops the work (kills the process / cancels
    /// the nested turn) and settles the task with status "killed".
    #[napi]
    pub fn cancel_task(&self, task_id: String, reason: Option<String>) -> napi::Result<()> {
        let state = self
            .tasks
            .get(&task_id)
            .or_else(|| self.tasks.find_by_agent_id(&task_id));
        let Some(state) = state else {
            return Err(napi::Error::from_reason(format!(
                "no background task with task_id: {task_id}"
            )));
        };
        if let Some(cancel) = state.cancel {
            cancel.cancel_with_reason(reason);
        }
        Ok(())
    }

    /// Close the session (TS `taskService.dispose` parity): the task event
    /// sink stops forwarding, the in-flight turn is cancelled, the event
    /// channel closes (waking the forwarder / blocked emitters), and
    /// `steer` refuses. Called by the TS runner when the agent is disposed;
    /// background workers/pollers observe `is_closed` and stop their work.
    #[napi]
    pub fn close(&self) {
        self.event_sink.close();
        self.event_channel.close();
        self.cancel.cancel();
    }

    /// Register the per-event callback: every engine event emitted by `run`
    /// /`resume` is pushed through it as JSON, in emission order, as it
    /// happens. The `run`/`resume` response then carries only the final
    /// progress. Register before the first `run`; the callback stays active
    /// across `resume` phases. Events flow through a bounded queue drained
    /// by a dedicated forwarding thread using Blocking ThreadsafeFunction
    /// calls — nothing is dropped under queue pressure (F6 fix).
    #[napi]
    pub fn set_on_event(&mut self, callback: EventCallback) {
        let tsfn = std::sync::Arc::new(callback);
        // One forwarder per session: the first `set_on_event` starts it;
        // later calls only re-point the sink (the TS side registers once
        // before the first run). The thread exits when the channel closes
        // (session `close()` / `Drop`, or the napi queue going `Closing`).
        if !self
            .forwarder_started
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            let channel = std::sync::Arc::clone(&self.event_channel);
            std::thread::spawn(move || {
                while let Some(event) = channel.recv() {
                    if let Ok(json) = serde_json::to_string(&event) {
                        let status =
                            tsfn.call(json, ThreadsafeFunctionCallMode::Blocking);
                        if status == Status::Closing || status == Status::Unknown {
                            // The JS side is gone (env teardown): stop
                            // forwarding; wake blocked emitters so they
                            // observe the close and exit.
                            channel.close();
                            break;
                        }
                    }
                    // Count the event as submitted to the ThreadsafeFunction
                    // so `run`/`resume` can wait for full delivery before
                    // resolving their promise.
                    channel.mark_delivered();
                }
            });
        }
        let sink_channel = std::sync::Arc::clone(&self.event_channel);
        let sink_callback: std::sync::Arc<dyn Fn(EngineEvent) + Send + Sync> =
            std::sync::Arc::new(move |event| {
                let _ = sink_channel.send(event);
        });
        self.event_sink.set(sink_callback);
    }

    /// Acknowledge one event after the TS callback has finished projecting it
    /// into the bus and context. `wait_for_events` uses these acknowledgements
    /// to make approval progress observe the same ordering as the old direct
    /// callback path.
    #[napi]
    pub fn acknowledge_event(&self) {
        self.event_channel.mark_observed();
    }

    /// Wait until every event emitted before this call has finished running
    /// on the JS side. The wait is offloaded because the callback must remain
    /// able to execute on the Node event loop and call `acknowledge_event`.
    #[napi]
    pub async fn wait_for_events(&self) -> napi::Result<()> {
        let channel = std::sync::Arc::clone(&self.event_channel);
        let target = channel.pushed_count();
        napi::tokio::task::spawn_blocking(move || channel.wait_observed(target))
            .await
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        Ok(())
    }

    /// Steer the running turn: the message is queued and drained into the
    /// next LLM request (async-subagent semantics). Returns `false` when the
    /// turn has already finished — the queue would never be drained again —
    /// so the TS runner falls back to starting a new turn with the message
    /// (a steer racing the teardown must not be silently dropped).
    #[napi]
    pub fn steer(&self, message: String) -> bool {
        if self.finished.load(std::sync::atomic::Ordering::Relaxed) || self.event_sink.is_closed() {
            return false;
        }
        self.steer_queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(dimi_engine::types::LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(message),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            });
        true
    }

    /// Advance the turn until completion or an approval request. Every
    /// engine event is streamed to the `set_on_event` callback as it is
    /// emitted (through the bounded event queue); the response carries only
    /// the progress.
    #[napi]
    pub async fn run(&self) -> napi::Result<String> {
        let progress = {
            let mut inner = self.inner.lock().await;
            // Re-sync the LLM-facing tool defs (external tools may have been
            // registered since the session was constructed).
            {
                let registry = self.tools.lock().await;
                let defs = engine_tools(&registry, self.active_tools.as_deref());
                inner.update_tools(defs.clone());
                // Subagents launched later must advertise the same current
                // tool set (the AsyncAgentTool reads this shared cell).
                *self
                    .subagent_tools_defs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = defs;
            }
            let channel = std::sync::Arc::clone(&self.event_channel);
            // Clone the policy snapshot (P1-6): the guard must not survive
            // the await (std MutexGuard is !Send).
            let policy = self.policy.lock().unwrap_or_else(|p| p.into_inner()).clone();
            inner
                .run(
                    self.llm.as_ref(),
                    &LockedRegistry::with_gate(std::sync::Arc::clone(&self.tools), std::sync::Arc::clone(&self.tool_gate)),
                    &policy,
                    &mut move |event| {
                        let _ = channel.send(event);
                    },
                )
                .await
        };
        // Every event emitted during the turn is now in the channel; wait for
        // the forwarding thread to submit each one to the ThreadsafeFunction
        // before the promise resolves — the TS side is guaranteed the turn's
        // events are SUBMITTED to the TSFN queue before the `run`
        // continuation (the ordering the old synchronous push had; the JS
        // callbacks themselves run on the Node event loop and are drained
        // before this promise's microtask in practice, but the guarantee the
        // channel enforces is submission, not JS-side observation). The wait
        // is a blocking condvar wait, so it runs on the blocking pool — it
        // must never occupy a tokio worker that engine workers (bash
        // pollers / subagent workers) run on.
        let channel = std::sync::Arc::clone(&self.event_channel);
        napi::tokio::task::spawn_blocking(move || channel.wait_caught_up())
            .await
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        progress_json(progress)
    }

    /// Register a TS-side tool: the engine routes `name` calls to the
    /// callback; the callback's async execution completes via
    /// `completeToolCall(requestId, resultJson)`. The LLM-facing definition
    /// (description + JSON parameters schema) is advertised to the model from
    /// the next request on.
    #[napi]
    pub fn register_external_tool(
        &self,
        name: String,
        description: String,
        parameters_json: String,
        callback: ToolCallback,
    ) -> napi::Result<()> {
        let pending = std::sync::Arc::clone(&self.pending_external);
        let tool = BridgeExternalTool {
            callback,
            pending,
            next_request_id: std::sync::atomic::AtomicU64::new(0),
        };
        let parameters: serde_json::Value = serde_json::from_str(&parameters_json)
            .map_err(|_| napi::Error::from_reason("invalid tool parameters JSON"))?;
        let def = serde_json::json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters,
            }
        });
        self.tools
            .try_lock()
            .map_err(|_| napi::Error::from_reason("session is busy"))
            .map(|mut registry| registry.register_with_def(name, Box::new(tool), Some(def)))?;
        Ok(())
    }

    /// Advertise the LLM-facing definition of a Rust-native tool (Agent /
    /// AgentOutput / WaitFor): the tool is registered executor-first at
    /// session construction, so the model could not see it in the request
    /// `tools` field. The registry keeps the executor and only swaps in the
    /// def, which the engine re-syncs into every request before each
    /// run/resume.
    #[napi]
    pub fn register_native_tool_def(
        &self,
        name: String,
        description: String,
        parameters_json: String,
    ) -> napi::Result<()> {
        let parameters: serde_json::Value = serde_json::from_str(&parameters_json)
            .map_err(|_| napi::Error::from_reason("invalid tool parameters JSON"))?;
        let def = serde_json::json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters,
            }
        });
        let mut registry = self
            .tools
            .try_lock()
            .map_err(|_| napi::Error::from_reason("session is busy"))?;
        if !registry.set_def(&name, Some(def)) {
            return Err(napi::Error::from_reason(format!(
                "no native tool registered with name: {name}"
            )));
        }
        Ok(())
    }

    /// Steer a running subagent (async-subagent semantics): the message is
    /// queued and drained into the subagent's next request.
    #[napi]
    pub fn steer_subagent(&self, agent_id: String, message: String) -> napi::Result<()> {
        let steer_map = self.steer_map.lock().unwrap_or_else(|p| p.into_inner());
        let Some(queue) = steer_map.get(&agent_id) else {
            return Err(napi::Error::from_reason(format!(
                "no running subagent with agent_id: {agent_id}"
            )));
        };
        queue.lock().unwrap_or_else(|p| p.into_inner()).push(
            dimi_engine::types::LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(message),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                tools: None,
                reasoning: None,
                origin: None,
            },
        );
        Ok(())
    }

    /// Complete a pending external tool call (called by the TS callback).
    #[napi]
    pub fn complete_tool_call(&self, request_id: String, result_json: String) {
        self.pending_external
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .complete(&request_id, result_json);
    }

    /// Resume after the user's approval decision
    /// (`{decision:"approved"|"rejected", feedback?}`). Events stream to the
    /// `set_on_event` callback as they are emitted (through the bounded
    /// event queue); the response carries only the progress.
    #[napi]
    pub async fn resume(&self, decision_json: String) -> napi::Result<String> {
        let decision: ApprovalDecision =
            serde_json::from_str(&decision_json).map_err(wire_error)?;
        let progress = {
            let mut inner = self.inner.lock().await;
            {
                let registry = self.tools.lock().await;
                let defs = engine_tools(&registry, self.active_tools.as_deref());
                inner.update_tools(defs.clone());
                *self
                    .subagent_tools_defs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = defs;
            }
            let channel = std::sync::Arc::clone(&self.event_channel);
            // Clone the policy snapshot (P1-6): the guard must not survive
            // the await (std MutexGuard is !Send).
            let policy = self.policy.lock().unwrap_or_else(|p| p.into_inner()).clone();
            inner
                .resume(
                    decision,
                    self.llm.as_ref(),
                    &LockedRegistry::with_gate(std::sync::Arc::clone(&self.tools), std::sync::Arc::clone(&self.tool_gate)),
                    &policy,
                    &mut move |event| {
                        let _ = channel.send(event);
                    },
                )
                .await
        };
        // See `run`: every emitted event is submitted to the TSFN queue
        // before the `resume` continuation (blocking-pool wait, same
        // reasoning as above).
        let channel = std::sync::Arc::clone(&self.event_channel);
        napi::tokio::task::spawn_blocking(move || channel.wait_caught_up())
            .await
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        progress_json(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_call_store_ignores_completion_after_waiter_is_dropped() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(ExternalCallStore::default()));
        {
            let _guard = ExternalCallGuard::new(std::sync::Arc::clone(&store), "request-1".into());
        }
        store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .complete("request-1", "late result".to_string());
        assert_eq!(
            store
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take("request-1"),
            None
        );

        let mut store = ExternalCallStore::default();
        store.begin("request-2");
        store.complete("request-2", "live result".to_string());
        assert_eq!(store.take("request-2"), Some("live result".to_string()));
    }

    #[test]
    fn disarm_clears_a_completion_that_arrives_after_the_waiter_takes_one() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(ExternalCallStore::default()));
        let mut guard = ExternalCallGuard::new(std::sync::Arc::clone(&store), "request-race".into());
        store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .complete("request-race", "first result".to_string());
        assert_eq!(
            store
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take("request-race"),
            Some("first result".to_string())
        );
        // A duplicate/late completion can arrive in the small window before
        // the normal waiter disarms its guard. It must not survive disarm.
        store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .complete("request-race", "late result".to_string());
        guard.disarm();
        assert_eq!(
            store
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take("request-race"),
            None
        );
    }

    #[test]
    fn pre_gate_drops_a_request_when_the_callback_is_unavailable() {
        let gate = std::sync::Arc::new(ToolGate::default());
        let locked = LockedRegistry::with_gate(
            std::sync::Arc::new(napi::tokio::sync::Mutex::new(ToolRegistry::default())),
            std::sync::Arc::clone(&gate),
        );
        let call = dimi_engine::tool::ToolCall {
            id: "call-1".to_string(),
            name: "Bash".to_string(),
            arguments: serde_json::json!({}),
        };

        napi::tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(async {
                assert!(locked.pre_gate(&call, Some(3)).await.is_none());
            });
        assert!(gate
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty());
    }

    fn task_started(i: i64) -> EngineEvent {
        EngineEvent::TaskStarted {
            task_id: format!("task-{i}"),
            agent_id: format!("agent-{i}"),
            kind: "bash".to_string(),
            description: "d".to_string(),
            pid: None,
            parent_tool_call_id: None,
        }
    }

    #[test]
    fn event_channel_preserves_fifo_and_applies_backpressure() {
        // F6: the bounded queue must never drop an event under pressure —
        // a full queue blocks the sender until the forwarder drains, and
        // FIFO order is preserved end-to-end.
        let channel = std::sync::Arc::new(EngineEventChannel::new(2));
        assert!(channel.send(task_started(0)));
        assert!(channel.send(task_started(1)));
        assert_eq!(channel.pushed_count(), 2);
        assert_eq!(channel.delivered_count(), 0);

        // The queue is full (cap 2): a third send blocks until a recv frees
        // a slot — no event is dropped.
        let sender = std::thread::spawn({
            let channel = std::sync::Arc::clone(&channel);
            move || channel.send(task_started(2))
        });
        // Give the sender time to hit the full queue, then drain: the
        // blocked send lands and the order stays 0, 1, 2.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(matches!(channel.recv(), Some(EngineEvent::TaskStarted { task_id, .. }) if task_id == "task-0"));
        assert!(
            sender.join().expect("sender completes"),
            "the blocked send must land, not be dropped"
        );
        assert!(matches!(channel.recv(), Some(EngineEvent::TaskStarted { task_id, .. }) if task_id == "task-1"));
        assert!(matches!(channel.recv(), Some(EngineEvent::TaskStarted { task_id, .. }) if task_id == "task-2"));
        assert_eq!(channel.pushed_count(), 3);
        // recv pops without counting: delivery is the forwarder's job (it
        // calls mark_delivered after each ThreadsafeFunction submit).
        assert_eq!(channel.delivered_count(), 0);

        // `wait_caught_up` (what run/resume call before resolving) blocks
        // until every pushed event is marked delivered; marking all three
        // releases it. This is the guarantee that every turn event is
        // submitted to the TSFN queue before the run/resume promise
        // continuation.
        let waited = std::thread::spawn({
            let channel = std::sync::Arc::clone(&channel);
            move || channel.wait_caught_up()
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        channel.mark_delivered();
        channel.mark_delivered();
        channel.mark_delivered();
        waited.join().expect("wait_caught_up returns");
        assert_eq!(channel.delivered_count(), 3);

        // Closed: sends are refused and an empty recv returns None (the
        // forwarder's exit signal).
        channel.close();
        assert!(!channel.send(task_started(3)), "closed channels refuse sends");
        assert_eq!(channel.recv(), None);
    }

    #[test]
    fn wait_caught_up_survives_deliveries_racing_waiter_registration() {
        // Lost-wakeup regression: `mark_delivered` used to update the
        // delivered counter WITHOUT the queue mutex, so the forwarder's final
        // delivery could land between `wait_caught_up`'s predicate
        // evaluation and its `caught_up.wait()` registration — the notify
        // was lost and `run`/`resume` hung forever (the turn promise never
        // resolves). Repeat the push → immediate-deliver → wait cycle so the
        // delivery races the waiter registration on every iteration: with
        // the counters guarded by the queue mutex the waiter either observes
        // the delivery in its predicate or is already registered when the
        // notify fires, so `wait_caught_up` always returns.
        let channel = std::sync::Arc::new(EngineEventChannel::new(16));
        for i in 0..1_000i64 {
            assert!(channel.send(task_started(i)));
            // Deliver on a fresh thread (like the real forwarder) so the
            // mark_delivered can complete before, during, or after the main
            // thread's wait registration.
            let deliverer = std::thread::spawn({
                let channel = std::sync::Arc::clone(&channel);
                move || channel.mark_delivered()
            });
            channel.wait_caught_up();
            deliverer
                .join()
                .expect("deliverer completes without hanging");
            // Drain so the queue never fills across iterations.
            assert!(channel.recv().is_some());
        }
    }

    #[test]
    fn wait_observed_requires_js_callback_acknowledgement() {
        let channel = std::sync::Arc::new(EngineEventChannel::new(2));
        assert!(channel.send(task_started(0)));
        channel.mark_delivered();

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn({
            let channel = std::sync::Arc::clone(&channel);
            move || {
                channel.wait_observed(1);
                done_tx.send(()).expect("waiter reports completion");
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(
            done_rx.try_recv().is_err(),
            "delivery to the TSFN queue must not count as JS observation"
        );

        channel.mark_observed();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("JS acknowledgement releases the waiter");
        waiter.join().expect("waiter completes");
        assert_eq!(channel.observed_count(), 1);
    }

    #[test]
    fn task_registry_is_scoped_per_agent_and_shared_within_it() {
        // P1-1/P1-3 (review): every `RustTurnSession` of the SAME agent must
        // share ONE subagent task registry — the Agent tool hardcodes
        // `stop_turn: true`, so the launching turn ends immediately and the
        // NEXT turn (a new session) must still resolve the task via
        // AgentOutput / WaitFor. Different agents must NOT see each other's
        // tasks (agent ids like `agent-0` are only unique within an agent).
        let first = session_registry("agent-a");
        let second = session_registry("agent-a");
        let other = session_registry("agent-b");
        let task_id = "agent-shared-table-test".to_string();
        first.tasks.insert(
            task_id.clone(),
            dimi_engine::tool::TaskState {
                agent_id: "agent-0".to_string(),
                status: "running".to_string(),
                output: "partial".to_string(),
                error: None,
                messages: vec![],
                started_at: 1,
                cancel: None,
                deadline: std::time::Instant::now(),
            },
        );
        // Same registry id → the second session handle sees the task.
        let state = second
            .tasks
            .get(&task_id)
            .expect("a task inserted via one session handle must be visible via another");
        assert_eq!(state.agent_id, "agent-0");
        assert_eq!(state.status, "running");
        // Different registry id → isolated.
        assert!(
            other.tasks.get(&task_id).is_none(),
            "a different agent's registry must not see the task"
        );
        // drop_task_registry removes the scope.
        drop_task_registry("agent-a".to_string());
        let fresh = session_registry("agent-a");
        assert!(fresh.tasks.get(&task_id).is_none(), "drop must clear the scope");
    }

    #[test]
    fn engine_tools_filters_defs_by_active_tools() {
        // `active_tools` (TS `activeToolNames` parity) hides non-whitelisted
        // defs from the LLM — today the bridge's hardcoded Bash def leaks even
        // when the profile does not allow Bash. `None` (absent) keeps every
        // registered def. Only the LLM-facing advertisement is filtered: the
        // executors stay in the registry (the filtered model simply never
        // calls them).
        let mut registry = ToolRegistry::new();
        for name in ["Bash", "Read", "Write"] {
            registry.register_with_def(
                name,
                Box::new(BashTool::default()),
                Some(serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": format!("{name} tool"),
                        "parameters": { "type": "object", "properties": {} },
                    }
                })),
            );
        }
        let names = |tools: Vec<dimi_engine::types::EngineTool>| -> Vec<String> {
            tools.into_iter().map(|t| t.name).collect()
        };

        // No whitelist → every registered def is advertised.
        let all = names(engine_tools(&registry, None));
        assert_eq!(
            all,
            vec!["Bash".to_string(), "Read".to_string(), "Write".to_string()]
        );

        // Whitelist → only the listed defs; the hardcoded Bash def must not
        // leak when it is outside the whitelist.
        let whitelisted = names(engine_tools(&registry, Some(&["Read".to_string()])));
        assert_eq!(whitelisted, vec!["Read".to_string()]);
        assert!(
            !whitelisted.iter().any(|n| n == "Bash"),
            "Bash must not be advertised when it is outside the whitelist"
        );

        // An empty whitelist hides every tool.
        let none = names(engine_tools(&registry, Some(&[])));
        assert!(none.is_empty(), "an empty whitelist hides every tool");
    }
}
