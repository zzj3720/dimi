//! Full-history compaction (M3 leftover 4) — engine-side mirror of the TS
//! `fullCompaction` domain + `contextMemory/compactionHandoff`: when the
//! assembled request estimate crosses the model-window trigger ratio, the
//! engine runs an LLM summary round (the same instruction the TS service
//! uses) and replaces its working messages with the compacted shape — kept
//! recent user messages plus the summary message.
//!
//! The wire/transcript side of compaction (`context.apply_compaction`,
//! `full_compaction.*` ops) stays on the TS runner: it consumes the
//! `context.compacted` event and applies the same record through the TS
//! context memory, so live projections and cold rebuilds agree.

use serde_json::Value;

use crate::types::LlmMessage;

/// `compactionTriggerRatio` — DEFAULT_COMPACTION_CONFIG.triggerRatio.
pub const COMPACTION_TRIGGER_RATIO: f64 = 0.85;

/// Default reserved context space (`DEFAULT_COMPACTION_CONFIG`).
pub const RESERVED_CONTEXT_SIZE: u64 = 50_000;

/// `COMPACT_USER_MESSAGE_MAX_TOKENS` — recent user messages kept verbatim.
pub const COMPACT_USER_MESSAGE_MAX_TOKENS: u64 = 20_000;

/// Head budget used when the complete user-input history exceeds the
/// verbatim budget (`COMPACT_USER_MESSAGE_HEAD_TOKENS`).
pub const COMPACT_USER_MESSAGE_HEAD_TOKENS: u64 = 2_000;

/// Bounded shrink retries when the summary round returns empty (the TS
/// service retries up to MAX_COMPACTION_RETRY_ATTEMPTS with history
/// shrinking; the engine keeps a smaller bound for the same loop).
pub const COMPACTION_MAX_SHRINK_ATTEMPTS: usize = 3;

/// `COMPACTION_SUMMARY_PREFIX` — the summary message's lead-in
/// (compaction-summary-prefix.md).
pub const COMPACTION_SUMMARY_PREFIX: &str = "The conversation so far has been compacted to free up context. What follows is your own working summary of this task — use it to continue your train of thought rather than starting over. Treat it as notes, not proof: where it says a step was done, tests passed, or a fix worked, verify that yourself before relying on it. Any user messages earlier in this context are preserved verbatim from the compacted conversation; where a system-reminder note among them marks an omitted middle section, the user messages it replaced are covered by this summary.";

/// The compaction instruction (compaction-instruction.md, rendered with an
/// empty custom-instruction block — the engine has no user instruction).
pub const COMPACTION_INSTRUCTION: &str = r#"You are about to run out of context. Write a first-person handoff note to
yourself so you can seamlessly continue this task after the earlier
conversation is cleared.

--- This message is a direct task, not part of the above conversation ---

Write the note as your own continuing train of thought — first person, present
tense, the way you would reason through the next move. Do not write a
third-party report about someone else's work, and do not impose rigid section
headings; let the shape follow the task. Write the note in the same language the
conversation has been using — do not switch to English just because these
instructions happen to be in English.

Make the note self-sufficient: the next turn will see only your most recent user
messages and this note — every assistant message, tool call, and tool result
above will be gone. In your own words, preserve what you genuinely need to
continue:

- What the latest request is actually asking for: your reading of its intent and
  any ambiguity you have already resolved — not a re-transcription, since what
  fits is kept verbatim in your most recent messages. But those kept messages are
  size-capped, so a long request is truncated there: if the latest request is
  large (a big paste or file), preserve the parts at risk of being dropped —
  above all the actual ask. If several requests are in play, say which one governs
  the next move, and re-quote any still-relevant earlier request that may have
  scrolled out of the kept messages.
- The instructions and constraints currently in force (user preferences,
  project rules, environment and tooling limits) — condensed to what still
  matters, keeping decisions you have already settled (what you chose and why)
  separate from questions still open, so you neither silently reopen a closed
  choice nor treat an undecided point as decided.
- What has actually been done, at high fidelity: keep the exact commands that
  were run, the exact file paths touched, and whether each succeeded or failed —
  and the results themselves, not just the commands: the concrete values
  returned, the key lines or error text, the schema or signature a lookup
  revealed, since re-running to recover them may be slow or impossible. Keep only
  the final working version of any code; drop intermediate attempts and
  already-resolved errors.
