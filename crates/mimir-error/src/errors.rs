//! Error types for Mimir.

use mimir_types::ContentHash;
use std::path::PathBuf;
use thiserror::Error;

/// Top-level error type for Mimir.
#[derive(Error, Debug)]
pub enum MimirError {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Graph error: {0}")]
    Graph(#[from] GraphError),

    #[error("Edit error: {0}")]
    Edit(#[from] EditError),

    #[error("Task error: {0}")]
    Task(#[from] TaskError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Storage-related errors.
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Content not found: {hash}")]
    ContentNotFound { hash: ContentHash },

    #[error("Failed to read file: {path}")]
    ReadFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to write file: {path}")]
    WriteFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to delete file: {path}")]
    DeleteFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Decompression error: {0}")]
    Decompression(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Invalid path: {path}")]
    InvalidPath { path: PathBuf },

    #[error("Directory not found: {path}")]
    DirectoryNotFound { path: PathBuf },
}

/// Parse-related errors.
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Failed to parse file: {path}")]
    ParseFailed { path: PathBuf, details: String },

    #[error("Unsupported language: {language}")]
    UnsupportedLanguage { language: String },

    #[error("Invalid syntax at line {line}, column {column}: {message}")]
    InvalidSyntax {
        line: u32,
        column: u32,
        message: String,
    },

    #[error("File too large: {path} ({size} bytes)")]
    FileTooLarge { path: PathBuf, size: u64 },
}

/// Graph-related errors.
#[derive(Error, Debug)]
pub enum GraphError {
    #[error("Symbol not found: {name}")]
    SymbolNotFound { name: String },

    #[error("Ambiguous symbol: {name} has {count} matches")]
    AmbiguousSymbol { name: String, count: usize },

    #[error("File not found in index: {path}")]
    FileNotFound { path: PathBuf },

    #[error("Cycle detected in graph")]
    CycleDetected,

    #[error("Invalid reference: from {from} to {to}")]
    InvalidReference { from: String, to: String },
}

/// Edit-related errors.
#[derive(Error, Debug)]
pub enum EditError {
    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("Content hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: ContentHash,
        actual: ContentHash,
    },

    #[error("Invalid span {start}..{end} for file {path} (size: {size} bytes)")]
    InvalidSpan {
        path: PathBuf,
        start: u32,
        end: u32,
        size: u32,
    },

    #[error("Invalid line range {start_line}..{end_line} for file {path} (lines: {total_lines})")]
    InvalidLineRange {
        path: PathBuf,
        start_line: u32,
        end_line: u32,
        total_lines: u32,
    },

    #[error("Overlapping edits detected in batch operation")]
    OverlappingEdits,

    #[error("Symbol not found for edit: {name}")]
    SymbolNotFound { name: String },

    #[error("Edit would create invalid syntax: {details}")]
    InvalidSyntax { details: String },

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Task-related errors.
#[derive(Error, Debug)]
pub enum TaskError {
    #[error("Task not found: {id}")]
    NotFound { id: String },

    #[error("Invalid task state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("Checkpoint not found: {id}")]
    CheckpointNotFound { id: String },

    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),

    #[error("Context limit exceeded: {used} / {limit} tokens")]
    ContextLimitExceeded { used: usize, limit: usize },

    #[error("Task cancelled")]
    Cancelled,
}

impl MimirError {
    /// Create an internal error with a message.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = StorageError::Compression("test error".to_string());
        assert_eq!(err.to_string(), "Compression error: test error");
    }

    #[test]
    fn test_error_conversion() {
        let storage_err = StorageError::Compression("test".to_string());
        let mimir_err: MimirError = storage_err.into();
        assert!(matches!(mimir_err, MimirError::Storage(_)));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let mimir_err: MimirError = io_err.into();
        assert!(matches!(mimir_err, MimirError::Io(_)));
    }
}
