//! `toolDedupe` domain — TS `agent/toolDedupe/toolDedupeService.ts` parity.
//!
//! Ports the TS dedupe behavior into the engine's step lifecycle:
//!
//! - **Same-step dedupe**: identical tool calls (same name + canonical args)
//!   within one step are suppressed after the first; the duplicate is
//!   announced (`tool.call.started`) but never executed and settles with the
//!   first call's finalized result (`tool.result`), exactly like the TS veto
//!   path (the executor dispatches the vetoed call with a placeholder result
//!   and `finalizeResult` swaps in the shared deferred result).
//! - **Cross-step reminders**: a key repeated in consecutive steps gets a
//!   `<system-reminder>` appended to the tool RESULT output — the 3rd
//!   occurrence onward uses `REMINDER_TEXT_1`, the 5th `makeReminderText2`,
//!   the 8th `REMINDER_TEXT_3` — and 12 consecutive repeats
//!   (`REPEAT_FORCE_STOP_STREAK`) force-stop the turn. The reminder lands on
//!   the result the model sees on the next step (TS `appendReminder`).
//!
//! The state is per-turn: the engine creates one `TurnSession` per turn, so
//! the TS `activeTurnId` reset of the consecutive streak is implicit.
//! Telemetry (`tool_call_dedup_detected` / `tool_call_repeat`) and
//! `toolExecutor.recordDupType` are not ported — the engine has no telemetry
//! boundary — but the suppression and reminder behavior is identical.

use std::collections::HashMap;

use crate::tool::ToolResult;

// TS `REPEAT_REMINDER_*` / `REPEAT_FORCE_STOP_STREAK` constants.
pub const REPEAT_REMINDER_1_START: u32 = 3;
pub const REPEAT_REMINDER_2_START: u32 = 5;
pub const REPEAT_REMINDER_3_START: u32 = 8;
pub const REPEAT_FORCE_STOP_STREAK: u32 = 12;

/// Byte-for-byte TS `REMINDER_TEXT_1` (leading `\n\n`, `<system-reminder>`
/// wrapping and trailing newline included).
pub const REMINDER_TEXT_1: &str = "\n\n<system-reminder>\n\
The same tool call has been repeated several times in a row. \
Before making your next call, write one sentence stating what new information you expect it to produce. \
Then act on that sentence: if it names something this result does not already give you, choose the action that best provides it; otherwise, continue with the evidence you already have.\
\n</system-reminder>";

/// Byte-for-byte TS `REMINDER_TEXT_3`.
pub const REMINDER_TEXT_3: &str = "\n\n<system-reminder>\n\
Write your final response now, without any further tool calls. \
Cover: the current blocker, each approach you have tried and what it established, and the specific information or decision you need from the user to unblock progress. \
Text only.\
\n</system-reminder>";

/// TS `makeReminderText2(repeatCount)` — the reminder carries the current
/// streak count.
pub fn make_reminder_text_2(repeat_count: u32) -> String {
    format!(
        "\n\n<system-reminder>\n\
The same tool call has now been issued {repeat_count} times in a row. \
Choose exactly one of the following and state your choice before acting:\n\
(1) Falsification check: run the cheapest test that could conclusively disprove your current approach, if such a test exists.\n\
(2) Missing input: tell the user precisely what information or decision you need to proceed, and ask for it.\n\
(3) Conclude: deliver your best result based on the evidence already gathered, listing anything that remains uncertain.\
\n</system-reminder>"
    )
}

/// TS `canonicalTelemetryArgs` parity: `JSON.stringify(sortJsonValue(value))`.
///
/// Object keys are sorted recursively (arrays keep their order, elements are
/// recursed). The result is the dedupe key suffix (`make_key`), internal-only
/// — byte-exactness with TS matters only where equal values must collide and
/// different values must not. One deliberate alignment: JS renders integral
/// floats as integers (`1.0` → `"1"`) while serde_json keeps the `.0`, so
/// integral floats are normalized to match (the model may emit `1` one step
/// and `1.0` the next; TS treats them as the same key).
pub fn canonical_args(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(canonical_args).collect();
            format!("[{}]", inner.join(","))
        }
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(String, String)> = map
                .iter()
                .map(|(key, value)| (key.clone(), canonical_args(value)))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let inner: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{}:{value}", json_string(key)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        serde_json::Value::Number(number) => canonical_number(number),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::String(s) => json_string(s),
    }
}

/// `JSON.stringify`-style string serialization (quotes + JSON escaping).
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s.replace('"', "\\\"")))
}