- What you still don't know: context the next step depends on that this
  conversation never established — files or paths referenced but not yet read,
  schemas or APIs assumed but unseen, questions the user has not answered. Name
  these gaps so the next turn goes and checks them instead of assuming.
- The forward plan — and this is the moment to invest in it. Right now you
  hold more context on this task than you ever will again; the next turn
  resumes with less, so the plan you commit here is the one it will follow.
  Give the exact next command or tool call, but don't stop at the next step:
  set out the remaining sequence to finish, the decisions you have already
  made for those upcoming steps (so the next turn doesn't reopen them), the
  obstacles or edge cases you can already foresee and how you mean to handle
  them, and any work you can commit to now — the exact patch, query, or shape
  of the final answer you already know you will produce. Anything you settle
  here is one less thing the next turn must rediscover. Include any required
  format for the final answer.

Be honest about uncertainty. If an earlier step claimed something was done but
was never verified (tests "passing", a fix "working", a file "created"), say so
plainly and treat it as unverified rather than fact — re-check before relying
on it.

Be concise, and keep the note proportional to the task: a long multi-step task
warrants detail, but a trivial or nearly finished exchange needs only a sentence
or two — do not pad it out. Include the critical data, identifiers, and
references needed to continue, and omit anything that does not change the next
move.

Respond with text only. Do not call any tools — you already have everything you
need in the conversation history."#;

/// `shouldCompact` with the default TS compaction configuration.
pub fn should_compact(estimated_tokens: u64, max_context_tokens: u32) -> bool {
    should_compact_with_config(
        estimated_tokens,
        max_context_tokens,
        COMPACTION_TRIGGER_RATIO,
        RESERVED_CONTEXT_SIZE,
    )
}

/// `DefaultCompactionStrategy.shouldCompact` parity: the ratio threshold and
/// the reserved-space threshold are independent ways to start compaction.
pub fn should_compact_with_config(
    estimated_tokens: u64,
    max_context_tokens: u32,
    trigger_ratio: f64,
    reserved_context_size: u64,
) -> bool {
    let max = u64::from(max_context_tokens);
    if max == 0 {
        return false;
    }
    let ratio_trigger = (estimated_tokens as f64) >= (max as f64) * trigger_ratio;
    let reserved_trigger = reserved_context_size > 0
        && reserved_context_size < max
        && estimated_tokens >= max - reserved_context_size;
    ratio_trigger || reserved_trigger
}

/// The user message carrying the compaction instruction.
pub fn compaction_instruction_message() -> LlmMessage {
    LlmMessage {
        role: "user".to_string(),
        content: Value::String(COMPACTION_INSTRUCTION.to_string()),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        tools: None,
        reasoning: None,
        origin: None,
    }
}

/// Compacted shape: the system message (if any), real user input split into a
/// head and tail when it exceeds the verbatim budget, an elision reminder, and
/// the summary message. Non-user history is folded into the summary, matching
/// `buildContextCompactionShape`.
/// Returns `(messages, tokens_after)`.
pub fn compacted_shape(messages: &[LlmMessage], summary: &str) -> (Vec<LlmMessage>, u64) {
    let messages = strip_dynamic_tool_context(messages);
    let mut kept: Vec<LlmMessage> = Vec::new();
    if let Some(system) = messages.iter().find(|m| m.role == "system") {
        kept.push(system.clone());
    }

    let user_messages: Vec<LlmMessage> = messages
        .iter()
        .filter(|message| is_compactable_user_message(message))
        .cloned()
        .collect();
    let selection = select_user_messages(&user_messages);
    kept.extend(selection.head);
    if selection.elided {
        kept.push(compaction_elision_message(selection.omitted_tokens));
    }
    kept.extend(selection.tail);
    let summary_text = format!("{COMPACTION_SUMMARY_PREFIX}\n{}", summary.trim());
    kept.push(LlmMessage {
        role: "user".to_string(),
        content: Value::String(summary_text),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        tools: None,
        reasoning: None,
        origin: Some(serde_json::json!({ "kind": "compaction_summary" })),
    });
    let tokens_after = crate::context::estimate_tokens_for_messages(&kept);
    (kept, tokens_after)
}

/// Remove progressive tool-disclosure protocol messages from the history sent
/// to the compaction model. The TS `toolSelect` service keeps these messages in
/// context so the loaded-tool ledger can be rebuilt, but they are protocol
/// bookkeeping rather than conversation and must not consume summary input.
pub fn strip_dynamic_tool_context(messages: &[LlmMessage]) -> Vec<LlmMessage> {
    messages
        .iter()
        .filter_map(|message| {
            if is_loadable_tools_announcement(message) {
                return None;
            }
            if is_dynamic_tool_schema_message(message)
                && content_is_empty(&message.content)
                && message
                    .tool_calls
                    .as_ref()
                    .is_none_or(|tool_calls| tool_calls.is_empty())
            {
                return None;
            }
            if is_dynamic_tool_schema_message(message) {
                let mut message = message.clone();
                message.tools = None;
                return Some(message);
            }
            Some(message.clone())
        })
        .collect()
}

fn is_dynamic_tool_schema_message(message: &LlmMessage) -> bool {
    message
        .tools
        .as_ref()
        .is_some_and(|tools| !tools.is_empty())
}

fn is_loadable_tools_announcement(message: &LlmMessage) -> bool {
    let Some(origin) = message.origin.as_ref().and_then(Value::as_object) else {
        return false;
    };
    origin.get("kind").and_then(Value::as_str) == Some("system_trigger")
        && origin.get("name").and_then(Value::as_str) == Some("loadable-tools")
}

fn content_is_empty(content: &Value) -> bool {
    match content {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(parts) => parts.is_empty(),
        Value::Object(_) | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[derive(Debug, Default)]
struct UserMessageSelection {
    head: Vec<LlmMessage>,
    tail: Vec<LlmMessage>,
    elided: bool,
    omitted_tokens: u64,
}

fn is_compactable_user_message(message: &LlmMessage) -> bool {
    if message.role != "user" {
        return false;
    }
    let Some(origin) = message.origin.as_ref() else {
        return true;
    };
    match origin.get("kind").and_then(Value::as_str) {
        Some("user") => true,
        Some("skill_activation") | Some("plugin_command") => {
            origin.get("trigger").and_then(Value::as_str) == Some("user-slash")
        }
        Some(
            "compaction_summary" | "injection" | "shell_command" | "system_trigger" | "task"
            | "cron_job" | "cron_missed" | "hook_result" | "retry",
        )
        | None => false,
        Some(_) => false,
    }
}

fn select_user_messages(messages: &[LlmMessage]) -> UserMessageSelection {
    let total_tokens = crate::context::estimate_tokens_for_messages(messages);
    if total_tokens <= COMPACT_USER_MESSAGE_MAX_TOKENS {
        return UserMessageSelection {
            tail: messages.to_vec(),
            ..UserMessageSelection::default()
        };
    }

    let head_budget = COMPACT_USER_MESSAGE_HEAD_TOKENS;
    let tail_budget = COMPACT_USER_MESSAGE_MAX_TOKENS - head_budget;
    let mut tail = Vec::new();
    let mut remaining = tail_budget;
    let mut head_end = messages.len();
    let mut tail_boundary_dropped_prefix: Option<LlmMessage> = None;
    for index in (0..messages.len()).rev() {
        let message = &messages[index];
        let tokens = crate::context::estimate_tokens_for_message(message);
        if tokens > remaining {
            let full_text = extract_text(&message.content);
            let kept_suffix = truncate_text_from_end(&full_text, remaining);
            tail.push(replace_message_text(message, &kept_suffix));
            let dropped_prefix_len = full_text.len().saturating_sub(kept_suffix.len());
            if dropped_prefix_len > 0 {
                tail_boundary_dropped_prefix = Some(replace_message_text(
                    message,
                    &full_text[..dropped_prefix_len],
                ));
            }
            head_end = index;
            break;
        }
        tail.push(message.clone());
        remaining -= tokens;
    }
    tail.reverse();

    let mut head = Vec::new();
    remaining = head_budget;
    let mut head_candidates = messages[..head_end].to_vec();
    if let Some(message) = tail_boundary_dropped_prefix {
        head_candidates.push(message);
    }
    for message in &head_candidates {
        let tokens = crate::context::estimate_tokens_for_message(message);
        if tokens > remaining {
            head.push(truncate_user_message(message, remaining));
            break;
        }
        head.push(message.clone());
        remaining -= tokens;
    }

    let kept_tokens = crate::context::estimate_tokens_for_messages(&head)
        + crate::context::estimate_tokens_for_messages(&tail);
    UserMessageSelection {
        head,
        tail,
        elided: true,
        omitted_tokens: total_tokens.saturating_sub(kept_tokens),
    }
}

fn extract_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                let object = part.as_object()?;
                if object.get("type").and_then(Value::as_str) != Some("text") {
                    return None;
                }
                object.get("text").and_then(Value::as_str)
            })
            .collect(),
        _ => String::new(),
    }
}

