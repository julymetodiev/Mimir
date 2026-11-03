//! Rust language adapter for Mimir.
//!
//! This crate provides Rust-specific parsing using tree-sitter-rust:
//! - `RustAdapter`: Implementation of LanguageAdapter for Rust
//! - Symbol extraction for functions, structs, enums, etc.
//! - Reference extraction for calls, imports, type uses, etc.

mod adapter;
mod references;
mod symbols;

pub use adapter::RustAdapter;

// Re-export for convenience
pub use mimir_parse_core::{LanguageAdapter, ParsedTree};
