//! Fixture round-trip tests: every fixture line must parse and re-serialize
//! byte-exactly (same JSON text), proving field names, ordering, optional
//! omission and number formatting all match the TS zod contract.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn assert_byte_exact_roundtrip<T: DeserializeOwned + Serialize>(name: &str, json: &str) {
    let parsed: T = serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("{name}: parse failed for {json:?}: {e}"));
    let reencoded = serde_json::to_string(&parsed).expect("serialize");
    assert_eq!(reencoded, json, "{name}: round-trip is not byte-exact");
}

fn check_jsonl<T: DeserializeOwned + Serialize + std::fmt::Debug>(file: &str) {
    let text = fs::read_to_string(fixtures_dir().join(file))
        .unwrap_or_else(|e| panic!("read {file}: {e}"));
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert!(!lines.is_empty(), "{file}: no fixture lines");
    for (i, line) in lines.iter().enumerate() {
        assert_byte_exact_roundtrip::<T>(&format!("{file}:{i}"), line);
    }
}

#[test]
fn items_roundtrip_byte_exact() {
    check_jsonl::<dimi_wire::Item>("items.jsonl");
}

#[test]
fn phases_roundtrip_byte_exact() {
    check_jsonl::<dimi_wire::AgentPhase>("phases.jsonl");
}
