//! `groupMessagesIntoSnapshot` — flat context messages → turn tree
//! (`packages/transcript/src/history/groupTurns.ts`).
//!
//! Best-effort cold reconstruction with the same accepted limitations:
//! one assistant message = one step, no live-only backfill, turn ordinals
//! 0-based, tasks/interactions/todos/prompts/meta left empty for the fold.

use dimi_wire::entity::{Attachment, AttachmentSource, TranscriptMeta};
use dimi_wire::frame::{Frame, TextRole};
use dimi_wire::item::{Item, Step, StepKind};
use dimi_wire::model::{StepState, TurnOrigin, TurnState};
use dimi_wire::record::HistoryMessage;
use dimi_wire::snapshot::AgentTranscriptSnapshot;

/// Origins whose content is context, not display (groupTurns.ts 85).
const HIDDEN_USER_ORIGINS: &[&str] = &["injection", "system_trigger", "retry"];
/// Hidden origins that open a real engine turn (groupTurns.ts 96).
const TURN_OPENING_SYSTEM_TRIGGERS: &[&str] = &["goal_continuation", "subagent"];
/// Origins rendered as timeline markers (groupTurns.ts 98–102).
const MARKER_USER_ORIGINS: &[(&str, &str)] = &[
    ("skill_activation", "skill"),
    ("plugin_command", "skill"),
    ("compaction_summary", "compaction"),
];

#[derive(Debug, Clone)]
struct TurnDraft {
    turn_id: String,
    ordinal: i64,
    origin: TurnOrigin,
    prompt: Option<String>,
    attachment_ids: Option<Vec<String>>,
    steps: Vec<StepDraft>,
}

#[derive(Debug, Clone)]
struct StepDraft {
    step_id: String,
    ordinal: i64,
    frames: Vec<Frame>,
}

