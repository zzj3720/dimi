//! `reduceContextTranscript` — wire journal → folded context messages
//! (`agent-core-v2/src/agent/contextMemory/contextTranscript.ts`).
//!
//! A stateful single-pass reducer over `context.*` records. Preserves the
//! snapshot-reducer's documented divergences from the live fold:
//! interrupted-tool messages are only emitted on the NEXT `step.begin`,
//! `undo` keeps trailing injections, `apply_compaction` keeps ALL history
//! plus a summary marker, `clear` only raises the floor.

use std::collections::{HashMap, HashSet};

use dimi_wire::record::{HistoryMessage, MessageOrigin, ToolCall, WireRecord};

/// `TOOL_INTERRUPTED_ON_RESUME_OUTPUT` (contextTranscript.ts 17–18).
pub const TOOL_INTERRUPTED_ON_RESUME_OUTPUT: &str = "Tool execution was interrupted before its result was recorded. Do not assume the tool completed successfully.";

#[derive(Debug, Clone, PartialEq)]
pub struct ContextTranscript {
    /// Folded message sequence (contains compaction history — never
    /// truncated, unlike the live model).
    pub entries: Vec<HistoryMessage>,
    /// Parallel per-message timestamps (the creating record's `time`).
    pub times: Vec<Option<i64>>,
    /// Folded-length ledger.
    pub folded_length: i64,
}

#[derive(Debug)]
pub struct ReduceError {
    pub message: String,
}

impl std::fmt::Display for ReduceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ReduceError {}

#[derive(Debug, Clone)]
struct MutableEntry {
    message: HistoryMessage,
    time: Option<i64>,
}

/// `reduceContextTranscript` (contextTranscript.ts 46–50).
pub fn reduce_context_transcript(records: &[WireRecord]) -> Result<ContextTranscript, ReduceError> {
    let mut reducer = ContextTranscriptReducer::default();
    for record in records {
        reducer.add(record)?;
    }
    Ok(reducer.result())
}

#[derive(Default)]
struct ContextTranscriptReducer {
    transcript: Vec<MutableEntry>,
    folded_length: i64,
    clear_floor: usize,
    open_steps: HashMap<String, usize>,
    pending_tool_result_ids: HashSet<String>,
    deferred: Vec<MutableEntry>,
    last_open_step_uuid: Option<String>,
}

