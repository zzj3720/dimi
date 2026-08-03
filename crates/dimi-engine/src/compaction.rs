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

/// `COMPACT_USER_MESSAGE_MAX_TOKENS` — recent user messages kept verbatim.
pub const COMPACT_USER_MESSAGE_MAX_TOKENS: u64 = 20_000;

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

/// `shouldCompact` — usedSize >= maxSize * triggerRatio (0.85).
pub fn should_compact(estimated_tokens: u64, max_context_tokens: u32) -> bool {
    max_context_tokens > 0
        && (estimated_tokens as f64) >= (max_context_tokens as f64) * COMPACTION_TRIGGER_RATIO
}

/// The user message carrying the compaction instruction.
pub fn compaction_instruction_message() -> LlmMessage {
    LlmMessage {
        role: "user".to_string(),
        content: Value::String(COMPACTION_INSTRUCTION.to_string()),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning: None,
    }
}

/// Compacted shape: the system message (if any) + recent user messages
/// within the verbatim budget + the summary message. Mirrors
/// `buildContextCompactionShape`'s tail selection (the engine drops the
/// head/elision nuance: every non-user message is folded into the summary).
/// Returns `(messages, tokens_after)`.
pub fn compacted_shape(messages: &[LlmMessage], summary: &str) -> (Vec<LlmMessage>, u64) {
    let mut kept: Vec<LlmMessage> = Vec::new();
    if let Some(system) = messages.iter().find(|m| m.role == "system") {
        kept.push(system.clone());
    }
    let mut budget = COMPACT_USER_MESSAGE_MAX_TOKENS;
    let mut recent: Vec<LlmMessage> = Vec::new();
    for message in messages.iter().rev() {
        if message.role != "user" {
            continue;
        }
        let tokens = crate::context::estimate_tokens_for_message(message);
        if tokens > budget {
            break;
        }
        budget -= tokens;
        recent.push(message.clone());
    }
    recent.reverse();
    kept.extend(recent);
    let summary_text = format!("{COMPACTION_SUMMARY_PREFIX}\n{}", summary.trim());
    kept.push(LlmMessage {
        role: "user".to_string(),
        content: Value::String(summary_text),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning: None,
    });
    let tokens_after = crate::context::estimate_tokens_for_messages(&kept);
    (kept, tokens_after)
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
    fn trigger_ratio_gates_compaction() {
        assert!(!should_compact(0, 100_000));
        assert!(!should_compact(84_999, 100_000));
        assert!(should_compact(85_000, 100_000));
        assert!(!should_compact(1_000_000, 0)); // unknown window
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
    fn compacted_shape_truncates_user_budget() {
        let mut messages = Vec::new();
        for _ in 0..20 {
            messages.push(text_message("user", &"x".repeat(10_000)));
        }
        let (shape, _) = compacted_shape(&messages, "sum");
        // Each "x"*10000 message is 2501 tokens (2500 content + 1 role); the
        // 20k verbatim budget fits 7, the 8th breaks the loop.
        let kept_users = shape
            .iter()
            .take(shape.len() - 1)
            .filter(|m| m.role == "user")
            .count();
        assert_eq!(kept_users, 7);
        assert_eq!(shape.last().unwrap().role, "user"); // summary message
    }
}
