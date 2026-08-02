//! Context utilities (slice 3a) — token estimation mirroring
//! `llmProtocol/tokens.ts` and window projection helpers.
//!
//! Estimates are character-based heuristics (ASCII ≈ 4 chars/token,
//! non-ASCII 1 token/char, media parts a flat 2000) — they size context
//! windows and compaction budgets, never billing.

use serde_json::Value;

use crate::types::LlmMessage;

/// Flat token estimate for media parts (MEDIA_TOKEN_ESTIMATE).
pub const MEDIA_TOKEN_ESTIMATE: u64 = 2000;

/// `estimateTokens(text)` — ASCII ≈ ceil(chars/4), non-ASCII 1 token/char.
pub fn estimate_tokens(text: &str) -> u64 {
    let mut ascii = 0u64;
    let mut non_ascii = 0u64;
    for ch in text.chars() {
        if (ch as u32) <= 127 {
            ascii += 1;
        } else {
            non_ascii += 1;
        }
    }
    ascii.div_ceil(4) + non_ascii
}

/// `estimateTokensForContentPart` — text/think count their text; media
/// parts (image/file/…) are the flat estimate.
fn estimate_content_value(content: &Value) -> u64 {
    match content {
        Value::String(text) => estimate_tokens(text),
        Value::Array(items) => items
            .iter()
            .map(|item| match item.get("text").and_then(|v| v.as_str()) {
                Some(text) => estimate_tokens(text),
                None => MEDIA_TOKEN_ESTIMATE,
            })
            .sum(),
        Value::Null => 0,
        other => estimate_tokens(&other.to_string()),
    }
}

/// `estimateTokensForMessage` — role + content + tool calls.
pub fn estimate_tokens_for_message(message: &LlmMessage) -> u64 {
    let mut total = estimate_tokens(&message.role);
    total += estimate_content_value(&message.content);
    if let Some(tool_calls) = &message.tool_calls {
        for call in tool_calls {
            total += estimate_tokens(&call.function.name);
            total += estimate_tokens(&call.function.arguments);
        }
    }
    total
}

/// `estimateTokensForMessages`.
pub fn estimate_tokens_for_messages(messages: &[LlmMessage]) -> u64 {
    messages.iter().map(estimate_tokens_for_message).sum()
}

/// Window projection: keep the system message (first message with role
/// system) plus the most recent `window` messages. Mirrors the projector's
/// tail-window behavior for slice 3 (full projector semantics — memory
/// pinning, compaction anchors — land with the engine-owned context).
pub fn project_window(messages: &[LlmMessage], window: usize) -> Vec<LlmMessage> {
    if messages.len() <= window {
        return messages.to_vec();
    }
    let mut projected = Vec::with_capacity(window + 1);
    if let Some(system) = messages.iter().find(|m| m.role == "system") {
        projected.push(system.clone());
    }
    let tail_start = messages.len() - window;
    projected.extend_from_slice(&messages[tail_start..]);
    projected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_message(role: &str, text: &str) -> LlmMessage {
        LlmMessage {
            role: role.to_string(),
            content: Value::String(text.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning: None,
        }
    }

    #[test]
    fn ascii_estimation_is_ceil_chars_over_4() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"a".repeat(400)), 100);
    }

    #[test]
    fn non_ascii_counts_per_char() {
        assert_eq!(estimate_tokens("中文"), 2);
        assert_eq!(estimate_tokens("héllo"), 2); // h,e,l,l,o ascii(4) + é(1)
    }

    #[test]
    fn message_includes_role_and_tool_calls() {
        let message = LlmMessage {
            role: "assistant".to_string(),
            content: Value::String("hi".to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![crate::types::LlmToolCall {
                id: "c1".to_string(),
                call_type: Some("function".to_string()),
                function: crate::types::LlmToolCallFunction {
                    name: "Bash".to_string(),
                    arguments: "{\"command\":\"ls\"}".to_string(),
                },
            }]),
            reasoning: None,
        };
        let expected = estimate_tokens("assistant") + estimate_tokens("hi") + estimate_tokens("Bash") + estimate_tokens("{\"command\":\"ls\"}");
        assert_eq!(estimate_tokens_for_message(&message), expected);
    }

    #[test]
    fn media_parts_use_flat_estimate() {
        let message = LlmMessage {
            role: "user".to_string(),
            content: Value::Array(vec![
                serde_json::json!({ "type": "image", "image": "base64..." }),
            ]),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning: None,
        };
        assert_eq!(estimate_tokens_for_message(&message), estimate_tokens("user") + MEDIA_TOKEN_ESTIMATE);
    }

    #[test]
    fn window_keeps_system_and_tail() {
        let messages = vec![
            text_message("system", "sys"),
            text_message("user", "u1"),
            text_message("assistant", "a1"),
            text_message("user", "u2"),
            text_message("assistant", "a2"),
        ];
        let projected = project_window(&messages, 3);
        assert_eq!(projected.len(), 4); // system + 3 tail
        assert_eq!(projected[0].content, Value::String("sys".to_string()));
        assert_eq!(projected[1].content, Value::String("a1".to_string()));
        assert_eq!(projected[3].content, Value::String("a2".to_string()));
    }
}
