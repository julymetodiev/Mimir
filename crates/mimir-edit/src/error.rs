//! Errors of file edits.

use mimir_types::ContentHash;
use std::path::PathBuf;
use thiserror::Error;

/// Errors from applying edit operations to files.
#[derive(Error, Debug)]
pub enum EditError {
    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("{path} already exists: to replace it, say so with overwrite")]
    FileExists { path: PathBuf },

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

    #[error("`{text}` is not in {path}: it must match exactly, so read the file again")]
    TextNotFound { path: PathBuf, text: String },

    #[error(
        "`{text}` occurs {count} times in {path}, on lines {lines}: include lines around it to say which, or ask for all"
    )]
    TextNotUnique {
        path: PathBuf,
        text: String,
        count: usize,
        lines: String,
    },

    #[error("No text to replace was given for {path}")]
    EmptyText { path: PathBuf },

    #[error("Overlapping edits detected in batch operation")]
    OverlappingEdits,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
