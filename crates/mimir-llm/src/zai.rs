//! Z.ai GLM provider (OpenAI-compatible API).
//!
//! Uses the Coding Plan endpoint for better tool-calling capabilities.

use crate::{
    openai_compat::{OpenAIMessage, OpenAIRequest, OpenAIResponse, OpenAITool, OpenAIFunction},
    ChunkType, Completion, CompletionRequest, CompletionStream, LlmError,
    LlmProvider, LlmResult, StreamChunk, ToolUseChunk,
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;

/// Coding Plan endpoint (better for tool-calling).
const ZAI_CODING_URL: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";

/// General endpoint.
const ZAI_GENERAL_URL: &str = "https://api.z.ai/api/paas/v4/chat/completions";

/// Z.ai GLM provider (uses OpenAI-compatible API).
pub struct ZaiProvider {
    client: Client,
    config: crate::ProviderConfig,
    use_coding_endpoint: bool,
}

impl ZaiProvider {
    /// Create a new Z.ai provider.
    pub fn new(config: crate::ProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config,
            use_coding_endpoint: true, // Default to coding endpoint
        }
    }

    /// Create from API key.
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(crate::ProviderConfig::new(api_key))
    }

    /// Create from environment variable.
    pub fn from_env() -> LlmResult<Self> {
        let api_key = std::env::var("ZAI_API_KEY")
            .map_err(|_| LlmError::NotConfigured("ZAI_API_KEY not set".to_string()))?;
        Ok(Self::from_api_key(api_key))
    }

    /// Use the general endpoint instead of coding endpoint.
    pub fn with_general_endpoint(mut self) -> Self {
        self.use_coding_endpoint = false;
        self
    }

    fn base_url(&self) -> &str {
        if let Some(url) = &self.config.base_url {
            return url;
        }
        if self.use_coding_endpoint {
            ZAI_CODING_URL
        } else {
            ZAI_GENERAL_URL
        }
    }

    fn build_request(&self, request: &CompletionRequest) -> OpenAIRequest {
        let mut messages: Vec<OpenAIMessage> = vec![];

        // Add system message if present
        if let Some(system) = &request.system {
            messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: Some(system.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Convert messages
        for msg in &request.messages {
            messages.push(OpenAIMessage::from_message(msg));
        }

        // Convert tools to OpenAI format
        let tools: Option<Vec<OpenAITool>> = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| OpenAITool {
                        r#type: "function".to_string(),
                        function: OpenAIFunction {
                            name: t.name.clone(),
                            description: Some(t.description.clone()),
                            parameters: serde_json::to_value(&t.parameters).unwrap_or(Value::Object(Default::default())),
                        },
                    })
                    .collect(),
            )
        };

        OpenAIRequest {
            model: request.model.clone(),
            messages,
            max_tokens: Some(request.max_tokens),
            temperature: request.temperature,
            tools,
            stop: if request.stop_sequences.is_empty() {
                None
            } else {
                Some(request.stop_sequences.clone())
            },
            stream: false,
        }
    }
}

#[async_trait]
impl LlmProvider for ZaiProvider {
    fn name(&self) -> &str {
        "zai"
    }

    fn models(&self) -> Vec<&str> {
        vec!["GLM-4.6", "GLM-4.5", "GLM-4.5-flash"]
    }

    fn default_model(&self) -> &str {
        self.config.default_model.as_deref().unwrap_or("GLM-4.6")
    }

    fn is_ready(&self) -> bool {
        !self.config.api_key.is_empty()
    }

    async fn complete(&self, request: CompletionRequest) -> LlmResult<Completion> {
        let api_request = self.build_request(&request);

        // Debug: dump first request to file
        if std::env::var("MIMIR_DEBUG_REQUEST").is_ok() {
            if let Ok(json) = serde_json::to_string_pretty(&api_request) {
                let _ = std::fs::write("/tmp/mimir_request.json", &json);
                eprintln!("[DEBUG] Request written to /tmp/mimir_request.json ({} bytes)", json.len());
            }
        }

        let response = self
            .client
            .post(self.base_url())
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
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

        let api_response: OpenAIResponse = response.json().await?;
        Ok(api_response.into_completion())
    }

    async fn stream(&self, request: CompletionRequest) -> LlmResult<CompletionStream> {
        let mut api_request = self.build_request(&request);
        api_request.stream = true;

        let response = self
            .client
            .post(self.base_url())
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
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
    let text =
        std::str::from_utf8(bytes).map_err(|e| LlmError::Stream(e.to_string()))?;

    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                return Ok(StreamChunk {
                    chunk_type: ChunkType::MessageStop,
                    text: None,
                    tool_use: None,
                });
            }

            if let Ok(event) = serde_json::from_str::<Value>(data) {
                if let Some(choices) = event["choices"].as_array() {
                    if let Some(choice) = choices.first() {
                        let delta = &choice["delta"];

                        // Text content
                        if let Some(content) = delta["content"].as_str() {
                            return Ok(StreamChunk {
                                chunk_type: ChunkType::ContentBlockDelta,
                                text: Some(content.to_string()),
                                tool_use: None,
                            });
                        }

                        // Tool calls
                        if let Some(tool_calls) = delta["tool_calls"].as_array() {
                            if let Some(tool_call) = tool_calls.first() {
                                let id = tool_call["id"].as_str().map(String::from);
                                let name = tool_call["function"]["name"]
                                    .as_str()
                                    .map(String::from);
                                let args = tool_call["function"]["arguments"]
                                    .as_str()
                                    .map(String::from);

                                if id.is_some() || name.is_some() {
                                    return Ok(StreamChunk {
                                        chunk_type: ChunkType::ToolUseStart,
                                        text: None,
                                        tool_use: Some(ToolUseChunk {
                                            id,
                                            name,
                                            input_delta: args,
                                        }),
                                    });
                                } else if args.is_some() {
                                    return Ok(StreamChunk {
                                        chunk_type: ChunkType::ToolUseInput,
                                        text: None,
                                        tool_use: Some(ToolUseChunk {
                                            id: None,
                                            name: None,
                                            input_delta: args,
                                        }),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(StreamChunk {
        chunk_type: ChunkType::ContentBlockDelta,
        text: Some(String::new()),
        tool_use: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    #[test]
    fn test_provider_creation() {
        let provider = ZaiProvider::from_api_key("test-key");
        assert_eq!(provider.name(), "zai");
        assert!(provider.is_ready());
    }

    #[test]
    fn test_default_model() {
        let provider = ZaiProvider::from_api_key("test-key");
        assert_eq!(provider.default_model(), "GLM-4.6");
    }

    #[test]
    fn test_coding_endpoint() {
        let provider = ZaiProvider::from_api_key("test-key");
        assert!(provider.base_url().contains("coding"));
    }

    #[test]
    fn test_general_endpoint() {
        let provider = ZaiProvider::from_api_key("test-key").with_general_endpoint();
        assert!(!provider.base_url().contains("coding"));
    }

    #[test]
    fn test_build_request() {
        let provider = ZaiProvider::from_api_key("test-key");
        let request = CompletionRequest::new("GLM-4.6", vec![Message::user("Hello")])
            .with_system("Be helpful")
            .with_max_tokens(1024);

        let api_request = provider.build_request(&request);
        assert_eq!(api_request.model, "GLM-4.6");
        assert_eq!(api_request.max_tokens, Some(1024));
        assert_eq!(api_request.messages.len(), 2); // system + user
    }
}
