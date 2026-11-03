//! LLM provider abstraction for Mimir agent.
//!
//! This crate provides a unified interface for interacting with different LLM providers
//! (Anthropic, OpenAI, etc.) with support for tool use and streaming.

mod anthropic;
mod message;
mod openai_compat;
mod openrouter;
mod provider;
mod zai;

pub use anthropic::AnthropicProvider;
pub use message::*;
pub use openrouter::OpenRouterProvider;
pub use provider::*;
pub use zai::ZaiProvider;

use thiserror::Error;

/// Errors that can occur when interacting with LLM providers.
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },

    #[error("Rate limited, retry after {retry_after:?} seconds")]
    RateLimited { retry_after: Option<u64> },

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Provider not configured: {0}")]
    NotConfigured(String),

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Timeout")]
    Timeout,

    #[error("{0}")]
    Other(String),
}

/// Result type for LLM operations.
pub type LlmResult<T> = Result<T, LlmError>;
