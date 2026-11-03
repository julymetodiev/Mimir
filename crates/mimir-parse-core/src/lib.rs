//! Core parsing traits and types for Mimir.
//!
//! This crate provides the language adapter framework:
//! - `LanguageAdapter`: Trait for language-specific parsers
//! - `ParsedTree`: Wrapper around tree-sitter parse tree
//! - `ExtractedSymbol`, `ExtractedReference`: Extraction results
//! - `AdapterRegistry`: Registry of language adapters

mod adapter;
pub mod extracted;
pub mod nodes;
mod registry;
mod tree;

pub use adapter::LanguageAdapter;
pub use extracted::{ExtractedReference, ExtractedSymbol};
pub use registry::AdapterRegistry;
pub use tree::ParsedTree;
