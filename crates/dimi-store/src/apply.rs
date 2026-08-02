//! The 14-op reducer (`packages/transcript/src/ops/apply.ts`).
//!
//! Semantics to preserve exactly:
//! - Every op except `append` is state-style and idempotent: replaying,
//!   duplicating or reordering converges to the same store.
//! - `append` is causal: `offset > local length` (or a diverged overlap) is a
//!   gap and the chunk is dropped — never a silent rewrite of local content.
//! - Ops apply strictly in arrival order; new steps sort stably by ordinal,
//!   frames keep arrival order.
//! - Equality short-circuits use VALUE comparison here (TS uses reference
//!   equality for nested payloads); state convergence is identical, only the
//!   `changed`/event output can differ in the re-serialized-identical-op
//!   corner (documented in lib.rs).

use std::collections::HashMap;

use dimi_wire::entity::{Interaction, InteractionState, TranscriptMetaMerge};
use dimi_wire::frame::Frame;
use dimi_wire::id::{StepId, TurnId};
use dimi_wire::item::{Item, StepKind};
use dimi_wire::model::{StepState, TurnOrigin, TurnState};
use dimi_wire::op::{AppendTarget, Operation, StepHeader, TurnHeader};

use crate::state::{AgentState, empty_agent_state};

/// Result of applying one op.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationResult {
    pub state: AgentState,
    pub changed: bool,
    /// Only `append` produces gaps: `{ expected, got }` (target is attached
    /// by the batch-level apply, mirroring `AgentTranscript.apply`).
    pub gap: Option<GapInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapInfo {
    pub expected: i64,
    pub got: i64,
}

impl OperationResult {
    fn unchanged(state: AgentState) -> Self {
        OperationResult {
            state,
            changed: false,
            gap: None,
        }
    }
}

/// `turnOrdinal(id)` — `Number(id.slice(1))`, non-finite → 0 (ids.ts 43–46).
/// Rust parses `i64`; the JS `Number` coercion is looser (floats, hex) but
/// engine-minted ordinals are always plain integers.
pub fn turn_ordinal(id: &str) -> i64 {
    id.get(1..).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0)
}

/// `itemIdOf(item)` (apply.ts 441–450).
pub fn item_id_of(item: &Item) -> String {
    match item {
        Item::Turn { turn_id, .. } => turn_id.as_str().to_owned(),
        Item::Marker { marker_id, .. } => marker_id.clone(),
        Item::TaskRef { ref_id, .. } => ref_id.clone(),
    }
}

// ------------------------------------------------------------ reset

fn apply_reset(
    _state: AgentState,
    snapshot: &dimi_wire::snapshot::AgentTranscriptSnapshot,
) -> OperationResult {
    let mut pending = std::collections::HashSet::new();
    for interaction in &snapshot.interactions {
        if interaction.state == InteractionState::Pending {
            pending.insert(interaction.interaction_id.clone());
        }
    }
    OperationResult {
        state: AgentState {
            items: snapshot.items.clone(),
            tasks: snapshot
                .tasks
                .iter()
                .map(|t| (t.task_id.clone(), t.clone()))
                .collect(),
            interactions: snapshot
                .interactions
                .iter()
                .map(|i| (i.interaction_id.clone(), i.clone()))
                .collect(),
            attachments: snapshot
                .attachments
                .iter()
                .map(|a| (a.attachment_id.clone(), a.clone()))
                .collect(),
            todos: snapshot
                .todos
                .iter()
                .map(|t| (t.todo_id.clone(), t.clone()))
                .collect(),
            prompts: snapshot
                .prompts
                .iter()
                .map(|p| (p.prompt_id.clone(), p.clone()))
                .collect(),
            meta: snapshot.meta.clone(),
            pending_interactions: pending,
            has_more_older: snapshot.has_more_older.unwrap_or(false),
        },
        changed: true,
        gap: None,
    }
}

// ------------------------------------------------------------ turn / step / frame

