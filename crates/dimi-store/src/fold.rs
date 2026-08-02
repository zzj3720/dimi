//! `foldWireRecordFacts` — non-`context.*` wire records → tasks,
//! interactions, todos, meta, appended markers/taskrefs
//! (`packages/transcript/src/history/foldFacts.ts`).
//!
//! Last-wins folding over the whole record list; interactions left pending
//! at the end fold to `cancelled` (a process that died mid-request is a
//! cancellation, never a ghost pending).

use std::collections::HashSet;

use chrono::{DateTime, SecondsFormat, Utc};
use dimi_wire::entity::{
    GoalMeta, GoalStatus, Interaction, InteractionKind, InteractionState, Todo, TodoItem,
    TodoItemStatus, TranscriptMeta,
};
use dimi_wire::item::Item;
use dimi_wire::record::WireRecord;
use dimi_wire::snapshot::AgentTranscriptSnapshot;
use dimi_wire::task::{Task, TaskKind, TaskState};

const TASK_STATES: &[&str] = &[
    "running",
    "completed",
    "failed",
    "timed_out",
    "killed",
    "lost",
];
const GOAL_STATUSES: &[&str] = &["active", "paused", "blocked", "complete"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanRevision {
    review_path: Option<String>,
    version: Option<i64>,
}

/// `foldWireRecordFacts` (foldFacts.ts 170–425).
pub fn fold_wire_record_facts(
    records: &[WireRecord],
    base: &AgentTranscriptSnapshot,
) -> AgentTranscriptSnapshot {
    let mut tasks: Vec<Task> = Vec::new();
    // Insertion-ordered like the TS `Map`s (foldFacts.ts) — fold output must
    // not depend on hash order.
    let mut interactions: Vec<Interaction> = Vec::new();
    let mut todo: Option<Todo> = None;
    let mut goal: Option<GoalMeta> = None;
    let mut goal_touched = false;
    let mut plan_active: Option<bool> = None;
    let mut plan_revision: Option<PlanRevision> = None;
    let mut swarm_active: Option<bool> = None;

    let mut appended: Vec<Item> = Vec::new();
    let mut marker_seq: i64 = 0;
    let mut used_ref_ids: HashSet<String> = HashSet::new();
    for item in &base.items {
        match item {
            Item::Marker { marker_id, .. } => {
                if let Some(digits) = marker_id.strip_prefix('m') {
                    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                        if let Ok(n) = digits.parse::<i64>() {
                            marker_seq = marker_seq.max(n);
                        }
                    }
                }
            }
            Item::TaskRef { ref_id, .. } => {
                used_ref_ids.insert(ref_id.clone());
            }
            _ => {}
        }
    }

    let mut push_marker = |marker: &str, record: &WireRecord, appended: &mut Vec<Item>| {
        marker_seq += 1;
        appended.push(Item::Marker {
            marker_id: format!("m{marker_seq}"),
            marker: marker.to_owned(),
            payload: Some(payload_of(record)),
            at: record_time_iso(record),
        });
    };

    for record in records {
        match record.r#type.as_str() {
            "tools.update_store" => {
                if record.rest.get("key").and_then(|v| v.as_str()) != Some("todo") {
                    continue;
                }
                todo = Some(Todo {
                    todo_id: "todo".into(),
                    items: read_todo_items(record.rest.get("value")),
                    updated_at: record_time_iso(record),
                });
            }
            "goal.create" => {
                goal_touched = true;
                goal = Some(GoalMeta {
                    objective: record
                        .rest
                        .get("objective")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    status: GoalStatus::Active,
                    completion_criterion: record
                        .rest
                        .get("completionCriterion")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                    budget_used: Some(0),
                    budget_limit: None,
                });
                push_marker("goal", record, &mut appended);
            }
            "goal.update" => {
                goal_touched = true;
                if let Some(current) = goal.as_mut() {
                    if let Some(status) = record.rest.get("status").and_then(|v| v.as_str()) {
                        if GOAL_STATUSES.contains(&status) {
                            current.status = match status {
                                "paused" => GoalStatus::Paused,
                                "blocked" => GoalStatus::Blocked,
                                "complete" => GoalStatus::Complete,
                                _ => GoalStatus::Active,
                            };
                        }
                    }
                    if let Some(tokens_used) =
                        record.rest.get("tokensUsed").and_then(|v| v.as_i64())
                    {
                        current.budget_used = Some(tokens_used);
                    }
                    let token_budget = record
                        .rest
                        .get("budgetLimits")
                        .and_then(|v| v.get("tokenBudget"))
                        .and_then(|v| v.as_i64());
                    if let Some(token_budget) = token_budget {
                        current.budget_limit = Some(token_budget);
                    }
                }
                push_marker("goal", record, &mut appended);
            }
            "goal.clear" => {
                goal_touched = true;
                goal = None;
            }
            "plan_mode.enter" => {
                plan_active = Some(true);
                plan_revision = None;
                push_marker("plan.enter", record, &mut appended);
            }
            "plan_mode.exit" | "plan_mode.cancel" => {
                plan_active = Some(false);
                plan_revision = None;
                push_marker("plan.exit", record, &mut appended);
            }
            "plan.revision" => {
                plan_active = Some(true);
                plan_revision = Some(PlanRevision {
                    review_path: record
                        .rest
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                    version: record.rest.get("version").and_then(|v| v.as_i64()),
                });
                push_marker("plan.revision", record, &mut appended);
            }
            "swarm_mode.enter" => {
                swarm_active = Some(true);
                push_marker("swarm.enter", record, &mut appended);
            }
            "swarm_mode.exit" => {
                swarm_active = Some(false);
                push_marker("swarm.exit", record, &mut appended);
            }
            "task.started" | "task.terminated" => {
                upsert_task(record, &mut tasks, &mut used_ref_ids, &mut appended);
            }
            "interaction.request" => {
                let kind = record.rest.get("kind").and_then(|v| v.as_str());
                if kind != Some("approval") && kind != Some("question") {
                    continue;
                }
                let Some(id) = record.rest.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let request_tool_call_id = record
                    .rest
                    .get("request")
                    .and_then(|v| v.get("toolCallId"))
                    .and_then(|v| v.as_str());
                let tool_call_id = record
                    .rest
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .or(request_tool_call_id)
                    .map(str::to_owned);
                interactions.push(Interaction {
                    interaction_id: id.to_owned(),
                    interaction_kind: if kind == Some("question") {
                        InteractionKind::Question
                    } else {
                        InteractionKind::Approval
                    },
                    tool_call_id,
                    state: InteractionState::Pending,
                    request: record.rest.get("request").cloned(),
                    response: None,
                });
            }
            "interaction.resolved" => {
                let Some(id) = record.rest.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(entity) = interactions
                    .iter()
                    .find(|e| e.interaction_id == id)
                    .cloned()
                else {
                    continue;
                };
                let response = record.rest.get("response").cloned();
                let state = map_interaction_end_state(entity.interaction_kind, response.as_ref());
                let mut next = entity.clone();
                next.state = state;
                next.response = response;
                if let Some(existing) = interactions.iter_mut().find(|e| e.interaction_id == id) {
                    *existing = next;
                }
            }
            _ => {}
        }
    }

    // Pending → cancelled (crash == cancellation).
    for entity in interactions.iter_mut() {
        if entity.state == InteractionState::Pending {
            entity.state = InteractionState::Cancelled;
        }
    }

    let modes_touched = plan_active.is_some() || swarm_active.is_some();
    let meta = TranscriptMeta {
        goal: if goal_touched {
            goal.clone()
        } else {
            base.meta.goal.clone()
        },
        modes: if modes_touched {
            Some(dimi_wire::entity::ModesMeta {
                plan: match plan_active {
                    None => base.meta.modes.as_ref().and_then(|m| m.plan.clone()),
                    // `planRevision ?? {}` — active plan mode with no revision
                    // record (older sessions) still carries a bare badge.
                    Some(true) => Some(plan_revision.map_or_else(
                        || dimi_wire::entity::PlanMeta {
                            review_path: None,
                            version: None,
                        },
                        |r| dimi_wire::entity::PlanMeta {
                            review_path: r.review_path,
                            version: r.version,
                        },
                    )),
                    Some(false) => None,
                },
                swarm: match swarm_active {
                    None => base.meta.modes.as_ref().and_then(|m| m.swarm.clone()),
                    Some(true) => Some(dimi_wire::entity::SwarmMeta { trigger: None }),
                    Some(false) => None,
                },
            })
        } else {
            base.meta.modes.clone()
        },
        activity: base.meta.activity,
        agent: base.meta.agent.clone(),
    };

    AgentTranscriptSnapshot {
        items: if appended.is_empty() {
            base.items.clone()
        } else {
            let mut items = base.items.clone();
            items.extend(appended);
            items
        },
        tasks,
        interactions,
        attachments: base.attachments.clone(),
        todos: match todo {
            Some(todo) => vec![todo],
            None => base.todos.clone(),
        },
        prompts: base.prompts.clone(),
        meta,
        has_more_older: base.has_more_older,
    }
}

