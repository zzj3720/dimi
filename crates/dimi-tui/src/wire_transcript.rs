//! Transcript data surface — cold rebuild from `wire.jsonl`.
//!
//! Pipeline mirrors the TS replay path (`session-replay.ts` +
//! `dimi-store`): `read_wire_records` → `reduce_context_transcript` →
//! `group_messages_into_snapshot`, then map the folded context messages into
//! the same transcript entries the live TUI renders (user / assistant /
//! thinking / tool-call), applying the same origin filters so model-facing
//! injections and system triggers never leak into the transcript.

use std::path::Path;

use dimi_store::reduce::{ContextTranscript, reduce_context_transcript};
use dimi_store::wire::read_wire_records;
use dimi_wire::record::HistoryMessage;

use crate::component::Component;
use crate::components::messages::tool_renderers::{ToolCallData, ToolResultData};
use crate::theme::ColorToken;

/// Transcript entry kinds rendered by slice 2 components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEntryKind {
    User,
    Assistant,
    Thinking,
    ToolCall,
    Status,
}

/// One renderable transcript entry.
#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub kind: TranscriptEntryKind,
    /// Text content (user prompt / assistant markdown / thinking / status).
    pub content: String,
    /// Optional user-message bullet override (empty string suppresses it).
    pub bullet: Option<String>,
    /// Tool call payload (kind == ToolCall).
    pub tool_call: Option<ToolCallData>,
    /// Tool result payload (attached when the matching `tool` message lands).
    pub tool_result: Option<ToolResultData>,
    /// Optional color for `Status` entries. `render_transcript` honors it for
    /// the status tone (e.g. error-colored errors); `None` renders the default
    /// dim status tone. Slice-6 addition so hosts can surface `ShowError`
    /// statuses in the error color.
    pub status_color: Option<ColorToken>,
}

impl TranscriptEntry {
    fn user(content: &str, bullet: Option<String>) -> Self {
        TranscriptEntry {
            kind: TranscriptEntryKind::User,
            content: content.to_owned(),
            bullet,
            tool_call: None,
            tool_result: None,
            status_color: None,
        }
    }
    fn assistant(content: &str) -> Self {
        TranscriptEntry {
            kind: TranscriptEntryKind::Assistant,
            content: content.to_owned(),
            bullet: None,
            tool_call: None,
            tool_result: None,
            status_color: None,
        }
    }
    fn thinking(content: &str) -> Self {
        TranscriptEntry {
            kind: TranscriptEntryKind::Thinking,
            content: content.to_owned(),
            bullet: None,
            tool_call: None,
            tool_result: None,
            status_color: None,
        }
    }
    fn tool(call: ToolCallData) -> Self {
        TranscriptEntry {
            kind: TranscriptEntryKind::ToolCall,
            content: String::new(),
            bullet: None,
            tool_call: Some(call),
            tool_result: None,
            status_color: None,
        }
    }
    fn status(content: &str) -> Self {
        TranscriptEntry {
            kind: TranscriptEntryKind::Status,
            content: content.to_owned(),
            bullet: None,
            tool_call: None,
            tool_result: None,
            status_color: None,
        }
    }
}

/// Errors from the wire transcript pipeline.
#[derive(Debug)]
pub enum WireTranscriptError {
    Read(dimi_store::wire::WireReadError),
    Reduce(String),
}

