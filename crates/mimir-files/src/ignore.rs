//! Gitignore-style pattern matching.
//!
//! This module re-exports functionality from the `ignore` crate.

// The `ignore` crate handles all the gitignore functionality.
// This module exists for potential future extensions.

/// Default patterns to ignore.
#[allow(dead_code)]
pub const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    "**/target/**",
    "**/node_modules/**",
    "**/.git/**",
    "**/dist/**",
    "**/build/**",
    "**/__pycache__/**",
    "**/*.pyc",
    "**/.venv/**",
    "**/venv/**",
];
