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
use aimux_core::error::AiMuxError;

/// Classify an aimux failure into the engine's provider error vocabulary
/// (TS `isRetryableGenerateError` parity): transient provider failures
/// (rate limit / connection / timeout / 5xx) are marked retryable and carry
/// their provider error code so the engine can retry the step; hard errors
/// (auth, model-not-found, …) are not.
fn classify_aimux_error(error: &AiMuxError) -> (Option<String>, Option<u64>, bool) {
    match error {
        AiMuxError::RateLimited { retry_after_ms } => (
            Some("provider.rate_limit".to_string()),
            Some(*retry_after_ms),
            true,
        ),
        AiMuxError::Auth(_) => (Some("provider.auth_error".to_string()), None, false),
        AiMuxError::ModelNotFound(_) | AiMuxError::NoSuchModel(_) => {
            (Some("model.not_found".to_string()), None, false)
        }
        AiMuxError::Http(_) | AiMuxError::ApiCall(_) => match error.status_code() {
            Some(413) => (Some("CONTEXT_OVERFLOW".to_string()), None, false),
            Some(408) => (Some("provider.timeout".to_string()), None, true),
            Some(429) => (
                Some("provider.rate_limit".to_string()),
                error.retry_after_hint().map(|ms| ms.max(0) as u64),
                true,
            ),
            Some(500..=599) => (Some("provider.api_error".to_string()), None, true),
            // No status: treat aimux's own retryability verdict (Http/ApiCall
            // are transient) as authoritative.
            _ => (Some("provider.api_error".to_string()), None, error.is_retryable()),
        },
        AiMuxError::Stream(message) | AiMuxError::Provider(message) | AiMuxError::Other(message) => {
            if looks_like_connection_error(message) {
                (Some("provider.connection_error".to_string()), None, true)
            } else {
                (None, None, error.is_retryable())
            }
        }
        _ => (None, None, error.is_retryable()),
    }
}

/// Message-text heuristic for transport-level failures (fetch failed,
/// connection reset, network unreachable, …).
fn looks_like_connection_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    ["connection", "connect", "network", "fetch failed", "dns", "econn", "eai_"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Build the engine `LlmError` from an aimux failure.
fn llm_error_from_aimux(error: &AiMuxError) -> LlmError {
    let (code, retry_after_ms, retryable) = classify_aimux_error(error);
    LlmError {
        message: error.to_string(),
        code,
        retry_after_ms,
        retryable,
    }
}

/// Preserve aimux's four input-token buckets at the engine boundary. The
/// unified total already includes cache reads and writes; the TS runner
/// later derives the ordinary-input bucket by subtracting both components.
fn aimux_usage_event(usage: &aimux_core::types::Usage) -> LlmStreamEvent {
    LlmStreamEvent::Usage {
        prompt_tokens: usage.input_tokens.total.map(u64::from),
        completion_tokens: usage.output_tokens.total.map(u64::from),
        total_tokens: None,
        prompt_tokens_details: Some(crate::llm::UsageDetails {
            cached_tokens: usage.input_tokens.cache_read.map(u64::from),
            cache_write_tokens: usage.input_tokens.cache_write.map(u64::from),
            reasoning_tokens: None,
        }),
        completion_tokens_details: Some(crate::llm::CompletionUsageDetails {
            reasoning_tokens: usage.output_tokens.reasoning.map(u64::from),
        }),
    }
}


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
            .map_err(|error| llm_error_from_aimux(&error))?;

        let mut stream = result.stream;
        let mut events: Vec<LlmStreamEvent> = Vec::new();
        let mut tool_calls: Vec<LlmToolCall> = Vec::new();
        let mut text = String::new();
        let mut thinking = String::new();

        while let Some(part) = stream.next().await {
            let part = part.map_err(|error| llm_error_from_aimux(&error))?;
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
                    events.push(aimux_usage_event(&usage));
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
                    return Err(llm_error_from_aimux(&error));
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
                    } else if let Some(url) =
                        item.get("url").and_then(|v| v.as_str()).or_else(|| {
                            item.get("imageUrl")
                                .and_then(|v| v.get("url"))
                                .and_then(|v| v.as_str())
                        })
                    {
                        if let Some((media_type, bytes)) = parse_data_url(url) {
                            parts.push(ContentPart::Image {
                                image: bytes,
                                media_type,
                                provider_options: None,
                            });
                        }
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

/// Construct the production aimux model for an OpenAI-compatible provider
/// (base URL + API key + model id from the engine input).
pub fn openai_model(config: &crate::types::ProviderConfig) -> Box<dyn LanguageModel> {
    let aimux_config = aimux_providers::OpenAIConfig {
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        ..aimux_providers::OpenAIConfig::new("")
    };
    let provider = aimux_providers::OpenAIProvider::new(aimux_config);
    Box::new(provider.model(&config.model))
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

/// Parse a `data:<media_type>;base64,<data>` URL into (mediaType, bytes).
fn parse_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let media_type = meta
        .split(';')
        .next()
        .unwrap_or("application/octet-stream")
        .to_string();
    if meta.contains(";base64") {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()
            .map(|bytes| (media_type, bytes))
    } else {
        Some((media_type, data.as_bytes().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::aimux_usage_event;
    use crate::llm::{CompletionUsageDetails, LlmStreamEvent, UsageDetails};
    use aimux_core::types::{TokenUsage, Usage};

    #[test]
    fn usage_event_preserves_cache_read_and_write() {
        let event = aimux_usage_event(&Usage {
            input_tokens: TokenUsage {
                total: Some(100),
                cache_read: Some(20),
                cache_write: Some(7),
                ..Default::default()
            },
            output_tokens: TokenUsage {
                total: Some(5),
                ..Default::default()
            },
            raw: None,
        });

        assert_eq!(
            event,
            LlmStreamEvent::Usage {
                prompt_tokens: Some(100),
                completion_tokens: Some(5),
                total_tokens: None,
                prompt_tokens_details: Some(UsageDetails {
                    cached_tokens: Some(20),
                    cache_write_tokens: Some(7),
                    reasoning_tokens: None,
                }),
                completion_tokens_details: Some(CompletionUsageDetails {
                    reasoning_tokens: None,
                }),
            }
        );
    }
}