/// `JSON.stringify` number parity: integral floats render as integers; other
/// floats use serde_json's shortest round-trip form (same as JS for the
/// common cases); integers serialize exactly (`is_f64()` distinguishes
/// literals like `1.0` from integer `1`, so a huge u64 never round-trips
/// through f64).
fn canonical_number(number: &serde_json::Number) -> String {
    if number.is_f64() {
        if let Some(f) = number.as_f64() {
            if f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                return (f as i64).to_string();
            }
            return format!("{f}");
        }
    }
    number.to_string()
}

/// The dedupe key: `${toolName} ${canonicalTelemetryArgs(args)}` (TS
/// `makeKey`).
fn make_key(tool_name: &str, args: &serde_json::Value) -> String {
    format!("{tool_name} {}", canonical_args(args))
}

/// Verdict of `DedupeState::check` for one tool call.
#[derive(Debug, Clone, PartialEq)]
pub enum DedupeCheck {
    /// First occurrence of the key in this step — execute normally.
    Original,
    /// Same-step duplicate — suppress execution and settle with the
    /// original's finalized result (`DedupeState::shared_result`).
    Duplicate { key: String },
}

/// Per-turn tool-call dedupe state (the TS `AgentToolDedupeService` mutable
/// fields). Same-step sharing uses the original's finalized result directly
/// instead of the TS promise `stepDeferreds`: batch execution is strictly
/// sequential in the engine, so a duplicate always follows its original and
/// the shared value is already available.
#[derive(Debug, Default)]
pub struct DedupeState {
    /// Keys in dispatch order for the current step (TS `stepCalls`).
    step_calls: Vec<String>,
    /// Keys seen so far in the current step (TS `stepDeferreds` membership):
    /// a duplicate is detected as soon as an earlier call recorded the key,
    /// not when that call finishes.
    step_keys: std::collections::HashSet<String>,
    /// call_id -> position in `step_calls` for original (non-synthetic) calls
    /// (TS `originalCallIndex`).
    original_index: HashMap<String, usize>,
    /// key -> finalized result of the first call in this step (TS
    /// `stepDeferreds`, resolved at finalize time).
    step_results: HashMap<String, ToolResult>,
    /// TS `consecutiveKey` / `consecutiveCount` — the cross-step streak,
    /// updated once per step in `end_step`.
    consecutive_key: Option<String>,
    consecutive_count: u32,
}

impl DedupeState {
    /// TS `beginStep`: reset the per-step state at the start of every step.
    /// The consecutive streak survives (it spans steps).
    pub fn begin_step(&mut self) {
        self.step_calls.clear();
        self.step_keys.clear();
        self.original_index.clear();
        self.step_results.clear();
    }

