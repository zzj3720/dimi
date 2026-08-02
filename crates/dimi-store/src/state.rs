//! The in-memory agent transcript state (`AgentState`, apply.ts 29–57).

use std::collections::{HashMap, HashSet};

use dimi_wire::entity::{Attachment, Interaction, Prompt, Todo, TranscriptMeta};
use dimi_wire::id::TaskId;
use dimi_wire::item::Item;
use dimi_wire::task::Task;

/// `AgentState` — the converged state of one agent's transcript.
///
/// `items` is the only ordered structure (the timeline); the entity maps are
/// global, last-wins documents. `pending_interactions` is a derived index
/// (interactions with `state == pending`), kept in sync by the reducer.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentState {
    pub items: Vec<Item>,
    pub tasks: HashMap<TaskId, Task>,
    pub interactions: HashMap<String, Interaction>,
    pub attachments: HashMap<String, Attachment>,
    pub todos: HashMap<String, Todo>,
    pub prompts: HashMap<String, Prompt>,
    pub meta: TranscriptMeta,
    pub pending_interactions: HashSet<String>,
    pub has_more_older: bool,
}

/// `EMPTY_AGENT_STATE` (apply.ts 47–57).
pub fn empty_agent_state() -> AgentState {
    AgentState {
        items: Vec::new(),
        tasks: HashMap::new(),
        interactions: HashMap::new(),
        attachments: HashMap::new(),
        todos: HashMap::new(),
        prompts: HashMap::new(),
        meta: TranscriptMeta::default(),
        pending_interactions: HashSet::new(),
        has_more_older: false,
    }
}
