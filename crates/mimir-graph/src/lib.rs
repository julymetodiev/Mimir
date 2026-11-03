//! Code graph facade combining symbols and relations.
//!
//! This crate provides the high-level graph API:
//! - `CodeGraph`: The main facade combining symbols and relations
//! - Query methods: find_symbol, get_outgoing, get_incoming
//! - Trace functionality for call chain analysis

pub mod relations;
pub mod symbols;

mod facade;
mod trace;

pub use facade::CodeGraph;
pub use relations::{EdgeData, EdgeKind, RelationGraph, RelationsHandle};
pub use symbols::{SymbolTable, SymbolsHandle};
pub use trace::{TraceNode, TraceOptions, TraceRelationship, TraceResult};
