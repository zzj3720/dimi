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
