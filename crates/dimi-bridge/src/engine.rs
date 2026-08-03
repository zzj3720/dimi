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
use dimi_engine::events::EngineEvent;
use dimi_engine::llm::{LlmClient, LlmStreamEvent, ScriptedLlmClient};
use dimi_engine::permission::{ApprovalDecision, PolicyConfig};
use dimi_engine::tool::{
    AgentOutputTool, AgentTasks, AsyncAgentTool, BashTool, ToolExecutor, ToolRegistry, WaitForTool,
};
use dimi_engine::types::EngineTurnInput;

use crate::wire_error;

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
                shell: "/bin/sh".to_string(),
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
/// via `set_on_event` before `run`/`resume`).
type EventCallback = ThreadsafeFunction<String, Unknown<'static>, String, Status, false>;

/// A TS-registered tool: the engine calls the napi callback, the TS side
/// executes the tool and completes the call via `completeToolCall`.
struct BridgeExternalTool {
    callback: ToolCallback,
    pending: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    next_request_id: std::sync::atomic::AtomicU64,
}

#[async_trait::async_trait]
impl ToolExecutor for BridgeExternalTool {
    async fn execute(
        &self,
        call: &dimi_engine::tool::ToolCall,
        _ctx: &dimi_engine::tool::ToolContext,
    ) -> dimi_engine::tool::ToolResult {
        let request_id = format!(
            "ext-{}",
            self.next_request_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let payload = serde_json::to_string(&serde_json::json!({
            "requestId": request_id,
            // The LLM's streamed tool-call id — the wire `tool.result` must
            // carry it (the fold matches `tool.result` against `tool.call`
            // by this id; "ext-N" is only the completion-slot key).
            "toolCallId": call.id,
            "name": call.name,
            "arguments": call.arguments,
        }))
        .unwrap_or_default();
        let _ = self
            .callback
            .call(payload, ThreadsafeFunctionCallMode::NonBlocking);
        // Poll for the TS side's completion.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            {
                let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(result_json) = pending.remove(&request_id) {
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

/// `RustTurnSession` — an in-flight Rust-engine turn with approval support.
#[napi]
pub struct RustTurnSession {
    inner: napi::tokio::sync::Mutex<dimi_engine::engine::TurnSession>,
    llm: std::sync::Arc<dyn LlmClient>,
    tools: std::sync::Arc<napi::tokio::sync::Mutex<ToolRegistry>>,
    policy: PolicyConfig,
    /// TS tool call completions keyed by request id.
    pending_external: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// Subagent steering queues keyed by agent id.
    steer_map: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>>>>>,
    /// This turn's own steering queue (drained into the next request).
    steer_queue: std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>>,
    /// Cooperative cancellation (TS RPC cancel).
    cancel: std::sync::Arc<dimi_engine::engine::CancelSignal>,
    /// TS-side event sink: every engine event is streamed as JSON, per
    /// event, in emission order (the turn's `run`/`resume` resolve with
    /// only the progress). `Arc` because `ThreadsafeFunction` is not
    /// `Clone` and the sink must survive across `run`/`resume` calls.
    on_event: Option<std::sync::Arc<EventCallback>>,
}

/// Mutex-wrapped registry implementing ToolExecutor.
struct LockedRegistry(std::sync::Arc<napi::tokio::sync::Mutex<ToolRegistry>>);

#[async_trait::async_trait]
impl ToolExecutor for LockedRegistry {
    async fn execute(
        &self,
        call: &dimi_engine::tool::ToolCall,
        ctx: &dimi_engine::tool::ToolContext,
    ) -> dimi_engine::tool::ToolResult {
        let registry = self.0.lock().await;
        registry.execute(call, ctx).await
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
/// request `tools` field).
fn engine_tools(registry: &ToolRegistry) -> Vec<dimi_engine::types::EngineTool> {
    registry
        .tool_defs()
        .into_iter()
        .filter_map(|def| {
            let function = def.get("function")?;
            serde_json::from_value(serde_json::json!({
                "name": function.get("name")?.as_str()?,
                "description": function.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                "argsSchema": function.get("parameters").cloned().unwrap_or(serde_json::json!({"type":"object","properties":{}})),
            }))
            .ok()
        })
        .collect()
}

#[napi]
impl RustTurnSession {
    #[napi(constructor)]
    pub fn new(
        input_json: String,
        policy_json: String,
        scripted_segments_json: Option<String>,
    ) -> napi::Result<Self> {
        let input: EngineTurnInput = serde_json::from_str(&input_json).map_err(wire_error)?;
        let policy: PolicyConfig = serde_json::from_str(&policy_json).map_err(wire_error)?;
        let llm: std::sync::Arc<dyn LlmClient> =
            std::sync::Arc::from(make_client(&input, scripted_segments_json)?);
        let tasks = AgentTasks::new();
        let steer_map: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let mut registry = ToolRegistry::new();
        registry.register_with_def(
            "Bash",
            Box::new(BashTool::with_tasks(tasks.clone())),
            Some(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "Bash",
                    "description": "Run a shell command on the local machine. Use for file operations, running tests, git, and any command-line work.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "description": "The shell command to run" },
                            "cwd": { "type": "string", "description": "Working directory (default: session cwd)" },
                            "timeout": { "type": "integer", "description": "Timeout in seconds (default 60, max 300)" }
                        },
                        "required": ["command"]
                    }
                }
            })),
        );
        let tools = std::sync::Arc::new(napi::tokio::sync::Mutex::new(registry));
        {
            let mut registry = tools
                .try_lock()
                .map_err(|_| napi::Error::from_reason("registry busy"))?;
            // Subagents execute through the same registry (all registered
            // tools — Bash, external TS tools, and the async tools).
            let subagent_tools: std::sync::Arc<dyn ToolExecutor> =
                std::sync::Arc::new(LockedRegistry(std::sync::Arc::clone(&tools)));
            registry.register(
                "Agent",
                Box::new(AsyncAgentTool {
                    llm: std::sync::Arc::clone(&llm),
                    tools: subagent_tools,
                    policy: policy.clone(),
                    max_steps: input.max_steps_per_turn,
                    shell: input.shell.clone().unwrap_or_else(|| "/bin/sh".to_string()),
                    tasks: tasks.clone(),
                    steer_map: std::sync::Arc::clone(&steer_map),
                }),
            );
            registry.register(
                "AgentOutput",
                Box::new(AgentOutputTool {
                    tasks: tasks.clone(),
                }),
            );
            registry.register("WaitFor", Box::new(WaitForTool { tasks }));
        }
        // Expose the registry's tool definitions to the LLM (initial set;
        // re-synced before every run/resume so tools registered mid-session
        // become visible to the model).
        let mut input = input;
        {
            let registry = tools
                .try_lock()
                .map_err(|_| napi::Error::from_reason("registry busy"))?;
            input.tools = engine_tools(&registry);
        }
        let steer_queue: std::sync::Arc<std::sync::Mutex<Vec<dimi_engine::types::LlmMessage>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let cancel = std::sync::Arc::new(dimi_engine::engine::CancelSignal::new());
        Ok(Self {
            inner: napi::tokio::sync::Mutex::new(
                dimi_engine::engine::TurnSession::with_steer_and_cancel(
                    input,
                    Some(std::sync::Arc::clone(&steer_queue)),
                    std::sync::Arc::clone(&cancel),
                ),
            ),
            llm,
            tools,
            policy,
            pending_external: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            steer_map,
            steer_queue,
            cancel,
            on_event: None,
        })
    }

    /// Cancel the running turn: the engine stops at the next step boundary
    /// (or races the in-flight LLM/tool await) and finishes as `cancelled`.
    #[napi]
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Register the per-event callback: every engine event emitted by `run`
    /// /`resume` is pushed through it as JSON, in emission order, as it
    /// happens. The `run`/`resume` response then carries only the final
    /// progress. Register before the first `run`; the callback stays active
    /// across `resume` phases.
    #[napi]
    pub fn set_on_event(&mut self, callback: EventCallback) {
        self.on_event = Some(std::sync::Arc::new(callback));
    }

    /// Steer the running turn: the message is queued and drained into the
    /// next LLM request (async-subagent semantics).
    #[napi]
    pub fn steer(&self, message: String) {
        self.steer_queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(dimi_engine::types::LlmMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(message),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning: None,
            });
    }

