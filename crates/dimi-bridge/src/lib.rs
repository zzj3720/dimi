//! `dimi-bridge` — the napi-rs socket between Node and the Rust runtime.
//!
//! M0.5 role: expose [`dimi_wire`]'s contract mirror to TS so the
//! differential harness can compare zod's normalization against Rust's on
//! the same documents. Later milestones extend this crate with the migrated
//! modules (store, exec, engine) behind feature/switch gates; the bridge is
//! retired at M6 when the TS side is deleted.
//!
//! Functions are intentionally thin: parse → re-serialize (normalize), or a
//! predicate. All contract knowledge lives in `dimi-wire` (and its mirror
//! discipline), never in this crate.

use napi_derive::napi;

mod env;
mod exec;
mod fs;

use dimi_store::AgentTranscript;
use dimi_store::paginate::TurnPageQuery;
use dimi_store::store::SnapshotWindow;

fn normalize<T>(json: &str) -> napi::Result<String>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let value: T = serde_json::from_str(json).map_err(wire_error)?;
    serde_json::to_string(&value).map_err(wire_error)
}

fn wire_error(err: serde_json::Error) -> napi::Error {
    napi::Error::from_reason(format!("dimi-wire: {err}"))
}

/// Parse one transcript item and re-serialize it in canonical (schema) order.
/// Throws with a `dimi-wire:` message when the document does not satisfy the
/// contract (same rejections as the zod schemas: `null` optionals, empty
/// ids, unknown `kind` tags, …).
#[napi]
pub fn normalize_item(json: String) -> napi::Result<String> {
    normalize::<dimi_wire::Item>(&json)
}

/// Parse one transcript step and re-serialize it canonically.
#[napi]
pub fn normalize_step(json: String) -> napi::Result<String> {
    normalize::<dimi_wire::Step>(&json)
}

/// Parse one task and re-serialize it canonically.
#[napi]
pub fn normalize_task(json: String) -> napi::Result<String> {
    normalize::<dimi_wire::Task>(&json)
}

/// Parse one agent phase (`meta.agent.phase`) and re-serialize it
/// canonically.
#[napi]
pub fn normalize_phase(json: String) -> napi::Result<String> {
    normalize::<dimi_wire::AgentPhase>(&json)
}

/// `isPlainAgentId` — filename-safe agent id check (mirrors the zod-side
/// helper in the transcript contract).
#[napi]
pub fn is_plain_agent_id(id: String) -> bool {
    dimi_wire::is_plain_agent_id(&id)
}

// ---------------------------------------------------------------- dimi-store (M1)

/// Full cold rebuild: wire records → reduced messages → turn tree → folded
/// facts (mirrors `reduceContextTranscript` + `groupMessagesIntoSnapshot` +
/// `foldWireRecordFacts`). Input: JSON array of wire records; output: the
/// `AgentTranscriptSnapshot` JSON.
#[napi]
pub fn cold_rebuild(records_json: String) -> napi::Result<String> {
    let records: Vec<dimi_wire::record::WireRecord> =
        serde_json::from_str(&records_json).map_err(wire_error)?;
    let reduced = dimi_store::reduce_context_transcript(&records)
        .map_err(|e| napi::Error::from_reason(format!("dimi-store: {}", e.message)))?;
    let base = dimi_store::group_messages_into_snapshot(&reduced.entries);
    let snapshot = dimi_store::fold_wire_record_facts(&records, &base);
    serde_json::to_string(&snapshot).map_err(wire_error)
}

/// `paginateTurns` — page items by turn cursor. Inputs: items JSON array and
/// a `TurnPageQuery` JSON; output: `TurnPage` JSON.
#[napi]
pub fn paginate_turns(items_json: String, query_json: String) -> napi::Result<String> {
    let items: Vec<dimi_wire::item::Item> =
        serde_json::from_str(&items_json).map_err(wire_error)?;
    let query: TurnPageQuery = serde_json::from_str(&query_json).map_err(wire_error)?;
    let page = dimi_store::paginate_turns(&items, query);
    serde_json::to_string(&page).map_err(wire_error)
}

/// `readWireRecords` — parse a `wire.jsonl` file with the TS line semantics
/// (torn last line dropped, corrupted middle line errors). Output: JSON
/// array of records.
#[napi]
pub fn read_wire_records(path: String) -> napi::Result<String> {
    let records = dimi_store::read_wire_records(std::path::Path::new(&path))
        .map_err(|e| napi::Error::from_reason(format!("dimi-store: {e}")))?;
    serde_json::to_string(&records).map_err(wire_error)
}

/// One agent's transcript store, held on the Rust side. The swap-in socket
/// for the kap-server `TranscriptService` storage backend (`DIMI_RUST_STORE`).
#[napi]
pub struct RustAgentTranscript {
    inner: AgentTranscript,
}

#[napi]
impl RustAgentTranscript {
    #[napi(constructor)]
    pub fn new(agent_id: String) -> Self {
        RustAgentTranscript {
            inner: AgentTranscript::new(agent_id),
        }
    }

    /// Apply an op batch (JSON array of operations). Returns `AppliedOps`
    /// JSON: `{ accepted, gap? }` — gap ops are dropped, only the last gap
    /// is reported.
    #[napi]
    pub fn apply(&mut self, ops_json: String) -> napi::Result<String> {
        let ops: Vec<dimi_wire::op::Operation> =
            serde_json::from_str(&ops_json).map_err(wire_error)?;
        let result = self.inner.apply(&ops);
        serde_json::to_string(&result).map_err(wire_error)
    }

    /// Materialize the current state. Optional window JSON:
    /// `{ tailTurns: number }`.
    #[napi]
    pub fn snapshot(&self, window_json: Option<String>) -> napi::Result<String> {
        let window = match window_json {
            Some(json) => {
                let window: SnapshotWindow = serde_json::from_str(&json).map_err(wire_error)?;
                Some(window)
            }
            None => None,
        };
        let snapshot = self.inner.snapshot(window);
        serde_json::to_string(&snapshot).map_err(wire_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_item_roundtrips_fixture_shape() {
        let json = r#"{"kind":"turn","turnId":"t_1","ordinal":0,"state":"completed","origin":{"kind":"user"},"steps":[]}"#;
        assert_eq!(normalize_item(json.into()).unwrap(), json);
    }

    #[test]
    fn normalize_item_rejects_null_optional() {
        let json = r#"{"kind":"turn","turnId":"t_1","ordinal":0,"state":"completed","origin":{"kind":"user"},"prompt":null,"steps":[]}"#;
        assert!(normalize_item(json.into()).is_err());
    }

    #[test]
    fn is_plain_agent_id_agrees_with_wire() {
        assert!(is_plain_agent_id("agent_1".into()));
        assert!(!is_plain_agent_id("../escape".into()));
    }
}