fn truncate_user_message(message: &LlmMessage, max_tokens: u64) -> LlmMessage {
    let text = extract_text(&message.content);
    replace_message_text(message, &truncate_text(&text, max_tokens))
}

fn replace_message_text(message: &LlmMessage, text: &str) -> LlmMessage {
    let mut replaced = message.clone();
    replaced.content = Value::String(text.to_string());
    replaced.tool_calls = None;
    replaced
}

fn truncate_text(text: &str, max_tokens: u64) -> String {
    if max_tokens == 0 {
        return String::new();
    }
    let mut ascii_count = 0u64;
    let mut non_ascii_count = 0u64;
    let mut end = 0;
    for (index, character) in text.char_indices() {
        if character.is_ascii() {
            ascii_count += 1;
        } else {
            non_ascii_count += 1;
        }
        if ascii_count.div_ceil(4) + non_ascii_count > max_tokens {
            break;
        }
        end = index + character.len_utf8();
    }
    text[..end].to_string()
}

fn truncate_text_from_end(text: &str, max_tokens: u64) -> String {
    if max_tokens == 0 {
        return String::new();
    }
    let mut ascii_count = 0u64;
    let mut non_ascii_count = 0u64;
    let mut start = text.len();
    for (index, character) in text.char_indices().rev() {
        if character.is_ascii() {
            ascii_count += 1;
        } else {
            non_ascii_count += 1;
        }
        if ascii_count.div_ceil(4) + non_ascii_count > max_tokens {
            break;
        }
        start = index;
    }
    text[start..].to_string()
}

