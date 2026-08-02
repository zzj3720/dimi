//! `wire.jsonl` reading (`kap-server snapshotReader.readWireRecords`,
//! snapshotReader.ts 270–289).
//!
//! Line semantics, exactly:
//! - split on `\n`; strip ONE trailing `\r` per line (CRLF tolerance).
//! - empty lines (after `\r` strip) are skipped — this also digests the
//!   trailing newline's empty element.
//! - a JSON parse failure on the LAST element is a torn write: silently
//!   dropped (`break`).
//! - a parse failure on any other line is an error carrying the TS message
//!   shape (`wire.jsonl: corrupted line <1-based> in <path>: <cause>`).
//! - no schema validation at all: any JSON value is returned as-is (a
//!   `null` line passes through and only fails later in the reducer, exactly
//!   like TS).

use std::fs;
use std::path::Path;

use dimi_wire::record::WireRecord;

/// `readWireRecords(path)` — parse a whole wire log. Errors mirror the TS
/// `Error` thrown for corrupted middle lines.
pub fn read_wire_records(path: &Path) -> Result<Vec<WireRecord>, WireReadError> {
    let raw = fs::read_to_string(path).map_err(|e| WireReadError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let lines: Vec<&str> = raw.split('\n').collect();
    let mut records = Vec::new();
    for (i, raw_line) in lines.iter().enumerate() {
        let mut line = *raw_line;
        if let Some(stripped) = line.strip_suffix('\r') {
            line = stripped;
        }
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<WireRecord>(line) {
            Ok(record) => records.push(record),
            Err(parse_error) => {
                if i == lines.len() - 1 {
                    // Torn last line — silent drop.
                    break;
                }
                return Err(WireReadError::CorruptedLine {
                    line: i + 1,
                    path: path.display().to_string(),
                    message: parse_error.to_string(),
                });
            }
        }
    }
    Ok(records)
}

#[derive(Debug)]
pub enum WireReadError {
    Io {
        path: String,
        source: std::io::Error,
    },
    CorruptedLine {
        line: usize,
        path: String,
        message: String,
    },
}

impl std::fmt::Display for WireReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireReadError::Io { path, source } => {
                write!(f, "wire.jsonl: read failed for {path}: {source}")
            }
            WireReadError::CorruptedLine {
                line,
                path,
                message,
            } => {
                write!(f, "wire.jsonl: corrupted line {line} in {path}: {message}")
            }
        }
    }
}

impl std::error::Error for WireReadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_wire::record::RecordTime;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique file per call: tests run in parallel threads, and two
    /// `SystemTime::now()` calls can collide on the same nanosecond, which
    /// made concurrent tests truncate each other's fixture files.
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn write_tmp(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dimi-wire-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("wire-{}-{seq}.jsonl", std::process::id()));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn parses_lines_and_skips_empty() {
        let p = write_tmp("{\"type\":\"a\",\"time\":1}\n\n{\"type\":\"b\"}\n");
        let records = read_wire_records(&p).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].r#type, "a");
        assert_eq!(records[0].time, Some(RecordTime::Ms(1)));
        assert_eq!(records[1].r#type, "b");
    }

    #[test]
    fn strips_one_crlf() {
        let p = write_tmp("{\"type\":\"a\"}\r\n{\"type\":\"b\"}\r\n");
        let records = read_wire_records(&p).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].r#type, "b");
    }

    #[test]
    fn torn_last_line_is_dropped() {
        let p = write_tmp("{\"type\":\"a\"}\n{\"type\":\"b\"}\n{\"broken");
        let records = read_wire_records(&p).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn corrupted_middle_line_errors() {
        let p = write_tmp("{\"type\":\"a\"}\n{\"broken\n{\"type\":\"b\"}\n");
        let err = read_wire_records(&p).unwrap_err();
        assert!(err.to_string().contains("corrupted line 2"), "{err}");
    }
}