/// `groupMessagesIntoSnapshot` (groupTurns.ts 106–265).
pub fn group_messages_into_snapshot(messages: &[HistoryMessage]) -> AgentTranscriptSnapshot {
    let mut items: Vec<Item> = Vec::new();
    let mut attachments: Vec<Attachment> = Vec::new();
    let mut turn: Option<TurnDraft> = None;
    let mut next_ordinal: i64 = 0;
    let mut marker_count: i64 = 0;

    for message in messages {
        if message.role == "system" {
            continue;
        }
        let origin_kind = message.origin.as_ref().map(|o| o.kind.as_str());

        if message.role == "user" {
            if let Some(kind) = origin_kind {
                if HIDDEN_USER_ORIGINS.contains(&kind) {
                    if opens_own_turn(message) {
                        // Real turn boundary: promptless, mirroring the live
                        // path's displayable-origin gate.
                        start_turn(
                            &mut turn,
                            &mut items,
                            &mut next_ordinal,
                            map_origin(message),
                            None,
                            None,
                        );
                    }
                    continue;
                }
            }
            let marker_key = origin_kind.and_then(|kind| {
                MARKER_USER_ORIGINS
                    .iter()
                    .find(|(k, _)| *k == kind)
                    .map(|(_, m)| *m)
            });
            if let Some(marker_key) = marker_key {
                marker_count += 1;
                items.push(Item::Marker {
                    marker_id: format!("m{marker_count}"),
                    marker: marker_key.to_owned(),
                    payload: Some(serde_json::json!({
                        "text": text_of(message),
                        "origin": message.origin,
                    })),
                    at: None,
                });
                if is_user_slash_prompt(message) {
                    start_turn(
                        &mut turn,
                        &mut items,
                        &mut next_ordinal,
                        map_origin(message),
                        Some(text_of(message)),
                        None,
                    );
                }
                continue;
            }
            let (origin, attachment_ids) = {
                let ids = collect_attachments(message, &mut attachments);
                (map_origin(message), ids)
            };
            start_turn(
                &mut turn,
                &mut items,
                &mut next_ordinal,
                origin,
                Some(text_of(message)),
                attachment_ids,
            );
            continue;
        }

        if message.role == "assistant" {
            let current = ensure_turn(&mut turn, &mut items, &mut next_ordinal);
            let step_ordinal = current.steps.len() as i64 + 1;
            let step_id = format!("{}.{}", current.turn_id, step_ordinal);
            let mut step = StepDraft {
                step_id: step_id.clone(),
                ordinal: step_ordinal,
                frames: Vec::new(),
            };
            let mut frame_count = 0i64;
            let mut next_frame_id = || {
                frame_count += 1;
                dimi_wire::id::FrameId::new_unchecked(format!("{step_id}.f{frame_count}"))
            };
            for part in message.content.iter().flatten() {
                let part_type = part.get("type").and_then(|v| v.as_str());
                match part_type {
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                step.frames.push(Frame::Text {
                                    frame_id: next_frame_id(),
                                    role: TextRole::Assistant,
                                    text: text.to_owned(),
                                    attachment_ids: None,
                                    task_id: None,
                                });
                            }
                        }
                    }
                    Some("think") => {
                        if let Some(text) = part.get("think").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                step.frames.push(Frame::Thinking {
                                    frame_id: next_frame_id(),
                                    text: text.to_owned(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            for call in message.tool_calls.iter().flatten() {
                step.frames.push(Frame::Tool {
                    frame_id: dimi_wire::id::FrameId::new_unchecked(format!(
                        "{step_id}.{}",
                        call.id
                    )),
                    tool_call_id: call.id.clone(),
                    name: call.name.clone(),
                    view: None,
                    state: dimi_wire::frame::ToolState::Running,
                    input: parse_arguments(call.arguments.as_deref()),
                    output: None,
                    display: None,
                    error: None,
                    input_text: None,
                    progress: None,
                    task_id: None,
                    approval_id: None,
                    todo_id: None,
                    agent_refs: None,
                });
            }
            current.steps.push(step);
            sync_turn_item(&mut items, current);
            continue;
        }

        if message.role == "tool" {
            let Some(draft) = turn.as_mut() else { continue };
            let Some(tool_call_id) = message.tool_call_id.as_deref() else {
                continue;
            };
            let Some(frame_index) = current_turn_tool_frame_index(draft, tool_call_id) else {
                continue;
            };
            let output = text_of(message);
            let is_error = message.is_error.unwrap_or(false);
            // Patch the frame in place.
            let frame = &mut draft.steps[frame_index.0].frames[frame_index.1];
            if let Frame::Tool {
                state,
                output: out,
                error,
                ..
            } = frame
            {
                *state = if is_error {
                    dimi_wire::frame::ToolState::Error
                } else {
                    dimi_wire::frame::ToolState::Done
                };
                *out = Some(serde_json::Value::String(output.clone()));
                *error = if is_error { Some(output) } else { None };
            }
            sync_turn_item(&mut items, draft);
        }
    }

    AgentTranscriptSnapshot {
        items,
        tasks: Vec::new(),
        interactions: Vec::new(),
        attachments,
        todos: Vec::new(),
        prompts: Vec::new(),
        meta: TranscriptMeta::default(),
        has_more_older: None,
    }
}

// ---------------------------------------------------------------- helpers

fn start_turn(
    turn: &mut Option<TurnDraft>,
    items: &mut Vec<Item>,
    next_ordinal: &mut i64,
    origin: TurnOrigin,
    prompt: Option<String>,
    attachment_ids: Option<Vec<String>>,
) {
    let ordinal = *next_ordinal;
    *next_ordinal += 1;
    let draft = TurnDraft {
        turn_id: format!("t{ordinal}"),
        ordinal,
        origin,
        prompt,
        attachment_ids,
        steps: Vec::new(),
    };
    items.push(draft_to_turn_item(&draft));
    *turn = Some(draft);
}

fn ensure_turn<'a>(
    turn: &'a mut Option<TurnDraft>,
    items: &mut Vec<Item>,
    next_ordinal: &mut i64,
) -> &'a mut TurnDraft {
    if turn.is_none() {
        start_turn(
            turn,
            items,
            next_ordinal,
            TurnOrigin::Other { payload: None },
            None,
            None,
        );
    }
    turn.as_mut().expect("turn just ensured")
}

/// `mapOrigin` (groupTurns.ts 292–318).
fn map_origin(message: &HistoryMessage) -> TurnOrigin {
    let Some(origin) = &message.origin else {
        return TurnOrigin::User { payload: None };
    };
    let payload = || serde_json::to_value(origin).ok();
    match origin.kind.as_str() {
        "cron_job" | "cron_missed" => {
            let job_id = origin.rest.get("jobId").and_then(|v| v.as_str());
            TurnOrigin::Cron {
                task_id: job_id.map(|s| dimi_wire::id::TaskId::new_unchecked(s.to_owned())),
                payload: payload(),
            }
        }
        "task" => {
            let task_id = origin.rest.get("taskId").and_then(|v| v.as_str());
            match task_id {
                Some(task_id) => TurnOrigin::Task {
                    task_id: dimi_wire::id::TaskId::new_unchecked(task_id.to_owned()),
                    payload: payload(),
                },
                None => TurnOrigin::Other { payload: payload() },
            }
        }
        "hook_result" => TurnOrigin::Hook { payload: payload() },
        "shell_command" => TurnOrigin::User { payload: payload() },
        "user" => TurnOrigin::User { payload: None },
        _ => TurnOrigin::Other { payload: payload() },
    }
}

/// `textOf` (groupTurns.ts 320–325).
fn text_of(message: &HistoryMessage) -> String {
    message
        .content
        .iter()
        .flatten()
        .filter(|part| part.get("type").and_then(|v| v.as_str()) == Some("text"))
        .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("")
}

/// `parseArguments` (groupTurns.ts 327–334): null/empty → undefined; JSON
/// object → parsed; unparsable → the raw string.
fn parse_arguments(raw: Option<&str>) -> Option<serde_json::Value> {
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => Some(value),
        Err(_) => Some(serde_json::Value::String(raw.to_owned())),
    }
}

