//! OpenRouter provider (OpenAI-compatible API).
//!
//! OpenRouter provides access to many models through a unified API.
//! See https://openrouter.ai/docs for available models.

use crate::{
    openai_compat::{OpenAIMessage, OpenAIRequest, OpenAIResponse, OpenAITool, OpenAIFunction},
    ChunkType, Completion, CompletionRequest, CompletionStream, LlmError,
    LlmProvider, LlmResult, StreamChunk, ToolUseChunk,
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;

/// OpenRouter API endpoint.
const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// OpenRouter provider.
pub struct OpenRouterProvider {
    client: Client,
    config: crate::ProviderConfig,
}

impl OpenRouterProvider {
    /// Create a new OpenRouter provider.
    pub fn new(config: crate::ProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Create from API key.
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(crate::ProviderConfig::new(api_key))
    }

    /// Create from environment variable.
    pub fn from_env() -> LlmResult<Self> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| LlmError::NotConfigured("OPENROUTER_API_KEY not set".to_string()))?;
        Ok(Self::from_api_key(api_key))
    }

    /// Set a custom base URL.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.config.base_url = Some(url.into());
        self
    }

    /// Set a default model.
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.config.default_model = Some(model.into());
        self
    }

    fn base_url(&self) -> &str {
        self.config.base_url.as_deref().unwrap_or(OPENROUTER_URL)
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
impl LlmProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn models(&self) -> Vec<&str> {
        // Popular models available on OpenRouter
        vec![
            "anthropic/claude-3.5-sonnet",
            "anthropic/claude-3-opus",
            "openai/gpt-4-turbo",
            "openai/gpt-4o",
            "google/gemini-pro-1.5",
            "meta-llama/llama-3.1-405b-instruct",
            "mistralai/mistral-large",
            "deepseek/deepseek-chat",
        ]
    }

    fn default_model(&self) -> &str {
        self.config
            .default_model
            .as_deref()
            .unwrap_or("anthropic/claude-3.5-sonnet")
    }

    fn is_ready(&self) -> bool {
        !self.config.api_key.is_empty()
    }

    async fn complete(&self, request: CompletionRequest) -> LlmResult<Completion> {
        let api_request = self.build_request(&request);

        let response = self
            .client
            .post(self.base_url())
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/JuliusML/Mimir")
            .header("X-Title", "Mimir CLI")
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
            .header("HTTP-Referer", "https://github.com/JuliusML/Mimir")
            .header("X-Title", "Mimir CLI")
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
    let text = std::str::from_utf8(bytes).map_err(|e| LlmError::Stream(e.to_string()))?;

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
        let provider = OpenRouterProvider::from_api_key("test-key");
        assert_eq!(provider.name(), "openrouter");
        assert!(provider.is_ready());
    }

    #[test]
    fn test_default_model() {
        let provider = OpenRouterProvider::from_api_key("test-key");
        assert_eq!(provider.default_model(), "anthropic/claude-3.5-sonnet");
    }

    #[test]
    fn test_custom_model() {
        let provider = OpenRouterProvider::from_api_key("test-key")
            .with_default_model("openai/gpt-4o");
        assert_eq!(provider.default_model(), "openai/gpt-4o");
    }

    #[test]
    fn test_base_url() {
        let provider = OpenRouterProvider::from_api_key("test-key");
        assert_eq!(provider.base_url(), OPENROUTER_URL);
    }

    #[test]
    fn test_custom_base_url() {
        let provider = OpenRouterProvider::from_api_key("test-key")
            .with_base_url("https://custom.endpoint.com/v1");
        assert_eq!(provider.base_url(), "https://custom.endpoint.com/v1");
    }

    #[tokio::test]
    #[ignore] // Run with: cargo test -p mimir-llm openrouter_real_api -- --ignored
    async fn test_openrouter_real_api() {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .expect("OPENROUTER_API_KEY must be set for this test");

        let provider = OpenRouterProvider::from_api_key(api_key)
            .with_default_model("qwen/qwen3-coder");

        let request = crate::CompletionRequest::new(
            "qwen/qwen3-coder",
            vec![Message::user("What is 2 + 2? Answer with just the number.")],
        )
        .with_max_tokens(100);

        let result = provider.complete(request).await;

        match result {
            Ok(completion) => {
                println!("Model: {}", completion.model);
                println!("Response: {:?}", completion.message.content);
                println!("Usage: {:?}", completion.usage);
                assert!(!completion.message.content.is_empty());
            }
            Err(e) => {
                panic!("API call failed: {:?}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_openrouter_streaming() {
        use futures::StreamExt;

        let api_key = std::env::var("OPENROUTER_API_KEY")
            .expect("OPENROUTER_API_KEY must be set for this test");

        let provider = OpenRouterProvider::from_api_key(api_key)
            .with_default_model("qwen/qwen3-coder");

        let request = crate::CompletionRequest::new(
            "qwen/qwen3-coder",
            vec![Message::user("Count from 1 to 5.")],
        )
        .with_max_tokens(100);

        let mut stream = provider.stream(request).await.expect("Failed to start stream");

        let mut full_response = String::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(c) => {
                    if let Some(text) = c.text {
                        print!("{}", text);
                        full_response.push_str(&text);
                    }
                }
                Err(e) => {
                    eprintln!("Stream error: {:?}", e);
                    break;
                }
            }
        }
        println!();
        println!("Full response: {}", full_response);
        assert!(!full_response.is_empty());
    }
}