impl ContextTranscriptReducer {
    fn add(&mut self, record: &WireRecord) -> Result<(), ReduceError> {
        match record.r#type.as_str() {
            "context.append_message" => {
                let entry = self.to_mutable_entry(record)?;
                if self.pending_tool_result_ids.is_empty() {
                    self.push(entry);
                } else {
                    self.deferred.push(entry);
                }
            }
            "context.append_loop_event" => {
                let event = record.rest.get("event").ok_or_else(|| ReduceError {
                    message: "context.append_loop_event: missing event".into(),
                })?;
                self.apply_loop_event(event, record.time.as_ref().and_then(|t| t.as_ms()))?;
            }
            "context.apply_compaction" => self.apply_compaction(record)?,
            "context.undo" => {
                let count = record.rest.get("count").and_then(|v| v.as_i64());
                self.apply_undo(count);
            }
            "context.clear" => {
                self.clear_floor = self.transcript.len();
                self.folded_length = 0;
                self.reset_open_state();
            }
            _ => {}
        }
        Ok(())
    }

    fn result(&self) -> ContextTranscript {
        ContextTranscript {
            entries: self.transcript.iter().map(|e| e.message.clone()).collect(),
            times: self.transcript.iter().map(|e| e.time).collect(),
            folded_length: self.folded_length,
        }
    }

    fn push(&mut self, entry: MutableEntry) {
        self.folded_length += 1;
        self.transcript.push(entry);
    }

    /// `toMutableEntry` (contextTranscript.ts 238–251): field passthrough,
    /// content/toolCalls copied shallowly; unknown message fields dropped by
    /// serde (no `deny_unknown_fields`).
    fn to_mutable_entry(&self, record: &WireRecord) -> Result<MutableEntry, ReduceError> {
        let message = record.rest.get("message").ok_or_else(|| ReduceError {
            message: "context.append_message: missing message".into(),
        })?;
        let message: HistoryMessage =
            serde_json::from_value(message.clone()).map_err(|e| ReduceError {
                message: format!("context.append_message: bad message: {e}"),
            })?;
        Ok(MutableEntry {
            message,
            time: record.time.as_ref().and_then(|t| t.as_ms()),
        })
    }

    fn apply_loop_event(
        &mut self,
        event: &serde_json::Value,
        time: Option<i64>,
    ) -> Result<(), ReduceError> {
        let Some(event_type) = event.get("type").and_then(|v| v.as_str()) else {
            return Ok(());
        };
        // step.begin/step.end key by `uuid`; content.part/tool.call key by
        // `stepUuid` (contextTranscript.ts 110–141).
        let uuid = event
            .get("uuid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let step_uuid = event
            .get("stepUuid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        match event_type {
            "step.begin" => {
                self.close_pending_tool_results(time);
                self.settle_step(self.last_open_step_uuid.clone());
                let message = HistoryMessage {
                    id: None,
                    role: "assistant".into(),
                    content: Some(Vec::new()),
                    tool_calls: Some(Vec::new()),
                    tool_call_id: None,
                    is_error: None,
                    origin: None,
                };
                self.push(MutableEntry { message, time });
                self.open_steps
                    .insert(uuid.clone(), self.transcript.len() - 1);
                self.last_open_step_uuid = Some(uuid);
            }
            "step.end" => {
                self.settle_step(Some(uuid.clone()));
                if self.last_open_step_uuid.as_deref() == Some(uuid.as_str()) {
                    self.last_open_step_uuid = None;
                }
                self.flush_deferred_if_tool_exchange_closed();
            }
            "content.part" => {
                if let Some(&index) = self.open_steps.get(&step_uuid) {
                    if let Some(part) = event.get("part") {
                        self.transcript[index]
                            .message
                            .content
                            .get_or_insert_with(Vec::new)
                            .push(part.clone());
                    }
                }
            }
            "tool.call" => {
                if let Some(&index) = self.open_steps.get(&step_uuid) {
                    let tool_call_id = event
                        .get("toolCallId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned();
                    let name = event
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned();
                    // `arguments === undefined ? null : JSON.stringify(arguments)`
                    // — only a MISSING key is null; a JSON `null` value
                    // stringifies to "null". The event field is `args`
                    // (contextTranscript.ts 126).
                    let arguments = event.get("args").map(|v| v.to_string());
                    self.transcript[index]
                        .message
                        .tool_calls
                        .get_or_insert_with(Vec::new)
                        .push(ToolCall {
                            id: tool_call_id.clone(),
                            name,
                            arguments,
                        });
                    self.pending_tool_result_ids.insert(tool_call_id);
                }
            }
            "tool.result" => {
                let tool_call_id = event
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                if !self.pending_tool_result_ids.contains(&tool_call_id) {
                    return Ok(());
                }
                let result = event.get("result");
                let output = result.and_then(|r| r.get("output"));
                let content = raw_tool_result_content(output);
                let is_error = result
                    .and_then(|r| r.get("isError"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.push(MutableEntry {
                    message: HistoryMessage {
                        id: None,
                        role: "tool".into(),
                        content: Some(content),
                        tool_calls: Some(Vec::new()),
                        tool_call_id: Some(tool_call_id.clone()),
                        is_error: Some(is_error),
                        origin: None,
                    },
                    time,
                });
                self.pending_tool_result_ids.remove(&tool_call_id);
                self.flush_deferred_if_tool_exchange_closed();
            }
            _ => {}
        }
        Ok(())
    }

    /// `closePendingToolResults` (contextTranscript.ts 70–87): every pending
    /// tool result becomes an interrupted-tool message; then flush deferred.
    fn close_pending_tool_results(&mut self, time: Option<i64>) {
        if self.pending_tool_result_ids.is_empty() {
            return;
        }
        let ids: Vec<String> = self.pending_tool_result_ids.iter().cloned().collect();
        for tool_call_id in ids {
            self.push(MutableEntry {
                message: HistoryMessage {
                    id: None,
                    role: "tool".into(),
                    content: Some(vec![serde_json::json!({
                        "type": "text",
                        "text": TOOL_INTERRUPTED_ON_RESUME_OUTPUT,
                    })]),
                    tool_calls: Some(Vec::new()),
                    tool_call_id: Some(tool_call_id),
                    is_error: Some(true),
                    origin: None,
                },
                time,
            });
        }
        self.pending_tool_result_ids.clear();
        self.flush_deferred_if_tool_exchange_closed();
    }

    fn flush_deferred_if_tool_exchange_closed(&mut self) {
        if self.pending_tool_result_ids.is_empty() && !self.deferred.is_empty() {
            let mut deferred = std::mem::take(&mut self.deferred);
            self.transcript.append(&mut deferred);
            self.folded_length += deferred.len() as i64;
        }
    }

    /// `settleStep` (contextTranscript.ts 94–104): a step with no tool calls
    /// and only vacuous content is removed from the transcript.
    fn settle_step(&mut self, uuid: Option<String>) {
        let Some(uuid) = uuid else { return };
        let Some(index) = self.open_steps.remove(&uuid) else {
            return;
        };
        let Some(entry) = self.transcript.get(index) else {
            return;
        };
        let has_tool_calls = entry
            .message
            .tool_calls
            .as_ref()
            .is_some_and(|c| !c.is_empty());
        if has_tool_calls {
            return;
        }
        let all_vacuous = entry
            .message
            .content
            .as_ref()
            .is_some_and(|parts| parts.iter().all(is_vacuous_content_part));
        if !all_vacuous {
            return;
        }
        self.transcript.remove(index);
        self.folded_length = (self.folded_length - 1).max(0);
    }

    /// `applyUndo` (contextTranscript.ts 163–185). Note the quirks: trailing
    /// injections after the anchor SURVIVE (they are `continue`d past), and a
    /// missing `count` deletes everything down to the clear floor.
    fn apply_undo(&mut self, count: Option<i64>) {
        if count.is_some_and(|c| c <= 0) {
            return;
        }
        let mut removed_user_count = 0i64;
        let mut i = self.transcript.len();
        while i > self.clear_floor {
            i -= 1;
            // Clone the message out so the transcript can be mutated while
            // the anchor is inspected (the TS code splices in place; value
            // semantics here are equivalent).
            let message = self.transcript[i].message.clone();
            let origin_kind = message.origin.as_ref().map(|o| o.kind.as_str());
            if origin_kind == Some("injection") {
                continue;
            }
            if origin_kind == Some("compaction_summary") {
                break;
            }
            self.transcript.remove(i);
            self.folded_length = (self.folded_length - 1).max(0);
            if is_undo_anchor(&message) {
                removed_user_count += 1;
                if count.is_some_and(|c| removed_user_count >= c) {
                    while i > self.clear_floor {
                        let prev = self.transcript[i - 1].message.clone();
                        if !is_prompt_owned_injection(&prev, &message) {
                            break;
                        }
                        self.transcript.remove(i - 1);
                        i -= 1;
                        self.folded_length = (self.folded_length - 1).max(0);
                    }
                    break;
                }
            }
        }
        self.reset_open_state();
    }

    /// `context.apply_compaction` (contextTranscript.ts 198–214): zod-validated
    /// summary record → one `compaction_summary` user message; ALL history is
    /// kept.
    fn apply_compaction(&mut self, record: &WireRecord) -> Result<(), ReduceError> {
        let rest = &record.rest;
        let summary = rest
            .get("summary")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ReduceError {
                message: "context.apply_compaction: summary required".into(),
            })?
            .to_owned();
        // String fields (contextApplyCompactionSchema).
        if !rest.get("contextSummary").is_some_and(|v| v.is_string()) {
            return Err(ReduceError {
                message: "context.apply_compaction: contextSummary required".into(),
            });
        }
        // Required numeric fields.
        for field in [
            "compactedCount",
            "tokensBefore",
            "tokensAfter",
            "keptUserMessageCount",
        ] {
            if !rest.get(field).is_some_and(|v| v.is_number()) {
                return Err(ReduceError {
                    message: format!("context.apply_compaction: {field} required"),
                });
            }
        }
        let kept_user_message_count = rest
            .get("keptUserMessageCount")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let kept_head_present = rest.contains_key("keptHeadUserMessageCount");

        self.push(MutableEntry {
            message: HistoryMessage {
                id: None,
                role: "user".into(),
                content: Some(vec![serde_json::json!({ "type": "text", "text": summary })]),
                tool_calls: Some(Vec::new()),
                tool_call_id: None,
                is_error: None,
                origin: Some(MessageOrigin {
                    kind: "compaction_summary".into(),
                    rest: Default::default(),
                }),
            },
            time: record.time.as_ref().and_then(|t| t.as_ms()),
        });
        self.folded_length = kept_user_message_count + if kept_head_present { 2 } else { 1 };
        self.reset_open_state();
        Ok(())
    }

    fn reset_open_state(&mut self) {
        self.open_steps.clear();
        self.pending_tool_result_ids.clear();
        self.deferred.clear();
        self.last_open_step_uuid = None;
    }
}

/// `rawToolResultContent` (contextTranscript.ts 266–269).
fn raw_tool_result_content(output: Option<&serde_json::Value>) -> Vec<serde_json::Value> {
    match output {
        Some(serde_json::Value::String(text)) => {
            vec![serde_json::json!({ "type": "text", "text": text })]
        }
        Some(serde_json::Value::Array(parts)) => parts.clone(),
        _ => Vec::new(),
    }
}

/// `isVacuousContentPart` (vacuousContent.ts 14–17).
fn is_vacuous_content_part(part: &serde_json::Value) -> bool {
    match part.get("type").and_then(|v| v.as_str()) {
        Some("text") => part
            .get("text")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t.trim().is_empty()),
        Some("think") => {
            part.get("encrypted").is_none()
                && part
                    .get("think")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t.trim().is_empty())
        }
        _ => false,
    }
}