/// `collectAttachments` (groupTurns.ts 117–150).
fn collect_attachments(
    message: &HistoryMessage,
    attachments: &mut Vec<Attachment>,
) -> Option<Vec<String>> {
    let mut ids = Vec::new();
    for part in message.content.iter().flatten() {
        let part_type = part.get("type").and_then(|v| v.as_str());
        let source = part.get("source");
        match part_type {
            Some("image") | Some("video") | Some("audio") => {
                let Some(source) = source else { continue };
                if source.is_null() {
                    continue;
                }
                let source_kind = source.get("kind").and_then(|v| v.as_str());
                let media_type = if source_kind == Some("base64") {
                    source
                        .get("media_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned()
                } else {
                    format!("{}/*", part_type.unwrap_or_default())
                };
                let attachment_source = match source_kind {
                    Some("url") => source.get("url").and_then(|v| v.as_str()).map(|url| {
                        AttachmentSource::Url {
                            url: url.to_owned(),
                        }
                    }),
                    Some("file") => source
                        .get("file_id")
                        .and_then(|v| v.as_str())
                        .map(|file_id| AttachmentSource::File {
                            file_id: file_id.to_owned(),
                        }),
                    _ => None, // base64 bytes deliberately dropped
                };
                let entity = Attachment {
                    attachment_id: format!("att_{}", attachments.len() + 1),
                    media_type,
                    name: None,
                    size: None,
                    source: attachment_source,
                    placeholder: None,
                };
                let id = entity.attachment_id.clone();
                attachments.push(entity);
                ids.push(id);
            }
            Some("file") => {
                let Some(file_id) = part.get("file_id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let entity = Attachment {
                    attachment_id: format!("att_{}", attachments.len() + 1),
                    media_type: part
                        .get("media_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    name: part.get("name").and_then(|v| v.as_str()).map(str::to_owned),
                    size: part.get("size").and_then(|v| v.as_i64()),
                    source: Some(AttachmentSource::File {
                        file_id: file_id.to_owned(),
                    }),
                    placeholder: None,
                };
                let id = entity.attachment_id.clone();
                attachments.push(entity);
                ids.push(id);
            }
            _ => {}
        }
    }
    if ids.is_empty() { None } else { Some(ids) }
}

/// `opensOwnTurn` (groupTurns.ts 270–277).
fn opens_own_turn(message: &HistoryMessage) -> bool {
    let Some(origin) = &message.origin else {
        return false;
    };
    origin.kind == "system_trigger"
        && origin
            .rest
            .get("name")
            .and_then(|v| v.as_str())
            .is_some_and(|name| TURN_OPENING_SYSTEM_TRIGGERS.contains(&name))
}

/// `isUserSlashPrompt` (groupTurns.ts 284–290).
fn is_user_slash_prompt(message: &HistoryMessage) -> bool {
    let Some(origin) = &message.origin else {
        return false;
    };
    (origin.kind == "skill_activation" || origin.kind == "plugin_command")
        && origin.rest.get("trigger").and_then(|v| v.as_str()) == Some("user-slash")
}

fn draft_to_turn_item(draft: &TurnDraft) -> Item {
    Item::Turn {
        turn_id: dimi_wire::id::TurnId::new_unchecked(draft.turn_id.clone()),
        ordinal: draft.ordinal,
        state: TurnState::Completed,
        origin: draft.origin.clone(),
        prompt: draft.prompt.clone(),
        attachment_ids: draft.attachment_ids.clone(),
        steps: draft
            .steps
            .iter()
            .map(|step| Step {
                kind: StepKind::Step,
                step_id: dimi_wire::id::StepId::new_unchecked(step.step_id.clone()),
                turn_id: dimi_wire::id::TurnId::new_unchecked(draft.turn_id.clone()),
                ordinal: step.ordinal,
                state: StepState::Completed,
                frames: step.frames.clone(),
                started_at: None,
                ended_at: None,
                usage: None,
                finish_reason: None,
                timing: None,
                retry: None,
                end_reason: None,
                end_message: None,
            })
            .collect(),
        started_at: None,
        ended_at: None,
        usage: None,
        duration_ms: None,
        error: None,
    }
}

fn sync_turn_item(items: &mut [Item], draft: &TurnDraft) {
    if let Some(entry) = items.iter_mut().find(
        |entry| matches!(entry, Item::Turn { turn_id, .. } if turn_id.as_str() == draft.turn_id),
    ) {
        *entry = draft_to_turn_item(draft);
    }
}

/// `currentTurnToolFrame` + `replaceToolFrame` (groupTurns.ts 362–384):
/// scan from the LAST step's last frame backwards.
fn current_turn_tool_frame_index(draft: &TurnDraft, tool_call_id: &str) -> Option<(usize, usize)> {
    for s in (0..draft.steps.len()).rev() {
        let step = &draft.steps[s];
        for f in (0..step.frames.len()).rev() {
            if let Frame::Tool {
                tool_call_id: id, ..
            } = &step.frames[f]
            {
                if id == tool_call_id {
                    return Some((s, f));
                }
            }
        }
    }
    None
}
