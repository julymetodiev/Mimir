//! Agent runtime for Mimir.
//!
//! This crate provides the core agent that orchestrates:
//! - LLM communication (via mimir-llm)
//! - Tool execution (via mimir-tools)
//! - Task and checkpoint management (via mimir-tasks)

mod agent;
mod config;
mod executor;

pub use agent::*;
pub use config::*;
pub use executor::*;

use thiserror::Error;

/// Errors that can occur during agent execution.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(#[from] mimir_llm::LlmError),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Task error: {0}")]
    Task(#[from] mimir_tasks::TaskError),

    #[error("Max iterations reached: {0}")]
    MaxIterations(u32),

    #[error("Agent not configured: {0}")]
    NotConfigured(String),

    #[error("Cancelled by user")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

/// Result type for agent operations.
pub type AgentResult<T> = Result<T, AgentError>;
