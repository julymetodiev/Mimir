//! LLM provider trait and configuration.

use crate::{Completion, LlmResult, Message};
use async_trait::async_trait;
use mimir_tools::ToolSchema;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use futures::Stream;

/// Stream of completion chunks for streaming responses.
pub type CompletionStream = Pin<Box<dyn Stream<Item = LlmResult<StreamChunk>> + Send>>;

/// A chunk in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Type of chunk.
    pub chunk_type: ChunkType,
    /// Text delta (for text chunks).
    pub text: Option<String>,
    /// Tool use info (for tool chunks).
    pub tool_use: Option<ToolUseChunk>,
}

/// Type of stream chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkType {
    /// Start of message.
    MessageStart,
    /// Text content delta.
    ContentBlockDelta,
    /// Start of tool use.
    ToolUseStart,
    /// Tool use input delta.
    ToolUseInput,
    /// End of message.
    MessageStop,
}

/// Tool use chunk info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseChunk {
    pub id: Option<String>,
    pub name: Option<String>,
    pub input_delta: Option<String>,
}

/// Configuration for a completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// Messages in the conversation.
    pub messages: Vec<Message>,

    /// System prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// Model to use.
    pub model: String,

    /// Maximum tokens to generate.
    pub max_tokens: u32,

    /// Temperature (0.0 - 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Available tools.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<ToolSchema>,

    /// Stop sequences.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub stop_sequences: Vec<String>,
}

impl CompletionRequest {
    /// Create a new completion request.
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            messages,
            system: None,
            model: model.into(),
            max_tokens: 4096,
            temperature: None,
            tools: vec![],
            stop_sequences: vec![],
        }
    }

    /// Set the system prompt.
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set max tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Add tools.
    pub fn with_tools(mut self, tools: Vec<ToolSchema>) -> Self {
        self.tools = tools;
        self
    }

    /// Add stop sequences.
    pub fn with_stop_sequences(mut self, sequences: Vec<String>) -> Self {
        self.stop_sequences = sequences;
        self
    }
}

/// Provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// API key.
    pub api_key: String,

    /// Base URL (optional, for custom endpoints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Default model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,

    /// Request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Max retries on transient errors.
    #[serde(default = "default_retries")]
    pub max_retries: u32,
}

fn default_timeout() -> u64 {
    120
}

fn default_retries() -> u32 {
    3
}

impl ProviderConfig {
    /// Create a new provider config with API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: None,
            default_model: None,
            timeout_secs: default_timeout(),
            max_retries: default_retries(),
        }
    }

    /// Set custom base URL.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set default model.
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }
}

/// Trait for LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider name (e.g., "anthropic", "openai").
    fn name(&self) -> &str;

    /// List available models.
    fn models(&self) -> Vec<&str>;

    /// Default model for this provider.
    fn default_model(&self) -> &str;

    /// Complete a conversation.
    async fn complete(&self, request: CompletionRequest) -> LlmResult<Completion>;

    /// Stream a completion (returns a stream of chunks).
    async fn stream(&self, request: CompletionRequest) -> LlmResult<CompletionStream>;

    /// Check if the provider is configured and ready.
    fn is_ready(&self) -> bool;
}

/// Type alias for a boxed provider.
pub type BoxedProvider = Box<dyn LlmProvider>;

/// Implementation of LlmProvider for Box<dyn LlmProvider> to allow dynamic dispatch.
#[async_trait]
impl LlmProvider for Box<dyn LlmProvider> {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn models(&self) -> Vec<&str> {
        (**self).models()
    }

    fn default_model(&self) -> &str {
        (**self).default_model()
    }

    async fn complete(&self, request: CompletionRequest) -> LlmResult<Completion> {
        (**self).complete(request).await
    }

    async fn stream(&self, request: CompletionRequest) -> LlmResult<CompletionStream> {
        (**self).stream(request).await
    }

    fn is_ready(&self) -> bool {
        (**self).is_ready()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_request() {
        let request = CompletionRequest::new("claude-3-opus", vec![Message::user("Hello")])
            .with_system("You are helpful.")
            .with_max_tokens(1024)
            .with_temperature(0.7);

        assert_eq!(request.model, "claude-3-opus");
        assert_eq!(request.max_tokens, 1024);
        assert_eq!(request.temperature, Some(0.7));
        assert!(request.system.is_some());
    }

    #[test]
    fn test_provider_config() {
        let config = ProviderConfig::new("sk-test")
            .with_base_url("https://custom.api.com")
            .with_default_model("gpt-4");

        assert_eq!(config.api_key, "sk-test");
        assert_eq!(config.base_url, Some("https://custom.api.com".to_string()));
        assert_eq!(config.default_model, Some("gpt-4".to_string()));
    }
}
