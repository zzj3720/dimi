//! dimi-engine — turn orchestration core (M3).
//!
//! The engine reproduces the TS `loopService` minimal closed loop in Rust:
//! messages in → LLM stream → tool execution → messages out, emitting the
//! same engine event stream the transcript projection layer consumes.
//!
//! Slice 1 scope: turn loop, scripted/OpenAI-compatible LLM boundary, Bash
//! tool over dimi-exec, usage accumulation, max-steps control. Context
//! assembly, permission/approval, compaction, subagents and the remaining
//! tool domains land in later slices (see `DESIGN-slice1.md`).

pub mod aimux;
pub mod compaction;
pub mod context;
pub mod dedupe;
pub mod engine;
pub mod events;
pub mod llm;
pub mod permission;
pub mod tool;
pub mod types;

pub use engine::Engine;
pub use events::{EngineEvent, EngineEventBatch};
pub use types::{EngineTurnInput, TurnEndReason, TurnOutcome};