/// `turnHeaderToTurn(header, steps)` (apply.ts 131–133).
fn turn_header_to_turn(header: &TurnHeader, steps: Vec<dimi_wire::item::Step>) -> Item {
    Item::Turn {
        turn_id: header.turn_id.clone(),
        ordinal: header.ordinal,
        state: header.state,
        origin: header.origin.clone(),
        prompt: header.prompt.clone(),
        attachment_ids: header.attachment_ids.clone(),
        steps,
        started_at: header.started_at.clone(),
        ended_at: header.ended_at.clone(),
        usage: header.usage.clone(),
        duration_ms: header.duration_ms,
        error: header.error.clone(),
    }
}

/// `skeletonTurn(turnId)` (apply.ts 135–144).
fn skeleton_turn(turn_id: &str) -> Item {
    Item::Turn {
        turn_id: TurnId::new_unchecked(turn_id.to_owned()),
        ordinal: turn_ordinal(turn_id),
        state: TurnState::Running,
        origin: TurnOrigin::Other { payload: None },
        prompt: None,
        attachment_ids: None,
        steps: Vec::new(),
        started_at: None,
        ended_at: None,
        usage: None,
        duration_ms: None,
        error: None,
    }
}

/// `skeletonStep(stepId, turnId)` (apply.ts 146–149).
fn skeleton_step(step_id: &str, turn_id: &str) -> dimi_wire::item::Step {
    let ordinal = step_id
        .get(turn_id.len() + 1..)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    dimi_wire::item::Step {
        kind: StepKind::Step,
        step_id: StepId::new_unchecked(step_id.to_owned()),
        turn_id: TurnId::new_unchecked(turn_id.to_owned()),
        ordinal,
        state: StepState::Running,
        frames: Vec::new(),
        started_at: None,
        ended_at: None,
        usage: None,
        finish_reason: None,
        timing: None,
        retry: None,
        end_reason: None,
        end_message: None,
    }
}

fn get_turn<'a>(state: &'a AgentState, turn_id: &str) -> Option<&'a Item> {
    state
        .items
        .iter()
        .find(|item| matches!(item, Item::Turn { turn_id: id, .. } if id.as_str() == turn_id))
}

/// `insertTurn` — first turn with `ordinal > turn.ordinal` wins; markers stay
/// put (apply.ts 157–169).
fn insert_turn(items: &mut Vec<Item>, turn: Item) {
    let Item::Turn { ordinal, .. } = &turn else {
        unreachable!()
    };
    let at = items
        .iter()
        .position(|entry| matches!(entry, Item::Turn { ordinal: o, .. } if o > ordinal))
        .unwrap_or(items.len());
    items.insert(at, turn);
}

fn replace_turn<F: FnOnce(&mut Item)>(items: &mut [Item], turn_id: &str, f: F) {
    if let Some(entry) = items
        .iter_mut()
        .find(|entry| matches!(entry, Item::Turn { turn_id: id, .. } if id.as_str() == turn_id))
    {
        f(entry);
    }
}

/// `turnEquals` (apply.ts 201–215) — value comparison in Rust.
fn turn_equals(turn: &Item, header: &TurnHeader) -> bool {
    let Item::Turn {
        ordinal,
        state,
        origin,
        prompt,
        attachment_ids,
        started_at,
        ended_at,
        usage,
        duration_ms,
        error,
        ..
    } = turn
    else {
        return false;
    };
    *ordinal == header.ordinal
        && *state == header.state
        && prompt == &header.prompt
        && attachment_ids == &header.attachment_ids
        && started_at == &header.started_at
        && ended_at == &header.ended_at
        && origin == &header.origin
        && usage == &header.usage
        && duration_ms == &header.duration_ms
        && error == &header.error
}