    /// Advance the turn until completion or an approval request. Every
    /// engine event is streamed to the `set_on_event` callback as it is
    /// emitted; the response carries only the progress.
    #[napi]
    pub async fn run(&self) -> napi::Result<String> {
        let progress = {
            let mut inner = self.inner.lock().await;
            // Re-sync the LLM-facing tool defs (external tools may have been
            // registered since the session was constructed).
            {
                let registry = self.tools.lock().await;
                inner.update_tools(engine_tools(&registry));
            }
            let on_event = self.on_event.clone();
            inner
                .run(
                    self.llm.as_ref(),
                    &LockedRegistry(std::sync::Arc::clone(&self.tools)),
                    &self.policy,
                    &mut move |event| {
                        if let Some(callback) = &on_event {
                            if let Ok(json) = serde_json::to_string(&event) {
                                let _ =
                                    callback.call(json, ThreadsafeFunctionCallMode::NonBlocking);
                            }
                        }
                    },
                )
                .await
        };
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
                reasoning: None,
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
            .insert(request_id, result_json);
    }

    /// Resume after the user's approval decision
    /// (`{decision:"approved"|"rejected", feedback?}`). Events stream to the
    /// `set_on_event` callback as they are emitted; the response carries
    /// only the progress.
    #[napi]
    pub async fn resume(&self, decision_json: String) -> napi::Result<String> {
        let decision: ApprovalDecision =
            serde_json::from_str(&decision_json).map_err(wire_error)?;
        let progress = {
            let mut inner = self.inner.lock().await;
            {
                let registry = self.tools.lock().await;
                inner.update_tools(engine_tools(&registry));
            }
            let on_event = self.on_event.clone();
            inner
                .resume(
                    decision,
                    self.llm.as_ref(),
                    &LockedRegistry(std::sync::Arc::clone(&self.tools)),
                    &self.policy,
                    &mut move |event| {
                        if let Some(callback) = &on_event {
                            if let Ok(json) = serde_json::to_string(&event) {
                                let _ =
                                    callback.call(json, ThreadsafeFunctionCallMode::NonBlocking);
                            }
                        }
                    },
                )
                .await
        };
        progress_json(progress)
    }
}
