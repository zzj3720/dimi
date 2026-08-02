//! Wire record shapes consumed by the cold-rebuild path.
//!
//! The wire log (`wire.jsonl`) is written by the engine's `WireService`
//! (`packages/agent-core-v2/src/wire/record.ts`): each line is
//! `{ type, time?, ...payload }`. The transcript package deliberately never
//! defines a closed wire type (`HistoryWireRecord` is structurally open),
//! and neither do we beyond the two fields the rebuild path touches.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One line of `wire.jsonl` — `{ type, time?, ...rest }`. `rest` keeps the
/// open payload (`{ type, ...payload }` encoding; a non-object payload lands
/// under a single `payload` key, mirroring `wireRecordToPayload`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireRecord {
    pub r#type: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub time: Option<i64>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl WireRecord {
    /// The `payload` value when the record was encoded with a non-object
    /// payload (`{ type, payload }`), matching `wireRecordToPayload`.
    pub fn payload(&self) -> Option<&Value> {
        if self.rest.len() == 1 {
            self.rest.get("payload")
        } else {
            None
        }
    }

    /// All record fields except `type` and `time` — the payload object
    /// shape the fold step consumes (`payloadOf` in foldFacts).
    pub fn payload_fields(&self) -> &Map<String, Value> {
        &self.rest
    }
}

/// `WIRE_PROTOCOL_VERSION` (`packages/agent-core-v2/src/wire/migration/migration.ts:17`).
pub const WIRE_PROTOCOL_VERSION: &str = "1.5";

/// The first line of a sealed wire log: `{ type:'metadata',
/// protocol_version, created_at }` (`record.ts:22-26`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireMetadataRecord {
    pub r#type: String,
    pub protocol_version: String,
    pub created_at: i64,
}

/// One folded context message (`groupTurns` input shape, groupTurns.ts
/// 60–67). Content parts and origin stay open (`Value`) — the reducer only
/// reaches into them through helpers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMessage {
    /// `message.id` — kept for `isPromptOwnedInjection` matching; loop-folded
    /// assistant/tool messages have no id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub id: Option<String>,
    pub role: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub content: Option<Vec<Value>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub tool_call_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub is_error: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::de::strict_option"
    )]
    pub origin: Option<MessageOrigin>,
}

/// `HistoryMessage.toolCalls` entry (groupTurns.ts 60-67).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// `null` in TS (`arguments: string | null`) — must not be confused with
    /// `Option::None` (absent key), so this field is not `strict_option`.
    #[serde(default)]
    pub arguments: Option<String>,
}

/// `HistoryMessage.origin` — `{ kind: string }`, structurally open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageOrigin {
    #[serde(default)]
    pub kind: String,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}