fn apply_turn_upsert(mut state: AgentState, header: &TurnHeader) -> OperationResult {
    let turn_id = header.turn_id.as_str();
    if let Some(existing) = get_turn(&state, turn_id).cloned() {
        if turn_equals(&existing, header) {
            return OperationResult::unchanged(state);
        }
        // Replace header, keep steps.
        let steps = match &existing {
            Item::Turn { steps, .. } => steps.clone(),
            _ => unreachable!(),
        };
        replace_turn(&mut state.items, turn_id, |entry| {
            *entry = turn_header_to_turn(header, steps);
        });
        OperationResult {
            state,
            changed: true,
            gap: None,
        }
    } else {
        insert_turn(&mut state.items, turn_header_to_turn(header, Vec::new()));
        OperationResult {
            state,
            changed: true,
            gap: None,
        }
    }
}

fn step_equals(step: &dimi_wire::item::Step, header: &StepHeader) -> bool {
    step.step_id == header.step_id
        && step.turn_id == header.turn_id
        && step.ordinal == header.ordinal
        && step.state == header.state
        && step.started_at == header.started_at
        && step.ended_at == header.ended_at
        && step.usage == header.usage
        && step.finish_reason == header.finish_reason
        && step.timing == header.timing
        && step.retry == header.retry
        && step.end_reason == header.end_reason
        && step.end_message == header.end_message
}

fn apply_step_upsert(mut state: AgentState, turn_id: &str, header: &StepHeader) -> OperationResult {
    if get_turn(&state, turn_id).is_none() {
        let turn = skeleton_turn(header.turn_id.as_str());
        insert_turn(&mut state.items, turn);
    }
    let mut changed = false;
    replace_turn(&mut state.items, turn_id, |entry| {
        let Item::Turn { steps, .. } = entry else {
            return;
        };
        if let Some(existing) = steps.iter().find(|s| s.step_id == header.step_id) {
            if step_equals(existing, header) {
                return;
            }
            // Replace header, keep frames.
            let frames = existing.frames.clone();
            let mut next = step_header_to_step(header, frames);
            next.kind = StepKind::Step;
            if let Some(slot) = steps.iter_mut().find(|s| s.step_id == header.step_id) {
                *slot = next;
            }
            changed = true;
        } else {
            let mut next = step_header_to_step(header, Vec::new());
            next.kind = StepKind::Step;
            steps.push(next);
            // `toSorted` is stable; Rust `sort_by` is stable too.
            steps.sort_by_key(|s| s.ordinal);
            changed = true;
        }
    });
    if changed {
        OperationResult {
            state,
            changed: true,
            gap: None,
        }
    } else {
        OperationResult::unchanged(state)
    }
}

/// `stepHeaderToStep` — `{ ...header, kind: 'step', frames }`.
fn step_header_to_step(header: &StepHeader, frames: Vec<Frame>) -> dimi_wire::item::Step {
    dimi_wire::item::Step {
        kind: StepKind::Step,
        step_id: header.step_id.clone(),
        turn_id: header.turn_id.clone(),
        ordinal: header.ordinal,
        state: header.state,
        frames,
        started_at: header.started_at.clone(),
        ended_at: header.ended_at.clone(),
        usage: header.usage,
        finish_reason: header.finish_reason.clone(),
        timing: header.timing,
        retry: header.retry.clone(),
        end_reason: header.end_reason.clone(),
        end_message: header.end_message.clone(),
    }
}

fn frame_equals(a: &Frame, b: &Frame) -> bool {
    a == b
}