fn compaction_elision_message(omitted_tokens: u64) -> LlmMessage {
    LlmMessage {
        role: "user".to_string(),
        content: Value::String(format!(
            "<system-reminder>\nSome of this conversation's user messages were omitted here during compaction: the messages above this note are the oldest user input, the messages below are the most recent, and roughly {omitted_tokens} tokens in between were dropped. The omitted content is covered by the compaction summary at the end of the conversation.\n</system-reminder>"
        )),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        tools: None,
        reasoning: None,
        origin: Some(serde_json::json!({
            "kind": "injection",
            "variant": "compaction_elision"
        })),
    }
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
            tools: None,
            reasoning: None,
            origin: None,
        }
    }

    #[test]
    fn trigger_ratio_gates_compaction() {
        assert!(!should_compact_with_config(0, 100_000, 0.85, 0));
        assert!(!should_compact_with_config(84_999, 100_000, 0.85, 0));
        assert!(should_compact_with_config(85_000, 100_000, 0.85, 0));
        assert!(!should_compact_with_config(1_000_000, 0, 0.85, 0)); // unknown window
    }

    #[test]
    fn reserved_context_can_trigger_compaction_before_ratio() {
        assert!(should_compact_with_config(210_000, 256_000, 0.85, 50_000));
        assert!(!should_compact_with_config(200_000, 256_000, 0.85, 50_000));
        assert!(!should_compact_with_config(210_000, 256_000, 0.85, 256_000));
    }

    #[test]
    fn compacted_shape_keeps_system_recent_users_and_summary() {
        let messages = vec![
            text_message("system", "sys"),
            text_message("user", "u1"),
            text_message("assistant", "a1"),
            text_message("tool", "t1"),
            text_message("user", "u2"),
            text_message("assistant", "a2"),
        ];
        let (shape, tokens_after) = compacted_shape(&messages, "sum");
        // All user messages fit the verbatim budget: system + u1 + u2 +
        // summary (assistant/tool folds into the summary).
        assert_eq!(shape.len(), 4);
        assert_eq!(shape[0].content, Value::String("sys".to_string()));
        assert_eq!(shape[1].content, Value::String("u1".to_string()));
        assert_eq!(shape[2].content, Value::String("u2".to_string()));
        assert_eq!(
            shape[3].content,
            Value::String(format!("{COMPACTION_SUMMARY_PREFIX}\nsum"))
        );
        assert!(tokens_after > 0);
    }

    #[test]
    fn compacted_shape_uses_head_tail_message_budget() {
        let mut messages = Vec::new();
        for _ in 0..20 {
            messages.push(text_message("user", &"x".repeat(10_000)));
        }
        let (shape, _) = compacted_shape(&messages, "sum");
        // The engine keeps a truncated head boundary and a truncated tail
        // boundary within the 2k/18k budgets, with an explicit elision marker
        // between them.
        let kept_users = shape
            .iter()
            .take(shape.len() - 1)
            .filter(|m| m.role == "user")
            .count();
        assert_eq!(kept_users, 10); // head + elision + 8 tail (summary excluded above)
        assert_eq!(shape.len(), 11); // head + elision + tail + summary
        assert!(shape.iter().any(|message| {
            message.content.as_str().is_some_and(|text| {
                text.contains("Some of this conversation's user messages were omitted")
            })
        }));
        assert_eq!(shape.last().unwrap().role, "user"); // summary message
    }

    #[test]
    fn compacted_shape_drops_non_user_origins() {
        let mut dropped = text_message("user", "internal");
        dropped.origin = Some(serde_json::json!({
            "kind": "system_trigger",
            "name": "task-notification"
        }));
        let (shape, _) = compacted_shape(&[text_message("user", "kept"), dropped], "sum");
        let texts: Vec<&str> = shape
            .iter()
            .filter_map(|message| message.content.as_str())
            .collect();
        assert!(texts.contains(&"kept"));
        assert!(!texts.contains(&"internal"));
    }

    #[test]
    fn dynamic_tool_context_is_removed_from_summary_history() {
        let mut schema = text_message("system", "");
        schema.origin = Some(serde_json::json!({
            "kind": "injection",
            "variant": "dynamic_tool_schema"
        }));
        schema.tools = Some(vec![serde_json::json!({ "name": "McpTool" })]);
        let mut announcement = text_message("system", "<tools_added>\nMcpTool\n</tools_added>");
        announcement.origin = Some(serde_json::json!({
            "kind": "system_trigger",
            "name": "loadable-tools"
        }));

        let stripped = strip_dynamic_tool_context(&[
            schema,
            announcement,
            text_message("user", "keep this"),
        ]);
        assert_eq!(stripped.len(), 1);
        assert_eq!(stripped[0].content, Value::String("keep this".to_string()));
    }

    #[test]
    fn dynamic_tool_context_keeps_content_but_strips_tool_definitions() {
        let mut message = text_message("system", "loaded tool context");
        message.tools = Some(vec![serde_json::json!({ "name": "McpTool" })]);

        let stripped = strip_dynamic_tool_context(&[message]);

        assert_eq!(stripped.len(), 1);
        assert_eq!(
            stripped[0].content,
            Value::String("loaded tool context".to_string())
        );
        assert_eq!(stripped[0].tools, None);
    }

    #[test]
    fn dynamic_tool_origin_without_tools_is_kept() {
        let mut message = text_message("system", "");
        message.origin = Some(serde_json::json!({
            "kind": "injection",
            "variant": "dynamic_tool_schema"
        }));

        let stripped = strip_dynamic_tool_context(&[message]);

        assert_eq!(stripped.len(), 1);
        assert_eq!(stripped[0].content, Value::String(String::new()));
    }

    #[test]
    fn compacted_shape_truncates_a_user_message_at_both_boundaries() {
        let huge = "x".repeat(100_000);
        let (shape, _) = compacted_shape(
            &[
                text_message("user", &huge),
                text_message("user", "recent user input"),
            ],
            "sum",
        );
        let user_texts: Vec<&str> = shape
            .iter()
            .filter(|message| message.role == "user")
            .filter_map(|message| message.content.as_str())
            .filter(|text| *text != "recent user input")
            .filter(|text| !text.starts_with(COMPACTION_SUMMARY_PREFIX))
            .filter(|text| !text.starts_with("<system-reminder>"))
            .collect();

        assert_eq!(user_texts.len(), 2);
        assert!(user_texts.iter().any(|text| text.len() < huge.len() && text.starts_with('x')));
        assert!(user_texts.iter().any(|text| text.len() < huge.len() && text.ends_with('x')));
    }
}
