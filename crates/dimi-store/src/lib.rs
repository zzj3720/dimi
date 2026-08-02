//! `dimi-store` — the event-sourced transcript store (M1).
//!
//! Byte-exact Rust mirror of the TS storage stack:
//!
//! | TS side | dimi-store |
//! |---|---|
//! | `packages/transcript/src/store/agentTranscript.ts` | [`store::AgentTranscript`] |
//! | `packages/transcript/src/ops/apply.ts` | [`apply`] (14-op reducer) |
//! | `packages/transcript/src/history/groupTurns.ts` | [`group`] |
//! | `packages/transcript/src/history/foldFacts.ts` | [`fold`] |
//! | `agent-core-v2 contextMemory/contextTranscript.ts` | [`reduce`] |
//! | `kap-server snapshotReader.readWireRecords` | [`wire::read_wire_records`] |
//! | `packages/transcript/src/pagination/paginate.ts` | [`paginate`] |
//!
//! ## Mirror discipline (M1)
//!
//! - State converges exactly like TS; `changed` flags may differ in the
//!   reference-vs-value equality corner (TS uses reference equality for
//!   nested payloads, we compare by value): a re-serialized identical op is
//!   a no-change here but a replace+event in TS. State results are identical.
//! - Op application is strictly sequential, in arrival order — never
//!   reordered (see `apply.ts` spec in PLAN.md for why).
//! - The cold-rebuild path mirrors the SERVER transcript path: it does NOT
//!   run the wire migrations (those only happen in the engine restore path).
//! - `reduce` mirrors the snapshot reducer, including its documented
//!   divergences from the live fold (interrupted-tool messages only on the
//!   next `step.begin`, undo keeps trailing injections, compaction keeps all
//!   history + summary marker, clear only raises the floor).

pub mod apply;
pub mod fold;
pub mod group;
pub mod paginate;
pub mod reduce;
pub mod state;
pub mod store;
pub mod wire;

pub use apply::{append_at_offset, apply_operation, item_id_of};
pub use fold::fold_wire_record_facts;
pub use group::group_messages_into_snapshot;
pub use paginate::paginate_turns;
pub use reduce::reduce_context_transcript;
pub use state::{AgentState, empty_agent_state};
pub use store::AgentTranscript;
pub use wire::read_wire_records;

/// `HistoryWireRecord` alias — one parsed `wire.jsonl` line.
pub type WireRecord = dimi_wire::record::WireRecord;

/// `HistoryMessage` alias — one folded context message.
pub type HistoryMessage = dimi_wire::record::HistoryMessage;