fn apply_frame_upsert(
    mut state: AgentState,
    turn_id: &str,
    step_id: &str,
    frame: &Frame,
) -> OperationResult {
    if get_turn(&state, turn_id).is_none() {
        insert_turn(&mut state.items, skeleton_turn(turn_id));
    }
    let mut changed = false;
    replace_turn(&mut state.items, turn_id, |entry| {
        let Item::Turn {
            turn_id: turn_id_field,
            steps,
            ..
        } = entry
        else {
            return;
        };
        let turn_id_str = turn_id_field.as_str().to_owned();
        let existing_step = steps.iter().find(|s| s.step_id.as_str() == step_id);
        if let Some(step) = existing_step {
            let mut next = step.clone();
            let mut step_changed = false;
            if let Some(existing_frame) = next
                .frames
                .iter()
                .find(|f| frame_id_of(f) == frame_id_of(frame))
            {
                if !frame_equals(existing_frame, frame) {
                    if let Some(slot) = next
                        .frames
                        .iter_mut()
                        .find(|f| frame_id_of(f) == frame_id_of(frame))
                    {
                        *slot = frame.clone();
                    }
                    step_changed = true;
                }
            } else {
                // Frames keep arrival order — never sorted.
                next.frames.push(frame.clone());
                step_changed = true;
            }
            if step_changed {
                if let Some(slot) = steps.iter_mut().find(|s| s.step_id.as_str() == step_id) {
                    *slot = next;
                }
                changed = true;
            }
        } else {
            let mut next = skeleton_step(step_id, &turn_id_str);
            next.frames.push(frame.clone());
            steps.push(next);
            steps.sort_by_key(|s| s.ordinal);
            changed = true;
        }
    });
    if changed {
        OperationResult {
            state,
            changed: true,
            gap: None,
        }
    } else {
        OperationResult::unchanged(state)
    }
}

fn frame_id_of(frame: &Frame) -> &str {
    match frame {
        Frame::Text { frame_id, .. } => frame_id.as_str(),
        Frame::Thinking { frame_id, .. } => frame_id.as_str(),
        Frame::Tool { frame_id, .. } => frame_id.as_str(),
        Frame::Notice { frame_id, .. } => frame_id.as_str(),
    }
}

// ------------------------------------------------------------ append