// ---------------------------------------------------------------- helpers

/// `recordTimeIso` (foldFacts.ts 137–142): finite epoch ms → ISO; ISO
/// strings pass through; else undefined.
fn record_time_iso(record: &WireRecord) -> Option<String> {
    match &record.time {
        Some(dimi_wire::record::RecordTime::Iso(iso)) => Some(iso.clone()),
        Some(other) => epoch_ms_to_iso(other.as_ms()?),
        None => None,
    }
}

/// `epochMsToIso` (foldFacts.ts 144–148) — JS `new Date(ms).toISOString()`.
fn epoch_ms_to_iso(ms: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// `payloadOf` — record fields minus the envelope; for the Rust mirror the
/// `rest` map already excludes `type`/`time`.
fn payload_of(record: &WireRecord) -> serde_json::Value {
    serde_json::Value::Object(record.rest.clone())
}

/// `readTodoItems` (foldFacts.ts 157–168): keep only well-formed entries.
fn read_todo_items(raw: Option<&serde_json::Value>) -> Vec<TodoItem> {
    let Some(serde_json::Value::Array(entries)) = raw else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries {
        let Some(title) = entry.get("title").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(status) = entry.get("status").and_then(|v| v.as_str()) else {
            continue;
        };
        let status = match status {
            "pending" => TodoItemStatus::Pending,
            "in_progress" => TodoItemStatus::InProgress,
            "done" => TodoItemStatus::Done,
            _ => continue,
        };
        items.push(TodoItem {
            title: title.to_owned(),
            status,
        });
    }
    items
}

/// `mapTaskKind` (foldFacts.ts 101–110).
fn map_task_kind(kind: Option<&str>) -> TaskKind {
    match kind {
        Some("process") => TaskKind::Shell,
        Some("agent") => TaskKind::Subagent,
        _ => TaskKind::Other,
    }
}

/// `upsertTask` (foldFacts.ts 210–250).
fn upsert_task(
    record: &WireRecord,
    tasks: &mut Vec<Task>,
    used_ref_ids: &mut HashSet<String>,
    appended: &mut Vec<Item>,
) {
    let Some(info) = record.rest.get("info") else {
        return;
    };
    let Some(task_id) = info.get("taskId").and_then(|v| v.as_str()) else {
        return;
    };
    let prev = tasks
        .iter()
        .find(|t| t.task_id.as_str() == task_id)
        .cloned();
    let status = info.get("status").and_then(|v| v.as_str());
    let state = if status.is_some_and(|s| TASK_STATES.contains(&s)) {
        match status {
            Some("completed") => TaskState::Completed,
            Some("failed") => TaskState::Failed,
            Some("timed_out") => TaskState::TimedOut,
            Some("killed") => TaskState::Killed,
            Some("lost") => TaskState::Lost,
            _ => TaskState::Running,
        }
    } else {
        prev.as_ref().map(|p| p.state).unwrap_or(TaskState::Running)
    };
    let detached = match info.get("detached").and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => prev.as_ref().map(|p| p.detached).unwrap_or(true),
    };
    let output_tail = record
        .rest
        .get("outputTail")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            prev.as_ref()
                .map(|p| p.output_tail.clone())
                .unwrap_or_default()
        });
    let started_at = prev
        .as_ref()
        .and_then(|p| p.started_at.clone())
        .or_else(|| {
            info.get("startedAt")
                .and_then(|v| v.as_i64())
                .and_then(epoch_ms_to_iso)
        });
    let ended_at = info
        .get("endedAt")
        .and_then(|v| v.as_i64())
        .and_then(epoch_ms_to_iso)
        .or_else(|| prev.as_ref().and_then(|p| p.ended_at.clone()));

    let task = Task {
        task_id: dimi_wire::id::TaskId::new_unchecked(task_id.to_owned()),
        kind: map_task_kind(info.get("kind").and_then(|v| v.as_str())),
        state,
        detached,
        description: info
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or_else(|| prev.as_ref().and_then(|p| p.description.clone())),
        agent_id: info
            .get("agentId")
            .and_then(|v| v.as_str())
            .map(|s| dimi_wire::id::AgentId::new_unchecked(s.to_owned()))
            .or_else(|| prev.as_ref().and_then(|p| p.agent_id.clone())),
        output_tail,
        started_at,
        ended_at,
        result_summary: None,
        error: None,
        state_reason: None,
        usage: None,
    };
    if let Some(existing) = tasks.iter_mut().find(|t| t.task_id.as_str() == task_id) {
        *existing = task;
    } else {
        tasks.push(task);
    }

    if record.r#type == "task.started" {
        let ref_id = format!("ref-{task_id}");
        if !used_ref_ids.contains(&ref_id) {
            used_ref_ids.insert(ref_id.clone());
            appended.push(Item::TaskRef {
                ref_id,
                task_id: dimi_wire::id::TaskId::new_unchecked(task_id.to_owned()),
                at: record_time_iso(record),
            });
        }
    }
}

/// `mapInteractionEndState` (foldFacts.ts 124–134).
fn map_interaction_end_state(
    kind: InteractionKind,
    response: Option<&serde_json::Value>,
) -> InteractionState {
    if kind == InteractionKind::Question {
        return match response {
            Some(serde_json::Value::Null) => InteractionState::Dismissed,
            _ => InteractionState::Answered,
        };
    }
    let decision = response
        .and_then(|r| r.get("decision"))
        .and_then(|v| v.as_str());
    match decision {
        Some("approved") => InteractionState::Approved,
        Some("rejected") => InteractionState::Rejected,
        Some("cancelled") => InteractionState::Cancelled,
        _ => InteractionState::Cancelled,
    }
}

#[allow(dead_code)]
fn _payload_of_unused() -> serde_json::Value {
    // Kept as the named mirror of payloadOf; call sites inline `rest` clones.
    serde_json::Value::Null
}
