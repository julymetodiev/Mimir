//! Anthropic Claude API provider.

use crate::{
    ChunkType, Completion, CompletionRequest, CompletionStream, ContentBlock, LlmError,
    LlmProvider, LlmResult, Message, ProviderConfig, Role, StopReason, StreamChunk,
    ToolUseChunk, Usage,
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Claude provider.
pub struct AnthropicProvider {
    client: Client,
    config: ProviderConfig,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider.
    pub fn new(config: ProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Create from API key.
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(ProviderConfig::new(api_key))
    }

    /// Create from environment variable.
    pub fn from_env() -> LlmResult<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| LlmError::NotConfigured("ANTHROPIC_API_KEY not set".to_string()))?;
        Ok(Self::from_api_key(api_key))
    }

    fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or(ANTHROPIC_API_URL)
    }

    fn build_request(&self, request: &CompletionRequest) -> AnthropicRequest {
        let messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .map(AnthropicMessage::from_message)
            .collect();

        let tools: Option<Vec<Value>> = if request.tools.is_empty() {
            None
        } else {
            Some(request.tools.iter().map(|t| t.to_anthropic_format()).collect())
        };

        AnthropicRequest {
            model: request.model.clone(),
            messages,
            system: request.system.clone(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            tools,
            stop_sequences: if request.stop_sequences.is_empty() {
                None
            } else {
                Some(request.stop_sequences.clone())
            },
            stream: false,
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn models(&self) -> Vec<&str> {
        vec![
            "claude-sonnet-4-20250514",
            "claude-opus-4-5-20251101",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
            "claude-3-opus-20240229",
        ]
    }

    fn default_model(&self) -> &str {
        self.config
            .default_model
            .as_deref()
            .unwrap_or("claude-sonnet-4-20250514")
    }

    fn is_ready(&self) -> bool {
        !self.config.api_key.is_empty()
    }

    async fn complete(&self, request: CompletionRequest) -> LlmResult<Completion> {
        let api_request = self.build_request(&request);

        let response = self
            .client
            .post(self.base_url())
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();

            if status.as_u16() == 429 {
                return Err(LlmError::RateLimited { retry_after: None });
            }
            if status.as_u16() == 401 {
                return Err(LlmError::Auth(error_body));
            }

            return Err(LlmError::Api {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let api_response: AnthropicResponse = response.json().await?;
        Ok(api_response.into_completion())
    }

    async fn stream(&self, request: CompletionRequest) -> LlmResult<CompletionStream> {
        let mut api_request = self.build_request(&request);
        api_request.stream = true;

        let response = self
            .client
            .post(self.base_url())
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let byte_stream = response.bytes_stream();
        let stream = byte_stream.map(|result| {
            result
                .map_err(LlmError::from)
                .and_then(|bytes| parse_sse_chunk(&bytes))
        });

        Ok(Box::pin(stream))
    }
}

fn parse_sse_chunk(bytes: &[u8]) -> LlmResult<StreamChunk> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| LlmError::Stream(e.to_string()))?;

    // Parse SSE format
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                return Ok(StreamChunk {
                    chunk_type: ChunkType::MessageStop,
                    text: None,
                    tool_use: None,
                });
            }

            let event: Value = serde_json::from_str(data)?;
            let event_type = event["type"].as_str().unwrap_or("");

            match event_type {
                "message_start" => {
                    return Ok(StreamChunk {
                        chunk_type: ChunkType::MessageStart,
                        text: None,
                        tool_use: None,
                    });
                }
                "content_block_delta" => {
                    let delta = &event["delta"];
                    if let Some(text) = delta["text"].as_str() {
                        return Ok(StreamChunk {
                            chunk_type: ChunkType::ContentBlockDelta,
                            text: Some(text.to_string()),
                            tool_use: None,
                        });
                    }
                    if let Some(input) = delta["partial_json"].as_str() {
                        return Ok(StreamChunk {
                            chunk_type: ChunkType::ToolUseInput,
                            text: None,
                            tool_use: Some(ToolUseChunk {
                                id: None,
                                name: None,
                                input_delta: Some(input.to_string()),
                            }),
                        });
                    }
                }
                "content_block_start" => {
                    let content_block = &event["content_block"];
                    if content_block["type"] == "tool_use" {
                        return Ok(StreamChunk {
                            chunk_type: ChunkType::ToolUseStart,
                            text: None,
                            tool_use: Some(ToolUseChunk {
                                id: content_block["id"].as_str().map(String::from),
                                name: content_block["name"].as_str().map(String::from),
                                input_delta: None,
                            }),
                        });
                    }
                }
                "message_stop" => {
                    return Ok(StreamChunk {
                        chunk_type: ChunkType::MessageStop,
                        text: None,
                        tool_use: None,
                    });
                }
                _ => {}
            }
        }
    }

    // No meaningful data in this chunk
    Ok(StreamChunk {
        chunk_type: ChunkType::ContentBlockDelta,
        text: Some(String::new()),
        tool_use: None,
    })
}

// Anthropic API types

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContent>,
}

impl AnthropicMessage {
    fn from_message(msg: &Message) -> Self {
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "user", // System handled separately
        };

        let content: Vec<AnthropicContent> = msg
            .content
            .iter()
            .map(|c| match c {
                ContentBlock::Text { text } => AnthropicContent::Text { text: text.clone() },
                ContentBlock::ToolUse { id, name, input } => AnthropicContent::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                },
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => AnthropicContent::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                },
            })
            .collect();

        Self {
            role: role.to_string(),
            content,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContent {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: String,
    stop_reason: String,
    usage: AnthropicUsage,
}

impl AnthropicResponse {
    fn into_completion(self) -> Completion {
        let content: Vec<ContentBlock> = self
            .content
            .into_iter()
            .map(|c| match c {
                AnthropicContent::Text { text } => ContentBlock::Text { text },
                AnthropicContent::ToolUse { id, name, input } => {
                    ContentBlock::ToolUse { id, name, input }
                }
                AnthropicContent::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                },
            })
            .collect();

        let stop_reason = match self.stop_reason.as_str() {
            "end_turn" => StopReason::EndTurn,
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            "stop_sequence" => StopReason::StopSequence,
            _ => StopReason::EndTurn,
        };

        Completion {
            message: Message {
                role: Role::Assistant,
                content,
            },
            stop_reason,
            usage: Usage {
                input_tokens: self.usage.input_tokens,
                output_tokens: self.usage.output_tokens,
                cache_creation_input_tokens: self.usage.cache_creation_input_tokens.unwrap_or(0),
                cache_read_input_tokens: self.usage.cache_read_input_tokens.unwrap_or(0),
            },
            model: self.model,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    cache_creation_input_tokens: Option<u32>,
    cache_read_input_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creation() {
        let provider = AnthropicProvider::from_api_key("test-key");
        assert_eq!(provider.name(), "anthropic");
        assert!(provider.is_ready());
    }

    #[test]
    fn test_default_model() {
        let provider = AnthropicProvider::from_api_key("test-key");
        assert!(provider.default_model().contains("claude"));
    }

    #[test]
    fn test_build_request() {
        let provider = AnthropicProvider::from_api_key("test-key");
        let request = CompletionRequest::new("claude-3-opus", vec![Message::user("Hello")])
            .with_system("Be helpful")
            .with_max_tokens(1024);

        let api_request = provider.build_request(&request);
        assert_eq!(api_request.model, "claude-3-opus");
        assert_eq!(api_request.max_tokens, 1024);
        assert_eq!(api_request.system, Some("Be helpful".to_string()));
    }
}
