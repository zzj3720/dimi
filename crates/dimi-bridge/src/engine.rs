//! `RustEngine` — the M3 swap-in socket: one turn of the Rust orchestration
//! core exposed to Node.
//!
//! Slice 1 keeps the surface synchronous: `start_turn` runs the full turn
//! (LLM stream + Bash tool execution) and returns the collected engine
//! event batch plus the outcome. The TS adapter publishes those events on
//! the existing event bus, so the transcript projection/broadcast layers
//! keep working unchanged. Streaming callbacks (ThreadsafeFunction) land
//! with the dogfood swap-in.
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
        let tools: Box<dyn ToolExecutor> = Box::new(BashTool);
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
                    let parsed: dimi_engine::tool::ToolResult = serde_json::from_str(&result_json)
                        .unwrap_or_else(|_| dimi_engine::tool::ToolResult {
                            tool_call_id: call.id.clone(),
                            tool_name: call.name.clone(),
                            output: format!("external tool returned invalid result: {result_json}"),
                            is_error: true,
                            stop_turn: false,
                            updates: vec![],
                        });
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

fn progress_json(
    progress: dimi_engine::engine::TurnProgress,
    events: Vec<EngineEvent>,
) -> napi::Result<String> {
    let progress = match progress {
        dimi_engine::engine::TurnProgress::Completed(outcome) => {
            serde_json::json!({ "status": "completed", "outcome": outcome })
        }
        dimi_engine::engine::TurnProgress::NeedsApproval(approval) => {
            serde_json::json!({ "status": "needsApproval", "approval": approval })
        }
    };
    serde_json::to_string(&serde_json::json!({ "events": events, "progress": progress }))
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
        let mut registry = ToolRegistry::new();
        registry.register("Bash", Box::new(BashTool));
        registry.register(
            "Agent",
            Box::new(AsyncAgentTool {
                llm: std::sync::Arc::clone(&llm),
                tools: std::sync::Arc::new(BashTool),
                policy: policy.clone(),
                max_steps: input.max_steps_per_turn,
                shell: input.shell.clone().unwrap_or_else(|| "/bin/sh".to_string()),
                tasks: tasks.clone(),
            }),
        );
        registry.register(
            "AgentOutput",
            Box::new(AgentOutputTool {
                tasks: tasks.clone(),
            }),
        );
        registry.register("WaitFor", Box::new(WaitForTool { tasks }));
        let tools = std::sync::Arc::new(napi::tokio::sync::Mutex::new(registry));
        Ok(Self {
            inner: napi::tokio::sync::Mutex::new(dimi_engine::engine::TurnSession::new(input)),
            llm,
            tools,
            policy,
            pending_external: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        })
    }

    /// Advance the turn until completion or an approval request.
    #[napi]
    pub async fn run(&self) -> napi::Result<String> {
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = {
            let mut inner = self.inner.lock().await;
            inner
                .run(
                    self.llm.as_ref(),
                    &LockedRegistry(std::sync::Arc::clone(&self.tools)),
                    &self.policy,
                    &mut |event| events.push(event),
                )
                .await
        };
        progress_json(progress, events)
    }

    /// Register a TS-side tool: the engine routes `name` calls to the
    /// callback; the callback's async execution completes via
    /// `completeToolCall(requestId, resultJson)`.
    #[napi]
    pub fn register_external_tool(&self, name: String, callback: ToolCallback) -> napi::Result<()> {
        let pending = std::sync::Arc::clone(&self.pending_external);
        let tool = BridgeExternalTool {
            callback,
            pending,
            next_request_id: std::sync::atomic::AtomicU64::new(0),
        };
        self.tools
            .try_lock()
            .map_err(|_| napi::Error::from_reason("session is busy"))
            .map(|mut registry| registry.register(name, Box::new(tool)))?;
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
    /// (`{decision:"approved"|"rejected", feedback?}`).
    #[napi]
    pub async fn resume(&self, decision_json: String) -> napi::Result<String> {
        let decision: ApprovalDecision =
            serde_json::from_str(&decision_json).map_err(wire_error)?;
        let mut events: Vec<EngineEvent> = Vec::new();
        let progress = {
            let mut inner = self.inner.lock().await;
            inner
                .resume(
                    decision,
                    self.llm.as_ref(),
                    &LockedRegistry(std::sync::Arc::clone(&self.tools)),
                    &self.policy,
                    &mut |event| events.push(event),
                )
                .await
        };
        progress_json(progress, events)
    }
}
