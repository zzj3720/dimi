//! `AimuxLlmClient` — the production LLM effect boundary, backed by aimux
//! (the unified LLM access layer: one `LanguageModel` trait over 172+
//! providers). The engine talks to models exclusively through this adapter
//! in production; the scripted client drives the differential tests.

use aimux_core::language_model::LanguageModel;
use aimux_core::language_model_message::{LanguageModelPrompt, LanguageModelPromptMessage};
use aimux_core::options::CallOptions;
use aimux_core::prelude::ContentPart;
use aimux_core::stream_part::StreamPart;
use aimux_core::types::{FinishReasonUnified, ReasoningEffort};
use futures::StreamExt;

use crate::llm::{AssistantTurn, ChatRequest, LlmClient, LlmError, LlmStreamEvent, StreamedTurn};
use crate::types::{LlmMessage, LlmToolCall};

/// Production LLM client: wraps one aimux `LanguageModel`.
pub struct AimuxLlmClient {
    pub model: Box<dyn LanguageModel>,
}

#[async_trait::async_trait]
impl LlmClient for AimuxLlmClient {
    async fn stream_chat(&self, request: &ChatRequest) -> Result<StreamedTurn, LlmError> {
        let prompt = convert_messages(&request.messages);
        let mut options = CallOptions::new(prompt);
        options.reasoning = request
            .thinking_effort
            .as_deref()
            .map(parse_reasoning_effort);
        if let Some(tools) = &request.tools {
            let parsed: Vec<aimux_core::tool::Tool> = tools
                .iter()
                .filter_map(|value| serde_json::from_value(value.clone()).ok())
                .collect();
            if !parsed.is_empty() {
                options.tools = Some(parsed);
            }
        }

        let result = self
            .model
            .do_stream(&options)
            .await
            .map_err(|error| LlmError {
                message: error.to_string(),
                code: None,
            })?;

        let mut stream = result.stream;
        let mut events: Vec<LlmStreamEvent> = Vec::new();
        let mut tool_calls: Vec<LlmToolCall> = Vec::new();
        let mut text = String::new();
        let mut thinking = String::new();

        while let Some(part) = stream.next().await {
            let part = part.map_err(|error| LlmError {
                message: error.to_string(),
                code: None,
            })?;
            match part {
                StreamPart::TextDelta { delta, .. } => {
                    text.push_str(&delta);
                    events.push(LlmStreamEvent::Text { delta });
                }
                StreamPart::ReasoningDelta { delta, .. } => {
                    thinking.push_str(&delta);
                    events.push(LlmStreamEvent::Thinking { delta });
                }
                StreamPart::ToolCall {
                    tool_call_id,
                    tool_name,
                    input,
                    ..
                } => {
                    tool_calls.push(LlmToolCall {
                        id: tool_call_id.clone(),
                        call_type: Some("function".to_string()),
                        function: crate::types::LlmToolCallFunction {
                            name: tool_name.clone(),
                            arguments: input.to_string(),
                        },
                    });
                    events.push(LlmStreamEvent::ToolCall {
                        tool_call_id,
                        name: Some(tool_name),
                        arguments_part: None,
                    });
                }
                StreamPart::Finish {
                    finish_reason,
                    usage,
                    ..
                } => {
                    let (prompt_tokens, cached_tokens, reasoning_tokens) = (
                        usage.input_tokens.total,
                        usage.input_tokens.cache_read,
                        usage.output_tokens.reasoning,
                    );
                    events.push(LlmStreamEvent::Usage {
                        prompt_tokens: prompt_tokens.map(u64::from),
                        completion_tokens: usage.output_tokens.total.map(u64::from),
                        total_tokens: None,
                        prompt_tokens_details: Some(crate::llm::UsageDetails {
                            cached_tokens: cached_tokens.map(u64::from),
                            reasoning_tokens: None,
                        }),
                        completion_tokens_details: Some(crate::llm::CompletionUsageDetails {
                            reasoning_tokens: reasoning_tokens.map(u64::from),
                        }),
                    });
                    events.push(LlmStreamEvent::Finish {
                        finish_reason: Some(
                            match finish_reason.unified {
                                FinishReasonUnified::Stop => "stop",
                                FinishReasonUnified::Length => "length",
                                FinishReasonUnified::ContentFilter => "content_filter",
                                FinishReasonUnified::ToolCalls => "tool_calls",
                                FinishReasonUnified::Error => "error",
                                FinishReasonUnified::Other => "stop",
                            }
                            .to_string(),
                        ),
                    });
                }
                StreamPart::Error { error } => {
                    let message = error.to_string();
                    events.push(LlmStreamEvent::Error {
                        message: message.clone(),
                    });
                    return Err(LlmError {
                        message,
                        code: None,
                    });
                }
                _ => {}
            }
        }

        Ok(StreamedTurn {
            events,
            assistant: AssistantTurn {
                tool_calls,
                text,
                thinking,
            },
        })
    }
}