impl std::fmt::Display for WireTranscriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireTranscriptError::Read(e) => write!(f, "{e}"),
            WireTranscriptError::Reduce(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for WireTranscriptError {}

impl From<dimi_store::wire::WireReadError> for WireTranscriptError {
    fn from(e: dimi_store::wire::WireReadError) -> Self {
        WireTranscriptError::Read(e)
    }
}

/// Origins whose content is model-facing, not display (`session-replay.ts`
/// renderUserMessage filters).
const HIDDEN_USER_ORIGINS: &[&str] = &[
    "hook_result",
    "injection",
    "task",
    "cron_job",
    "cron_missed",
    "system_trigger",
    "skill_activation",
    "plugin_command",
];

/// Cold-rebuild the transcript entries for a `wire.jsonl` file.
pub fn transcript_from_wire(path: &Path) -> Result<Vec<TranscriptEntry>, WireTranscriptError> {
    let records = read_wire_records(path)?;
    let ctx =
        reduce_context_transcript(&records).map_err(|e| WireTranscriptError::Reduce(e.message))?;
    Ok(transcript_from_messages(&ctx))
}

/// Map folded context messages to transcript entries (the slice 2 replay
/// mapping, faithful for the common user/assistant/tool flow).
pub fn transcript_from_messages(ctx: &ContextTranscript) -> Vec<TranscriptEntry> {
    let mut entries: Vec<TranscriptEntry> = Vec::new();
    // Pending tool calls awaiting their `tool` result message.
    let mut pending_tools: Vec<usize> = Vec::new();

    for message in &ctx.entries {
        match message.role.as_str() {
            "system" => continue,
            "user" => {
                if let Some(kind) = message.origin.as_ref().map(|o| o.kind.as_str()) {
                    if kind == "compaction_summary" {
                        entries.push(TranscriptEntry::status("Compaction complete"));
                        let summary = content_parts_to_text(message);
                        if !summary.is_empty() {
                            entries.push(TranscriptEntry::status(&summary));
                        }
                        continue;
                    }
                    if HIDDEN_USER_ORIGINS.contains(&kind) {
                        continue;
                    }
                    if kind == "shell_command" {
                        // `!` command echo: `$ cmd` with the bullet suppressed.
                        let text = content_parts_to_text(message);
                        let phase = message
                            .origin
                            .as_ref()
                            .and_then(|o| o.rest.get("phase"))
                            .and_then(|v| v.as_str());
                        if phase == Some("input") {
                            entries.push(TranscriptEntry::user(&text, Some(String::new())));
                        } else {
                            entries.push(TranscriptEntry::status(&text));
                        }
                        continue;
                    }
                }
                entries.push(TranscriptEntry::user(&content_parts_to_text(message), None));
            }
            "assistant" => {
                // Collect thinking + text parts, then flush.
                let mut thinking = String::new();
                let mut text = String::new();
                for part in message.content.iter().flatten() {
                    match part.get("type").and_then(|v| v.as_str()) {
                        Some("think") => {
                            if let Some(t) = part.get("think").and_then(|v| v.as_str()) {
                                thinking.push_str(t);
                            }
                        }
                        Some("text") => {
                            if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                text.push_str(t);
                            }
                        }
                        _ => {}
                    }
                }
                if !thinking.trim().is_empty() {
                    entries.push(TranscriptEntry::thinking(&thinking));
                }
                if !text.trim().is_empty() {
                    entries.push(TranscriptEntry::assistant(&text));
                }
                // Tool calls start pending tool cards.
                for call in message.tool_calls.iter().flatten() {
                    let args = parse_arguments(call.arguments.as_deref());
                    let idx = entries.len();
                    entries.push(TranscriptEntry::tool(ToolCallData {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        args,
                        truncated: false,
                    }));
                    pending_tools.push(idx);
                }
            }
            "tool" => {
                let Some(tool_call_id) = message.tool_call_id.as_deref() else {
                    continue;
                };
                // Find the pending tool card (search from the end — the last
                // matching pending card is the one this result belongs to).
                if let Some(idx) = pending_tools.iter().rev().find_map(|i| {
                    let entry = &entries[*i];
                    if entry
                        .tool_call
                        .as_ref()
                        .is_some_and(|c| c.id == tool_call_id)
                    {
                        Some(*i)
                    } else {
                        None
                    }
                }) {
                    let output = tool_result_output(message);
                    let is_error = message.is_error.unwrap_or(false);
                    entries[idx].tool_result = Some(ToolResultData {
                        tool_call_id: tool_call_id.to_owned(),
                        output,
                        is_error,
                    });
                    // Consume the pending slot so a later duplicate result
                    // does not double-attach.
                    if let Some(pos) = pending_tools.iter().position(|i| *i == idx) {
                        pending_tools.remove(pos);
                    }
                }
            }
            _ => {}
        }
    }
    entries
}

/// `contentPartsToText` — concatenate all part text (think included, matching
/// the TS helper which maps think → part.think).
fn content_parts_to_text(message: &HistoryMessage) -> String {
    let mut out = String::new();
    for part in message.content.iter().flatten() {
        match part.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                }
            }
            Some("think") => {
                if let Some(t) = part.get("think").and_then(|v| v.as_str()) {
                    out.push_str(t);
                }
            }
            _ => {}
        }
    }
    out
}

/// `toolResultOutput` — all-text content joins; mixed content JSON-stringifies.
fn tool_result_output(message: &HistoryMessage) -> String {
    let parts = message.content.as_deref().unwrap_or_default();
    let all_text = parts
        .iter()
        .all(|p| p.get("type").and_then(|v| v.as_str()) == Some("text"));
    if all_text {
        content_parts_to_text(message)
    } else {
        serde_json::to_string(parts).unwrap_or_default()
    }
}

