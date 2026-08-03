//! Engine event stream — the shapes the TS loop publishes on its event bus
//! (see `agent/loop/turnEvents.ts` and `agent/toolExecutor/toolExecutorEvents.ts`).
//! The transcript projection layer (`coreEventMap`) consumes these verbatim,
//! so field names and semantics must stay byte-compatible with the TS types.

use serde::{Deserialize, Serialize};

use dimi_wire::model::{TranscriptUsage, TurnOrigin};

/// Why a step ended — mirrors `FinishReason` in the TS llmProtocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Completed,
    ToolCalls,
    Other,
    Truncated,
    Filtered,
    Length,
    ContentFilter,
    Cancelled,
    Interrupted,
}

/// One engine event, emitted in chronological order while a turn runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EngineEvent {
    #[serde(rename = "turn.started", rename_all = "camelCase")]
    TurnStarted {
        turn_id: i64,
        origin: TurnOrigin,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    #[serde(rename = "turn.ended", rename_all = "camelCase")]
    TurnEnded {
        turn_id: i64,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<i64>,
    },
    #[serde(rename = "turn.step.started", rename_all = "camelCase")]
    TurnStepStarted {
        turn_id: i64,
        step: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_id: Option<String>,
    },
    #[serde(rename = "turn.step.completed", rename_all = "camelCase")]
    TurnStepCompleted {
        turn_id: i64,
        step: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TranscriptUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },
    #[serde(rename = "turn.step.interrupted", rename_all = "camelCase")]
    TurnStepInterrupted {
        turn_id: i64,
        step: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_id: Option<String>,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "assistant.delta", rename_all = "camelCase")]
    AssistantDelta { turn_id: i64, delta: String },
    #[serde(rename = "thinking.delta", rename_all = "camelCase")]
    ThinkingDelta { turn_id: i64, delta: String },
    #[serde(rename = "tool.call.delta", rename_all = "camelCase")]
    ToolCallDelta {
        turn_id: i64,
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments_part: Option<String>,
    },
    #[serde(rename = "tool.call.started", rename_all = "camelCase")]
    ToolCallStarted {
        turn_id: i64,
        tool_call_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    #[serde(rename = "tool.progress", rename_all = "camelCase")]
    ToolProgress {
        turn_id: i64,
        tool_call_id: String,
        update: ToolUpdate,
    },
    #[serde(rename = "tool.result", rename_all = "camelCase")]
    ToolResult {
        turn_id: i64,
        tool_call_id: String,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        synthetic: Option<bool>,
    },
    /// Full-history compaction ran between steps: the engine replaced its
    /// working messages with the LLM summary (TS `fullCompaction` parity).
    #[serde(rename = "context.compacted", rename_all = "camelCase")]
    ContextCompacted {
        turn_id: i64,
        summary: String,
        tokens_before: u64,
        tokens_after: u64,
        compacted_count: u64,
    },
    /// A background task (subagent or timeout-backgrounded bash command) was
    /// registered: emitted from the launch site (AsyncAgentTool /
    /// BashTool backgrounding) so the TS side can record `task.started` /
    /// `subagent.spawned`. `pid` is bash-only; `parent_tool_call_id` is
    /// subagent-only (the Agent tool call that launched it).
    #[serde(rename = "task.started", rename_all = "camelCase")]
    TaskStarted {
        task_id: String,
        agent_id: String,
        /// "agent" (subagent) | "bash" (backgrounded command).
        kind: String,
        description: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pid: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
    },
    /// A background task settled: emitted from the settlement points (the
    /// spawned subagent worker / bash poller), never from readers. Carries
    /// the final output/error so the TS side can record `task.terminated`,
    /// fire `subagent.completed`/`subagent.failed`, and deliver the
    /// completion notification to the model.
    #[serde(rename = "task.settled", rename_all = "camelCase")]
    TaskSettled {
        task_id: String,
        agent_id: String,
        /// "agent" (subagent) | "bash" (backgrounded command).
        kind: String,
        /// "completed" | "failed" | "timed_out".
        status: String,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i64>,
    },
    /// A background task produced more output while still running: emitted
    /// from the bash poller when it appends a drained chunk to the task
    /// state, so the TS side can stream live output into TaskOutput (TS
    /// ProcessTask parity — TS streams chunks as they arrive; the engine's
    /// settle-only event left TaskOutput empty until the task ended).
    #[serde(rename = "task.output", rename_all = "camelCase")]
    TaskOutput {
        task_id: String,
        /// The exact chunk appended to the task state (utf8-lossy decoded,
        /// already capped by the result-builder limits).
        delta: String,
    },
}

/// Cloneable sink for task lifecycle events emitted by tools (subagent
/// workers / bash pollers — the settlement points). The bridge points it at
/// the session's per-event callback (`set_on_event`); before that it is a
/// no-op, so tools constructed without a wired sink stay silent.
#[derive(Clone, Default)]
pub struct EventSink {
    inner: std::sync::Arc<
        std::sync::Mutex<Option<std::sync::Arc<dyn Fn(EngineEvent) + Send + Sync>>>,
    >,
    /// Session-closed flag (TS `taskService.dispose` parity): once the
    /// owning session is torn down, `emit` stops forwarding so late settles
    /// from workers/pollers cannot fire into a disposed runner. Workers also
    /// poll it (`is_closed`) to stop their work and kill their processes.
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EventSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wire the sink to a callback (the bridge installs the session's
    /// per-event napi callback here; called once, before the turn runs).
    pub fn set(&self, sink: std::sync::Arc<dyn Fn(EngineEvent) + Send + Sync>) {
        *self.inner.lock().unwrap_or_else(|p| p.into_inner()) = Some(sink);
    }

    /// Mark the session closed: further `emit`s are dropped (TS parity — a
    /// disposed session suppresses terminal notifications).
    pub fn close(&self) {
        self.closed.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether the owning session was closed (workers/pollers poll this to
    /// stop background work and skip settlement events).
    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Emit one event through the wired callback (no-op before `set` or
    /// after `close`).
    pub fn emit(&self, event: EngineEvent) {
        if self.is_closed() {
            return;
        }
        let sink = self
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(sink) = sink {
            sink(event);
        }
    }
}

/// `ToolUpdate` — the streaming tool output shape (toolExecutorEvents.ts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ToolUpdate {
    #[serde(rename = "stdout")]
    Stdout { text: String },
    #[serde(rename = "stderr")]
    Stderr { text: String },
    #[serde(rename = "progress")]
    Progress {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        percent: Option<f64>,
    },
    #[serde(rename = "status")]
    Status { text: String },
}

/// Convenience: a turn's events collected for tests / the napi surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineEventBatch {
    pub events: Vec<EngineEvent>,
    pub outcome: super::types::TurnOutcome,
}