/// Convert the engine's OpenAI-shaped messages into an aimux prompt.
fn convert_messages(messages: &[LlmMessage]) -> LanguageModelPrompt {
    let mut prompt = Vec::with_capacity(messages.len());
    for message in messages {
        let role = match message.role.as_str() {
            "system" => aimux_core::message::Role::System,
            "assistant" => aimux_core::message::Role::Assistant,
            "tool" => aimux_core::message::Role::Tool,
            _ => aimux_core::message::Role::User,
        };
        let mut parts: Vec<ContentPart> = Vec::new();
        match &message.content {
            serde_json::Value::String(text) if !text.is_empty() => {
                parts.push(ContentPart::text(text));
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        parts.push(ContentPart::text(text));
                    }
                }
            }
            _ => {}
        }
        if message.role == "tool" {
            if let Some(tool_call_id) = &message.tool_call_id {
                let mut result =
                    ContentPart::tool_result(tool_call_id.clone(), message.content.clone());
                if let Some(name) = &message.name {
                    if let ContentPart::ToolResult { tool_name, .. } = &mut result {
                        *tool_name = Some(name.clone());
                    }
                }
                parts.push(result);
            }
        }
        if message.role == "assistant" {
            if let Some(tool_calls) = &message.tool_calls {
                for call in tool_calls {
                    parts.push(ContentPart::tool_call(
                        call.id.clone(),
                        call.function.name.clone(),
                        serde_json::from_str(&call.function.arguments)
                            .unwrap_or(serde_json::Value::Null),
                    ));
                }
            }
        }
        if parts.is_empty() && message.role != "tool" {
            parts.push(ContentPart::text(""));
        }
        prompt.push(LanguageModelPromptMessage {
            role,
            content: parts,
            provider_options: None,
        });
    }
    prompt
}

fn parse_reasoning_effort(value: &str) -> ReasoningEffort {
    match value.to_ascii_lowercase().as_str() {
        "none" => ReasoningEffort::None,
        "minimal" => ReasoningEffort::Minimal,
        "low" => ReasoningEffort::Low,
        "medium" => ReasoningEffort::Medium,
        "high" => ReasoningEffort::High,
        _ => ReasoningEffort::ProviderDefault,
    }
}

/// Placeholder model for the napi surface until the real aimux provider
/// construction lands (slice-1 tail: LLM request implementation). Scripted
/// segments drive the differential suite in the meantime.
pub fn unimplemented_model() -> impl LanguageModel {
    struct Unimplemented;
    #[async_trait::async_trait]
    impl LanguageModel for Unimplemented {
        fn provider(&self) -> &str {
            "unimplemented"
        }
        fn model_id(&self) -> &str {
            "unimplemented"
        }
        async fn do_generate(
            &self,
            _options: &CallOptions,
        ) -> Result<aimux_core::result::GenerateResult, aimux_core::error::AiMuxError> {
            Err(aimux_core::error::AiMuxError::Provider(
                "aimux model not wired yet".to_string(),
            ))
        }
        async fn do_stream(
            &self,
            _options: &CallOptions,
        ) -> Result<aimux_core::result::StreamResult, aimux_core::error::AiMuxError> {
            Err(aimux_core::error::AiMuxError::Provider(
                "aimux model not wired yet".to_string(),
            ))
        }
    }
    Unimplemented
}
