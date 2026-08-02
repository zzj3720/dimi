//! The agent transcript snapshot (`agentTranscriptSnapshotSchema`,
//! `schema.ts` 383–393) — the full state carried by `reset` and REST reads.
//!
//! Serialization matches `AgentTranscript.snapshot()` (agentTranscript.ts
//! 151–184) on the live path: ALL keys are always emitted — empty arrays
//! included, `meta` always present (possibly `{}`), `hasMoreOlder` always a
//! boolean. The cold rebuild (`groupMessagesIntoSnapshot` +
//! `foldWireRecordFacts`) produces a snapshot WITHOUT `hasMoreOlder` (the TS
//! cold path never sets it), so the field is optional here and `None` omits
//! the key — the live store always passes `Some(..)`.

use serde::{Deserialize, Serialize};

use crate::entity::{Attachment, Interaction, Prompt, Todo, TranscriptMeta};
use crate::item::Item;
use crate::task::Task;

/// `agentTranscriptSnapshotSchema` (`schema.ts` 383–393).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTranscriptSnapshot {
    pub items: Vec<Item>,
    pub tasks: Vec<Task>,
    /// Added later in the TS schema; `#[serde(default)]` tolerates older
    /// servers, but serialization ALWAYS emits the key (like the TS
    /// `snapshot()` method, which spreads `[...state.interactions.values()]`).
    #[serde(default)]
    pub interactions: Vec<Interaction>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub todos: Vec<Todo>,
    #[serde(default)]
    pub prompts: Vec<Prompt>,
    pub meta: TranscriptMeta,
    /// `z.boolean().optional()` on the wire; the server-side `snapshot()`
    /// always emits a boolean (`Some(..)`), the cold rebuild omits the key
    /// (`None`) exactly like TS. `#[serde(default)]` parses a missing key
    /// as `None` (the `?? false` the TS reset path applies lives in the
    /// store's state, see apply.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_more_older: Option<bool>,
}
