//! Deserialization helpers enforcing zod optional semantics.
//!
//! zod's `z.string().optional()` accepts an absent key or a value, but
//! rejects JSON `null`. serde's default `Option<T>` accepts `null` as `None`;
//! [`strict_option`] closes that gap so a `null` field is a parse error,
//! exactly like zod.

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Field-level `deserialize_with` helper for tri-state optional fields:
/// absent → `None`, JSON `null` → `Some(None)`, value → `Some(Some(v))`.
///
/// Standard `Option<Option<T>>` cannot express the tri-state: `Option`'s own
/// impl swallows `null` as `None`. Used by `meta.merge` mode badges, where
/// `null` CLEARS the badge (schema.ts `modesMetaMergeSchema`).
pub fn nullable_optional<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(Some(None)),
        value => serde_json::from_value(value)
            .map(|t| Some(Some(t)))
            .map_err(D::Error::custom),
    }
}

/// Field-level `deserialize_with` helper for optional fields: rejects JSON
/// `null`, passes absent (via `#[serde(default)]`) and present values through.
///
/// Use together with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
pub fn strict_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    // Deliberately NOT `Option::<Value>::deserialize`: Option's own impl
    // treats JSON `null` as None, which would hide the null from us.
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Err(D::Error::custom(
            "null is not allowed for this field; omit it instead",
        )),
        value => serde_json::from_value(value)
            .map(Some)
            .map_err(D::Error::custom),
    }
}
