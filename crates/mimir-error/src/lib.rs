//! Error types for the Mimir semantic code intelligence engine.

mod errors;

pub use errors::{EditError, GraphError, MimirError, ParseError, StorageError, TaskError};

/// A specialized Result type for Mimir operations.
pub type Result<T> = std::result::Result<T, MimirError>;
