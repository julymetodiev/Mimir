//! Edit error types.

use mimir_types::ContentHash;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during edit operations.
#[derive(Debug, Error)]
pub enum EditError {
    /// File not found.
    #[error("file not found: {path}")]
    FileNotFound { path: PathBuf },

    /// Content hash mismatch (optimistic locking failure).
    #[error("content hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: ContentHash,
        actual: ContentHash,
    },

    /// Invalid span - out of bounds.
    #[error("invalid span {start}..{end} for file {path} (size: {size})")]
    InvalidSpan {
        path: PathBuf,
        start: u32,
        end: u32,
        size: u32,
    },

    /// Invalid line range.
    #[error("invalid line range {start_line}..{end_line} for file {path} (lines: {total_lines})")]
    InvalidLineRange {
        path: PathBuf,
        start_line: u32,
        end_line: u32,
        total_lines: u32,
    },

    /// Overlapping edits in a batch.
    #[error("overlapping edits detected in batch operation")]
    OverlappingEdits,

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// UTF-8 decoding error.
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Result type for edit operations.
pub type EditResult<T> = Result<T, EditError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = EditError::FileNotFound {
            path: PathBuf::from("test.rs"),
        };
        assert!(err.to_string().contains("test.rs"));

        let err = EditError::InvalidSpan {
            path: PathBuf::from("test.rs"),
            start: 100,
            end: 200,
            size: 50,
        };
        assert!(err.to_string().contains("100..200"));
    }
}
