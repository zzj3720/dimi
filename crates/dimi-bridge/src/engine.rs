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

use napi_derive::napi;

use dimi_engine::aimux::{AimuxLlmClient, unimplemented_model};
use dimi_engine::events::EngineEvent;
use dimi_engine::llm::{LlmClient, LlmStreamEvent, ScriptedLlmClient};
use dimi_engine::tool::{BashTool, ToolExecutor};
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
                model: Box::new(unimplemented_model()),
            }),
        };
        let tools: Box<dyn ToolExecutor> = Box::new(BashTool);

        let mut events: Vec<EngineEvent> = Vec::new();
        let outcome = self
            .inner
            .run_turn(&input, llm.as_ref(), tools.as_ref(), &mut |event| {
                events.push(event);
            })
            .await;

        let batch = dimi_engine::events::EngineEventBatch { events, outcome };
        serde_json::to_string(&batch).map_err(wire_error)
    }
}
