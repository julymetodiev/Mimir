//! Task and checkpoint system for Mimir agent.
//!
//! This crate provides:
//! - Task definitions with goals and context
//! - Checkpoint system for saving/resuming agent state
//! - Persistent task storage

mod checkpoint;
mod store;
mod task;

pub use checkpoint::*;
pub use store::*;
pub use task::*;

use thiserror::Error;

/// Errors that can occur in task operations.
#[derive(Debug, Error)]
pub enum TaskError {
    #[error("Task not found: {0}")]
    NotFound(String),

    #[error("Checkpoint not found: {0}")]
    CheckpointNotFound(String),

    #[error("Invalid task state: {0}")]
    InvalidState(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for task operations.
pub type TaskResult<T> = Result<T, TaskError>;
