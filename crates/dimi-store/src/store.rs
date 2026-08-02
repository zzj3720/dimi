//! `AgentTranscript` — one agent's transcript store (`agentTranscript.ts`).

use dimi_wire::item::Item;
use dimi_wire::op::{AppendTarget, Operation};
use dimi_wire::snapshot::AgentTranscriptSnapshot;

use crate::apply::apply_operation;
use crate::state::{AgentState, empty_agent_state};

/// `AppliedOps` — the batch-apply result (operation.ts 185–190).
///
/// `gap` must OMIT the key when `None` (never `null`): TS consumers check
/// `result.gap !== undefined` to distinguish a clean batch from a gap, and a
/// JSON `null` would read as a gap and silently drop every batch.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AppliedOps {
    /// Ops that actually changed the state, in arrival order.
    pub accepted: Vec<Operation>,
    /// The LAST gap of the batch, with the append target attached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<BatchGap>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BatchGap {
    pub target: AppendTarget,
    pub expected: i64,
    pub got: i64,
}

/// One agent's transcript store — the convergence entry point.
///
/// Mirror of `AgentTranscript` (agentTranscript.ts 42–184). Copy-on-write
/// semantics are not needed in Rust: `apply` mutates in place, and `items()`
/// hands out clones to keep the "shared read view stays stable" contract.
#[derive(Debug, Clone)]
pub struct AgentTranscript {
    agent_id: String,
    state: AgentState,
}

impl AgentTranscript {
    pub fn new(agent_id: impl Into<String>) -> Self {
        AgentTranscript {
            agent_id: agent_id.into(),
            state: empty_agent_state(),
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn state(&self) -> &AgentState {
        &self.state
    }

    /// `apply(ops)` (agentTranscript.ts 58–78): sequential application, gap
    /// ops dropped (state not advanced, not accepted), only the last gap
    /// reported, changed ops collected.
    pub fn apply(&mut self, ops: &[Operation]) -> AppliedOps {
        let mut accepted = Vec::new();
        let mut gap: Option<BatchGap> = None;
        for op in ops {
            let result = apply_operation(self.state.clone(), op);
            if let Some(g) = result.gap {
                let target = match op {
                    Operation::Append { target, .. } => target.clone(),
                    _ => unreachable!("gap can only come from append"),
                };
                gap = Some(BatchGap {
                    target,
                    expected: g.expected,
                    got: g.got,
                });
                continue;
            }
            if !result.changed {
                continue;
            }
            self.state = result.state;
            accepted.push(op.clone());
        }
        AppliedOps { accepted, gap }
    }

    /// `receive(ops)` — identical to apply (full load is a reset).
    pub fn receive(&mut self, ops: &[Operation]) -> AppliedOps {
        self.apply(ops)
    }

    /// `snapshot(window?)` (agentTranscript.ts 151–184): ALWAYS emits all
    /// eight keys; `{ tailTurns }` windows items to the newest turns and sets
    /// `hasMoreOlder`.
    pub fn snapshot(&self, window: Option<SnapshotWindow>) -> AgentTranscriptSnapshot {
        let mut items = self.state.items.clone();
        let mut has_more_older = self.state.has_more_older;
        if let Some(window) = window {
            let turn_count = items
                .iter()
                .filter(|e| matches!(e, Item::Turn { .. }))
                .count();
            if turn_count as i64 > window.tail_turns {
                let skip = turn_count as i64 - window.tail_turns;
                let mut kept = Vec::new();
                let mut seen = 0i64;
                for entry in items {
                    if matches!(entry, Item::Turn { .. }) {
                        seen += 1;
                        if seen <= skip {
                            continue;
                        }
                        kept.push(entry);
                    } else if seen > skip {
                        kept.push(entry);
                    }
                }
                items = kept;
                has_more_older = true;
            }
        }
        AgentTranscriptSnapshot {
            items,
            tasks: self.state.tasks.values().cloned().collect(),
            interactions: self.state.interactions.values().cloned().collect(),
            attachments: self.state.attachments.values().cloned().collect(),
            todos: self.state.todos.values().cloned().collect(),
            prompts: self.state.prompts.values().cloned().collect(),
            meta: self.state.meta.clone(),
            has_more_older: Some(has_more_older),
        }
    }

    /// `getItems()` — clone of the timeline.
    pub fn items(&self) -> Vec<Item> {
        self.state.items.clone()
    }

    /// `getTurn(turnId)` — linear find.
    pub fn get_turn(&self, turn_id: &str) -> Option<&Item> {
        self.state
            .items
            .iter()
            .find(|entry| matches!(entry, Item::Turn { turn_id: id, .. } if id.as_str() == turn_id))
    }

    pub fn has_more_older(&self) -> bool {
        self.state.has_more_older
    }
}

/// `{ tailTurns }` snapshot window — camelCase on the wire (`snapshot()`
/// in agentTranscript.ts takes `{ tailTurns }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotWindow {
    pub tail_turns: i64,
}