    /// TS `checkToolCall` (the `onBeforeExecuteTool` veto listener): record
    /// the call and detect a same-step duplicate. Runs before the policy
    /// gate, matching the TS veto ordering (the veto fires before the allow
    /// decision, so a duplicate is suppressed even if the original would be
    /// denied).
    pub fn check(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> DedupeCheck {
        let key = make_key(tool_name, args);
        let index = self.step_calls.len();
        self.step_calls.push(key.clone());
        if !self.step_keys.insert(key.clone()) {
            return DedupeCheck::Duplicate { key };
        }
        self.original_index.insert(tool_call_id.to_string(), index);
        DedupeCheck::Original
    }

    /// The shared result for a same-step duplicate — the original call's
    /// finalized result (with any cross-step reminder already appended).
    /// `None` only when the original's result was lost, which cannot happen
    /// with sequential batch execution (TS resolves the deferred with an
    /// error result at the next `beginStep` instead).
    pub fn shared_result(&self, key: &str) -> Option<&ToolResult> {
        self.step_results.get(key)
    }

    /// TS `finalizeResult` for an original call: compute the cross-step
    /// streak at this call's position in the step, append the matching
    /// system-reminder to the result output (3rd/5th/8th occurrence; 12th
    /// also forces the turn to stop), and record the result so same-step
    /// duplicates share it. Returns the (possibly modified) result.
    pub fn finalize_result(&mut self, tool_call_id: &str, mut result: ToolResult) -> ToolResult {
        let Some(index) = self.original_index.remove(tool_call_id) else {
            return result;
        };
        let Some(key) = self.step_calls.get(index).cloned() else {
            return result;
        };

        let streak = self.streak_at(index);
        if streak >= REPEAT_FORCE_STOP_STREAK {
            // TS `forceStopResult`: append REMINDER_TEXT_3 and stop the turn.
            result.output.push_str(REMINDER_TEXT_3);
            result.stop_turn = true;
        } else if streak >= REPEAT_REMINDER_3_START {
            result.output.push_str(REMINDER_TEXT_3);
        } else if streak >= REPEAT_REMINDER_2_START {
            result.output.push_str(&make_reminder_text_2(streak));
        } else if streak >= REPEAT_REMINDER_1_START {
            result.output.push_str(REMINDER_TEXT_1);
        }
        self.step_results.insert(key, result.clone());
        result
    }

    /// TS `endStep`: fold every key of the finished step into the cross-step
    /// streak (duplicates count too — a step that called the same tool twice
    /// advances the streak by two).
    pub fn end_step(&mut self) {
        for key in &self.step_calls {
            if self.consecutive_key.as_deref() == Some(key.as_str()) {
                self.consecutive_count += 1;
            } else {
                self.consecutive_key = Some(key.clone());
                self.consecutive_count = 1;
            }
        }
    }

    /// The streak value as of `index` in `step_calls` — TS `finalizeResult`
    /// walks `stepCalls[0..=index]` from the stored `consecutiveKey` /
    /// `consecutiveCount` for every call, because the consecutive state only
    /// advances at `endStep`.
    fn streak_at(&self, index: usize) -> u32 {
        let mut last_key = self.consecutive_key.clone();
        let mut streak = self.consecutive_count;
        for key in &self.step_calls[0..=index] {
            if Some(key) == last_key.as_ref() {
                streak += 1;
            } else {
                last_key = Some(key.clone());
                streak = 1;
            }
        }
        streak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_args_sorts_object_keys_recursively() {
        let a = serde_json::json!({"b": 1, "a": {"z": 2, "y": 1}});
        let b = serde_json::json!({"a": {"y": 1, "z": 2}, "b": 1});
        assert_eq!(canonical_args(&a), canonical_args(&b));
        assert_eq!(canonical_args(&a), r#"{"a":{"y":1,"z":2},"b":1}"#);
    }

    #[test]
    fn canonical_args_preserves_array_order() {
        let a = serde_json::json!({"list": [{"b": 1, "a": 2}, 3]});
        let b = serde_json::json!({"list": [{"a": 2, "b": 1}, 3]});
        assert_eq!(canonical_args(&a), canonical_args(&b));
        assert_eq!(canonical_args(&a), r#"{"list":[{"a":2,"b":1},3]}"#);
        let different = serde_json::json!({"list": [3, {"a": 2, "b": 1}]});
        assert_ne!(canonical_args(&a), canonical_args(&different));
    }

    #[test]
    fn canonical_args_normalizes_integral_floats() {
        // JS `JSON.stringify(1.0)` → "1": the model may emit `1` one step
        // and `1.0` the next — TS treats them as the same dedupe key.
        assert_eq!(
            canonical_args(&serde_json::json!({"n": 1.0})),
            canonical_args(&serde_json::json!({"n": 1}))
        );
        assert_eq!(
            canonical_args(&serde_json::json!({"n": 1.5})),
            r#"{"n":1.5}"#
        );
    }

    #[test]
    fn make_key_separates_name_and_args() {
        let key = make_key("Bash", &serde_json::json!({"command": "echo hi"}));
        assert_eq!(key, r#"Bash {"command":"echo hi"}"#);
        let other = make_key("Bash", &serde_json::json!({"command": "echo bye"}));
        assert_ne!(key, other);
    }

    #[test]
    fn same_step_duplicate_shares_and_end_step_counts_both() {
        let mut dedupe = DedupeState::default();
        dedupe.begin_step();
        let first = dedupe.check("call_1", "Bash", &serde_json::json!({"command": "echo hi"}));
        assert_eq!(first, DedupeCheck::Original);
        let second = dedupe.check("call_2", "Bash", &serde_json::json!({"command": "echo hi"}));
        match second {
            DedupeCheck::Duplicate { key } => assert_eq!(
                key,
                r#"Bash {"command":"echo hi"}"#
            ),
            DedupeCheck::Original => panic!("second identical call must be a duplicate"),
        }
        let result = ToolResult {
            tool_call_id: "call_1".to_string(),
            tool_name: "Bash".to_string(),
            output: "hi".to_string(),
            is_error: false,
            stop_turn: false,
            updates: vec![],
        };
        let finalized = dedupe.finalize_result("call_1", result);
        assert_eq!(finalized.output, "hi");
        let shared = dedupe
            .shared_result(r#"Bash {"command":"echo hi"}"#)
            .expect("shared result must be recorded");
        assert_eq!(shared.output, "hi");
        assert_eq!(shared.tool_call_id, "call_1");

        dedupe.end_step();
        assert_eq!(dedupe.consecutive_key.as_deref(), Some(r#"Bash {"command":"echo hi"}"#));
        assert_eq!(dedupe.consecutive_count, 2);
    }

    #[test]
    fn different_args_are_not_duplicates() {
        let mut dedupe = DedupeState::default();
        dedupe.begin_step();
        let first = dedupe.check("call_1", "Bash", &serde_json::json!({"command": "echo a"}));
        let second = dedupe.check("call_2", "Bash", &serde_json::json!({"command": "echo b"}));
        assert_eq!(first, DedupeCheck::Original);
        assert_eq!(second, DedupeCheck::Original);
    }

    #[test]
    fn cross_step_streak_breaks_on_a_different_key() {
        let mut dedupe = DedupeState::default();
        dedupe.begin_step();
        dedupe.check("c1", "Bash", &serde_json::json!({"command": "echo a"}));
        dedupe.end_step();
        dedupe.begin_step();
        dedupe.check("c2", "Bash", &serde_json::json!({"command": "echo a"}));
        dedupe.end_step();
        assert_eq!(dedupe.consecutive_count, 2);
        dedupe.begin_step();
        dedupe.check("c3", "Bash", &serde_json::json!({"command": "echo b"}));
        dedupe.end_step();
        assert_eq!(dedupe.consecutive_count, 1);
        assert_eq!(
            dedupe.consecutive_key.as_deref(),
            Some(r#"Bash {"command":"echo b"}"#)
        );
    }

    #[test]
    fn reminders_fire_at_3_5_8_and_stop_at_12() {
        let mut dedupe = DedupeState::default();
        let key = r#"Bash {"command":"echo x"}"#;
        let mut outputs = Vec::new();
        for step in 1..=12u32 {
            dedupe.begin_step();
            let call_id = format!("call_{step}");
            let check = dedupe.check(&call_id, "Bash", &serde_json::json!({"command": "echo x"}));
            assert_eq!(check, DedupeCheck::Original);
            let result = ToolResult {
                tool_call_id: call_id,
                tool_name: "Bash".to_string(),
                output: "out".to_string(),
                is_error: false,
                stop_turn: false,
                updates: vec![],
            };
            outputs.push(dedupe.finalize_result(&format!("call_{step}"), result));
            dedupe.end_step();
            assert_eq!(dedupe.consecutive_count, step);
            assert_eq!(dedupe.consecutive_key.as_deref(), Some(key));
        }
        assert_eq!(outputs[0].output, "out");
        assert_eq!(outputs[1].output, "out");
        assert_eq!(outputs[2].output, format!("out{REMINDER_TEXT_1}"));
        assert_eq!(outputs[3].output, format!("out{REMINDER_TEXT_1}"));
        assert_eq!(outputs[4].output, format!("out{}", make_reminder_text_2(5)));
        assert_eq!(outputs[5].output, format!("out{}", make_reminder_text_2(6)));
        assert_eq!(outputs[6].output, format!("out{}", make_reminder_text_2(7)));
        assert_eq!(outputs[7].output, format!("out{REMINDER_TEXT_3}"));
        for output in &outputs[8..11] {
            assert_eq!(output.output, format!("out{REMINDER_TEXT_3}"));
        }
        // The 12th forces the turn to stop.
        assert!(!outputs[10].stop_turn);
        assert_eq!(outputs[11].output, format!("out{REMINDER_TEXT_3}"));
        assert!(outputs[11].stop_turn);
    }

    #[test]
    fn reminder_text_matches_ts_byte_for_byte() {
        // Pins the exact TS strings (leading `\n\n`, `<system-reminder>`
        // markers, trailing `\n</system-reminder>`).
        assert!(REMINDER_TEXT_1.starts_with("\n\n<system-reminder>\n"));
        assert!(REMINDER_TEXT_1.ends_with("\n</system-reminder>"));
        assert!(REMINDER_TEXT_1.contains(
            "The same tool call has been repeated several times in a row. "
        ));
        assert!(REMINDER_TEXT_1.contains(
            "continue with the evidence you already have.\n</system-reminder>"
        ));

        let text_2 = make_reminder_text_2(5);
        assert!(text_2.starts_with("\n\n<system-reminder>\n"));
        assert!(text_2.ends_with("\n</system-reminder>"));
        assert!(text_2.contains(
            "The same tool call has now been issued 5 times in a row. "
        ));
        assert!(text_2.contains(
            "(3) Conclude: deliver your best result based on the evidence already gathered, listing anything that remains uncertain.\n</system-reminder>"
        ));

        assert!(REMINDER_TEXT_3.starts_with("\n\n<system-reminder>\n"));
        assert!(REMINDER_TEXT_3.ends_with("\n</system-reminder>"));
        assert!(REMINDER_TEXT_3.contains("Write your final response now, without any further tool calls. "));
        assert!(REMINDER_TEXT_3.contains("Text only.\n</system-reminder>"));
    }
}