/// `parseReplayToolArguments` — the JSON-string args → object map.
fn parse_arguments(raw: Option<&str>) -> serde_json::Map<String, serde_json::Value> {
    let Some(raw) = raw else {
        return serde_json::Map::new();
    };
    if raw.is_empty() {
        return serde_json::Map::new();
    }
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// Render a transcript to component lines (used by the frame harness).
pub fn render_transcript(entries: &[TranscriptEntry], width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in entries {
        match entry.kind {
            TranscriptEntryKind::User => {
                let mut c = crate::components::messages::user_message::UserMessageComponent::new(
                    &entry.content,
                    entry.bullet.clone(),
                );
                lines.extend(c.render(width));
            }
            TranscriptEntryKind::Assistant => {
                let mut c =
                    crate::components::messages::assistant_message::AssistantMessageComponent::new(
                        true,
                    );
                c.update_content(&entry.content);
                lines.extend(c.render(width));
            }
            TranscriptEntryKind::Thinking => {
                let mut c = crate::components::messages::thinking::ThinkingComponent::new(
                    &entry.content,
                    true,
                    crate::components::messages::thinking::ThinkingMode::Finalized,
                );
                lines.extend(c.render(width));
            }
            TranscriptEntryKind::ToolCall => {
                if let Some(call) = &entry.tool_call {
                    let mut c = crate::components::messages::tool_call::ToolCallComponent::new(
                        call.clone(),
                        entry.tool_result.clone(),
                    );
                    lines.extend(c.render(width));
                }
            }
            TranscriptEntryKind::Status => {
                let mut c =
                    crate::components::messages::status_message::StatusMessageComponent::new(
                        &entry.content,
                        entry.status_color,
                    );
                lines.extend(c.render(width));
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    fn fixture_path() -> String {
        format!("{}/testdata/sample-wire.jsonl", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn wire_transcript_maps_messages() {
        let binding = fixture_path();
        let path = Path::new(&binding);
        let entries = transcript_from_wire(path).expect("wire transcript");
        // user, thinking, assistant, tool(Bash), user(injection skipped),
        // assistant, tool(WebSearch)
        assert_eq!(entries.len(), 6, "entries: {entries:#?}");

        assert_eq!(entries[0].kind, TranscriptEntryKind::User);
        assert_eq!(entries[0].content, "Hello there!");

        assert_eq!(entries[1].kind, TranscriptEntryKind::Thinking);
        assert_eq!(entries[1].content, "Let me think about this carefully.");

        assert_eq!(entries[2].kind, TranscriptEntryKind::Assistant);
        assert_eq!(entries[2].content, "Hello **world**!");

        assert_eq!(entries[3].kind, TranscriptEntryKind::ToolCall);
        let call = entries[3].tool_call.as_ref().expect("tool call");
        assert_eq!(call.name, "Bash");
        assert_eq!(
            call.args.get("command").and_then(|v| v.as_str()),
            Some("ls -la")
        );
        let result = entries[3].tool_result.as_ref().expect("tool result");
        assert!(result.output.starts_with("total 8"));
        assert!(!result.is_error);

        assert_eq!(entries[4].kind, TranscriptEntryKind::Assistant);
        assert_eq!(entries[4].content, "Another assistant reply");

        assert_eq!(entries[5].kind, TranscriptEntryKind::ToolCall);
        assert_eq!(entries[5].tool_call.as_ref().unwrap().name, "WebSearch");
        assert!(
            entries[5]
                .tool_result
                .as_ref()
                .unwrap()
                .output
                .contains("Search results")
        );
    }

    #[test]
    fn wire_transcript_renders_to_lines() {
        set_palette(DARK_COLORS);
        let binding = fixture_path();
        let path = Path::new(&binding);
        let entries = transcript_from_wire(path).expect("wire transcript");
        let lines = render_transcript(&entries, 80);
        // user (2) + thinking (2) + assistant (2) + tool bash (6) + assistant (2) + tool websearch (2)
        assert!(
            lines.len() >= 10,
            "expected many lines, got {}",
            lines.len()
        );
        let joined = lines.join("\n");
        assert!(joined.contains("Hello there!"));
        assert!(joined.contains("Hello "));
        assert!(joined.contains("world"));
        assert!(joined.contains("Let me think about this carefully."));
        assert!(joined.contains("ls -la"));
        assert!(joined.contains("total 8"));
        assert!(joined.contains("rust terminal ui"));
        // The injection user message must not leak.
        assert!(!joined.contains("Second prompt"));
    }
}
