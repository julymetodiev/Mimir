//! Rust language adapter for Lantern Lattice.
//!
//! This crate provides Rust-specific parsing using tree-sitter-rust:
//! - `RustAdapter`: Implementation of LanguageAdapter for Rust
//! - Symbol extraction for functions, structs, enums, etc.
//! - Reference extraction for calls, imports, type uses, etc.

mod adapter;
mod format_reads;
mod imports;
mod references;
mod symbols;
mod uses;

pub use adapter::RustAdapter;

// Re-export for convenience
pub use latt_parse_core::{LanguageAdapter, ParsedTree};