/// `isUndoAnchor` (conversationTime.ts 13–21).
fn is_undo_anchor(message: &HistoryMessage) -> bool {
    if message.role != "user" {
        return false;
    }
    match message.origin.as_ref() {
        None => true,
        Some(origin) if origin.kind == "user" => true,
        Some(origin)
            if (origin.kind == "skill_activation" || origin.kind == "plugin_command")
                && origin.rest.get("trigger").and_then(|v| v.as_str()) == Some("user-slash") =>
        {
            true
        }
        _ => false,
    }
}

/// `isPromptOwnedInjection` (conversationTime.ts 23–33).
fn is_prompt_owned_injection(message: &HistoryMessage, anchor: &HistoryMessage) -> bool {
    let Some(origin) = &message.origin else {
        return false;
    };
    if origin.kind != "injection" {
        return false;
    }
    let Some(owner) = origin.rest.get("ownerPromptId").and_then(|v| v.as_str()) else {
        return false;
    };
    owner == anchor.id.as_deref().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_wire::record::RecordTime;

    fn record(type_: &str, extra: serde_json::Value) -> WireRecord {
        let mut rest = serde_json::Map::new();
        if let serde_json::Value::Object(map) = extra {
            for (k, v) in map {
                rest.insert(k, v);
            }
        }
        WireRecord {
            r#type: type_.into(),
            time: Some(RecordTime::Ms(1)),
            rest,
        }
    }

    #[test]
    fn append_message_passthrough() {
        let records = vec![record(
            "context.append_message",
            serde_json::json!({ "message": {
                "id": "m1", "role": "user", "content": [{"type": "text", "text": "hi"}],
                "toolCalls": [], "origin": {"kind": "user"}
            }}),
        )];
        let out = reduce_context_transcript(&records).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].role, "user");
        assert_eq!(out.folded_length, 1);
    }

    #[test]
    fn loop_events_fold_step_and_tool() {
        let records = vec![
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "step.begin", "uuid": "s1"}
                }),
            ),
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "tool.call", "stepUuid": "s1", "toolCallId": "c1", "name": "bash", "args": {"cmd": "ls"}}
                }),
            ),
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "tool.result", "toolCallId": "c1", "result": {"output": "out", "isError": false}}
                }),
            ),
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "step.end", "uuid": "s1"}
                }),
            ),
        ];
        let out = reduce_context_transcript(&records).unwrap();
        // step.begin (assistant) + tool.result (tool); step.end settles the
        // step but produces no entry (the assistant kept its tool call).
        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.entries[0].role, "assistant");
        assert_eq!(out.entries[0].tool_calls.as_ref().unwrap()[0].id, "c1");
        assert_eq!(
            out.entries[0].tool_calls.as_ref().unwrap()[0]
                .arguments
                .as_deref(),
            Some(r#"{"cmd":"ls"}"#)
        );
        assert_eq!(out.entries[1].role, "tool");
        assert_eq!(out.entries[1].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(out.entries[1].is_error, Some(false));
        assert_eq!(out.folded_length, 2);
    }

    #[test]
    fn interrupted_tool_on_next_step_begin() {
        let records = vec![
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "step.begin", "uuid": "s1"}
                }),
            ),
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "tool.call", "stepUuid": "s1", "toolCallId": "c1", "name": "bash"}
                }),
            ),
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "step.begin", "uuid": "s2"}
                }),
            ),
        ];
        let out = reduce_context_transcript(&records).unwrap();
        // assistant(s1) + interrupted tool + assistant(s2).
        assert_eq!(out.entries.len(), 3);
        assert_eq!(out.entries[1].role, "tool");
        assert_eq!(out.entries[1].is_error, Some(true));
        assert_eq!(out.entries[1].tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn empty_step_is_reclaimed() {
        let records = vec![
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "step.begin", "uuid": "s1"}
                }),
            ),
            record(
                "context.append_loop_event",
                serde_json::json!({
                    "event": {"type": "step.end", "uuid": "s1"}
                }),
            ),
        ];
        let out = reduce_context_transcript(&records).unwrap();
        assert_eq!(out.entries.len(), 0);
        assert_eq!(out.folded_length, 0);
    }

    #[test]
    fn compaction_keeps_history_and_summary() {
        let records = vec![
            record(
                "context.append_message",
                serde_json::json!({
                    "message": {"id": "m1", "role": "user", "content": [{"type": "text", "text": "hi"}], "toolCalls": []}
                }),
            ),
            record(
                "context.apply_compaction",
                serde_json::json!({
                    "summary": "old stuff", "contextSummary": "x", "compactedCount": 2,
                    "tokensBefore": 10, "tokensAfter": 3, "keptUserMessageCount": 1
                }),
            ),
        ];
        let out = reduce_context_transcript(&records).unwrap();
        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.entries[1].role, "user");
        assert_eq!(
            out.entries[1].origin.as_ref().unwrap().kind,
            "compaction_summary"
        );
        assert_eq!(out.folded_length, 2);
    }

    #[test]
    fn undo_removes_until_anchor_count() {
        let records = vec![
            record(
                "context.append_message",
                serde_json::json!({
                    "message": {"id": "m1", "role": "user", "content": [{"type": "text", "text": "a"}], "toolCalls": []}
                }),
            ),
            record(
                "context.append_message",
                serde_json::json!({
                    "message": {"id": "m2", "role": "user", "content": [{"type": "text", "text": "b"}], "toolCalls": []}
                }),
            ),
            record(
                "context.append_message",
                serde_json::json!({
                    "message": {"id": "m3", "role": "user", "content": [{"type": "text", "text": "c"}], "toolCalls": []}
                }),
            ),
            record("context.undo", serde_json::json!({ "count": 2 })),
        ];
        let out = reduce_context_transcript(&records).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].id.as_deref(), Some("m1"));
        assert_eq!(out.folded_length, 1);
    }
}