/// `appendAtOffset` (apply.ts 379–399) — the only non-idempotent op.
pub fn append_at_offset(local: &str, offset: i64, chunk: &str) -> AppendOutcome {
    let local_len = local.len() as i64;
    if offset > local_len {
        return AppendOutcome::Gap {
            expected: local_len,
            got: offset,
        };
    }
    let offset_us = offset as usize;
    let chunk_bytes = chunk.as_bytes();
    let local_bytes = local.as_bytes();
    if local_bytes.get(offset_us..offset_us.saturating_add(chunk_bytes.len())) == Some(chunk_bytes)
    {
        return AppendOutcome::Unchanged;
    }
    let overlap = local_len - offset;
    let overlap_us = overlap as usize;
    let local_tail = local_bytes.get(offset_us..).unwrap_or_default();
    let chunk_head = chunk_bytes.get(..overlap_us).unwrap_or_default();
    if local_tail != chunk_head {
        return AppendOutcome::Gap {
            expected: local_len,
            got: offset,
        };
    }
    let novel = if overlap_us > 0 {
        chunk_bytes.get(overlap_us..).unwrap_or_default()
    } else {
        chunk_bytes
    };
    if novel.is_empty() {
        return AppendOutcome::Unchanged;
    }
    let mut text = local.to_owned();
    text.replace_range(offset_us.., chunk);
    AppendOutcome::Changed { text }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendOutcome {
    Changed { text: String },
    Unchanged,
    Gap { expected: i64, got: i64 },
}

fn apply_append(
    state: AgentState,
    target: &AppendTarget,
    offset: i64,
    text: &str,
) -> OperationResult {
    match target {
        AppendTarget::Task { task_id } => {
            let task = state.tasks.get(task_id);
            let current = task.map(|t| t.output_tail.clone()).unwrap_or_default();
            match append_at_offset(&current, offset, text) {
                AppendOutcome::Gap { expected, got } => OperationResult {
                    state,
                    changed: false,
                    gap: Some(GapInfo { expected, got }),
                },
                AppendOutcome::Unchanged => OperationResult::unchanged(state),
                AppendOutcome::Changed { text: merged } => {
                    let mut tasks = state.tasks.clone();
                    let next = task.cloned().map(|mut t| {
                        t.output_tail = merged.clone();
                        t
                    });
                    let next = next.unwrap_or_else(|| dimi_wire::task::Task {
                        task_id: task_id.clone(),
                        kind: dimi_wire::task::TaskKind::Other,
                        state: dimi_wire::task::TaskState::Running,
                        detached: false,
                        description: None,
                        agent_id: None,
                        output_tail: merged,
                        started_at: None,
                        ended_at: None,
                        result_summary: None,
                        error: None,
                        state_reason: None,
                        usage: None,
                    });
                    tasks.insert(task_id.clone(), next);
                    OperationResult {
                        state: AgentState { tasks, ..state },
                        changed: true,
                        gap: None,
                    }
                }
            }
        }
        AppendTarget::Frame {
            turn_id,
            step_id,
            frame_id,
        } => {
            let turn = get_turn(&state, turn_id.as_str()).cloned();
            let Some(turn) = turn else {
                return OperationResult {
                    state,
                    changed: false,
                    gap: Some(GapInfo {
                        expected: 0,
                        got: offset,
                    }),
                };
            };
            let Item::Turn { steps, .. } = &turn else {
                unreachable!()
            };
            let Some(step) = steps
                .iter()
                .find(|s| s.step_id.as_str() == step_id.as_str())
            else {
                return OperationResult {
                    state,
                    changed: false,
                    gap: Some(GapInfo {
                        expected: 0,
                        got: offset,
                    }),
                };
            };
            let Some(frame) = step
                .frames
                .iter()
                .find(|f| frame_id_of(f) == frame_id.as_str())
            else {
                return OperationResult {
                    state,
                    changed: false,
                    gap: Some(GapInfo {
                        expected: 0,
                        got: offset,
                    }),
                };
            };
            let frame_text = match frame {
                Frame::Text { text, .. } | Frame::Thinking { text, .. } => text,
                _ => {
                    return OperationResult {
                        state,
                        changed: false,
                        gap: Some(GapInfo {
                            expected: 0,
                            got: offset,
                        }),
                    };
                }
            };
            match append_at_offset(frame_text, offset, text) {
                AppendOutcome::Gap { expected, got } => OperationResult {
                    state,
                    changed: false,
                    gap: Some(GapInfo { expected, got }),
                },
                AppendOutcome::Unchanged => OperationResult::unchanged(state),
                AppendOutcome::Changed { text: merged } => {
                    let mut state = state;
                    replace_turn(&mut state.items, turn_id.as_str(), |entry| {
                        let Item::Turn { steps, .. } = entry else {
                            return;
                        };
                        for step in steps.iter_mut() {
                            if step.step_id.as_str() != step_id.as_str() {
                                continue;
                            }
                            for frame in step.frames.iter_mut() {
                                if frame_id_of(frame) != frame_id.as_str() {
                                    continue;
                                }
                                match frame {
                                    Frame::Text { text, .. } | Frame::Thinking { text, .. } => {
                                        *text = merged.clone();
                                    }
                                    _ => {}
                                }
                            }
                        }
                    });
                    OperationResult {
                        state,
                        changed: true,
                        gap: None,
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------ marker / taskref

fn apply_item_upsert(
    mut state: AgentState,
    item: &Item,
    before_turn: Option<i64>,
) -> OperationResult {
    let id = item_id_of(item);
    let exists = state.items.iter().any(|entry| item_id_of(entry) == id);
    if exists {
        // In-place replace; position unchanged. TS short-circuits on
        // reference equality — Rust compares by value, so a byte-identical
        // item still replaces (converged state, extra event).
        for entry in state.items.iter_mut() {
            if item_id_of(entry) == id {
                *entry = item.clone();
                break;
            }
        }
        OperationResult {
            state,
            changed: true,
            gap: None,
        }
    } else if let Some(anchor) = before_turn {
        // First turn with `ordinal >= beforeTurn` wins; else append.
        let at = state
            .items
            .iter()
            .position(|entry| matches!(entry, Item::Turn { ordinal, .. } if *ordinal >= anchor))
            .unwrap_or(state.items.len());
        state.items.insert(at, item.clone());
        OperationResult {
            state,
            changed: true,
            gap: None,
        }
    } else {
        state.items.push(item.clone());
        OperationResult {
            state,
            changed: true,
            gap: None,
        }
    }
}

// ------------------------------------------------------------ entity upserts

fn apply_task_upsert(mut state: AgentState, task: &dimi_wire::task::Task) -> OperationResult {
    if state.tasks.get(&task.task_id) == Some(task) {
        return OperationResult::unchanged(state);
    }
    state.tasks.insert(task.task_id.clone(), task.clone());
    OperationResult {
        state,
        changed: true,
        gap: None,
    }
}

fn apply_interaction_upsert(mut state: AgentState, interaction: &Interaction) -> OperationResult {
    if state.interactions.get(&interaction.interaction_id) == Some(interaction) {
        return OperationResult::unchanged(state);
    }
    if interaction.state == InteractionState::Pending {
        state
            .pending_interactions
            .insert(interaction.interaction_id.clone());
    } else {
        state
            .pending_interactions
            .remove(&interaction.interaction_id);
    }
    state
        .interactions
        .insert(interaction.interaction_id.clone(), interaction.clone());
    OperationResult {
        state,
        changed: true,
        gap: None,
    }
}

fn apply_attachment_upsert(
    mut state: AgentState,
    attachment: &dimi_wire::entity::Attachment,
) -> OperationResult {
    if state.attachments.get(&attachment.attachment_id) == Some(attachment) {
        return OperationResult::unchanged(state);
    }
    state
        .attachments
        .insert(attachment.attachment_id.clone(), attachment.clone());
    OperationResult {
        state,
        changed: true,
        gap: None,
    }
}

fn apply_todo_upsert(mut state: AgentState, todo: &dimi_wire::entity::Todo) -> OperationResult {
    if state.todos.get(&todo.todo_id) == Some(todo) {
        return OperationResult::unchanged(state);
    }
    state.todos.insert(todo.todo_id.clone(), todo.clone());
    OperationResult {
        state,
        changed: true,
        gap: None,
    }
}

fn apply_prompt_upsert(
    mut state: AgentState,
    prompt: &dimi_wire::entity::Prompt,
) -> OperationResult {
    if state.prompts.get(&prompt.prompt_id) == Some(prompt) {
        return OperationResult::unchanged(state);
    }
    state
        .prompts
        .insert(prompt.prompt_id.clone(), prompt.clone());
    OperationResult {
        state,
        changed: true,
        gap: None,
    }
}

// ------------------------------------------------------------ meta.merge

fn merge_agent_status(
    base: &Option<dimi_wire::entity::AgentStatusMeta>,
    patch: &dimi_wire::entity::AgentStatusMeta,
) -> Option<dimi_wire::entity::AgentStatusMeta> {
    let base = base.clone().unwrap_or_default();
    Some(dimi_wire::entity::AgentStatusMeta {
        model: patch.model.clone().or(base.model),
        thinking_effort: patch.thinking_effort.clone().or(base.thinking_effort),
        usage: patch.usage.clone().or(base.usage),
        context_tokens: patch.context_tokens.or(base.context_tokens),
        max_context_tokens: patch.max_context_tokens.or(base.max_context_tokens),
        context_usage: patch.context_usage.or(base.context_usage),
        permission: patch.permission.or(base.permission),
        phase: patch.phase.clone().or(base.phase),
    })
}

/// `meta.merge` (apply.ts 602–631): modes null-clears a badge, agent merges
/// one level, activity keep-on-undefined (`??` — no null-clear).
fn apply_meta_merge(mut state: AgentState, meta: &TranscriptMetaMerge) -> OperationResult {
    let before = state.meta.clone();

    if let Some(modes_merge) = &meta.modes {
        let base_modes = state.meta.modes.clone().unwrap_or_default();
        let mut plan = base_modes.plan.clone();
        if let Some(p) = &modes_merge.plan {
            plan = p.clone();
        }
        let mut swarm = base_modes.swarm.clone();
        if let Some(s) = &modes_merge.swarm {
            swarm = s.clone();
        }
        if plan.is_none() && swarm.is_none() {
            state.meta.modes = None;
        } else {
            state.meta.modes = Some(dimi_wire::entity::ModesMeta { plan, swarm });
        }
    }
    if let Some(agent) = &meta.agent {
        state.meta.agent = merge_agent_status(&state.meta.agent, agent);
    }
    if meta.activity.is_some() {
        state.meta.activity = meta.activity;
    }

    let changed = state.meta != before;
    OperationResult {
        state,
        changed,
        gap: None,
    }
}

// ------------------------------------------------------------ items.remove

/// `items.remove` (apply.ts 452–488): filter by id; cascade-delete
/// interactions anchored on removed turns' tool frames.
fn apply_items_remove(mut state: AgentState, ids: &[String]) -> OperationResult {
    let mut removed = Vec::new();
    state.items.retain(|entry| {
        let keep = !ids.contains(&item_id_of(entry));
        if !keep {
            removed.push(entry.clone());
        }
        keep
    });
    if removed.is_empty() {
        return OperationResult::unchanged(state);
    }
    let mut tool_call_ids = std::collections::HashSet::new();
    for entry in &removed {
        if let Item::Turn { steps, .. } = entry {
            for step in steps {
                for frame in &step.frames {
                    if let Frame::Tool { tool_call_id, .. } = frame {
                        tool_call_ids.insert(tool_call_id.clone());
                    }
                }
            }
        }
    }
    if !tool_call_ids.is_empty() {
        state.interactions.retain(|_, i| {
            !i.tool_call_id
                .as_ref()
                .is_some_and(|id| tool_call_ids.contains(id))
        });
        state
            .pending_interactions
            .retain(|id| state.interactions.contains_key(id));
    }
    OperationResult {
        state,
        changed: true,
        gap: None,
    }
}

// ------------------------------------------------------------ dispatch

/// `applyOperation(state, op)` (apply.ts 67–98).
pub fn apply_operation(state: AgentState, op: &Operation) -> OperationResult {
    match op {
        Operation::Reset { snapshot, .. } => apply_reset(state, snapshot),
        Operation::TurnUpsert { turn } => apply_turn_upsert(state, turn),
        Operation::StepUpsert { turn_id, step } => apply_step_upsert(state, turn_id.as_str(), step),
        Operation::FrameUpsert {
            turn_id,
            step_id,
            frame,
        } => apply_frame_upsert(state, turn_id.as_str(), step_id.as_str(), frame),
        Operation::Append {
            target,
            offset,
            text,
        } => apply_append(state, target, *offset, text),
        Operation::MarkerUpsert { item, before_turn } => {
            apply_item_upsert(state, item, *before_turn)
        }
        Operation::TaskRefUpsert { item, before_turn } => {
            apply_item_upsert(state, item, *before_turn)
        }
        Operation::TaskUpsert { task } => apply_task_upsert(state, task),
        Operation::InteractionUpsert { interaction } => {
            apply_interaction_upsert(state, interaction)
        }
        Operation::AttachmentUpsert { attachment } => apply_attachment_upsert(state, attachment),
        Operation::TodoUpsert { todo } => apply_todo_upsert(state, todo),
        Operation::PromptUpsert { prompt } => apply_prompt_upsert(state, prompt),
        Operation::MetaMerge { meta } => apply_meta_merge(state, meta),
        Operation::ItemsRemove { ids } => apply_items_remove(state, ids),
    }
}

#[allow(dead_code)]
fn _unused_imports_guard() {
    // HashMap is used indirectly via AgentState construction.
    let _ = std::any::type_name::<HashMap<String, String>>();
    let _ = empty_agent_state();
}
