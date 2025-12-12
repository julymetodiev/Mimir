//! Code graph facade combining symbols and relations.
//!
//! This crate provides the high-level graph API:
//! - `CodeGraph`: The main facade combining symbols and relations
//! - Query methods: find_symbol, find_references, get_callers, get_callees
//! - Trace functionality for call chain analysis

mod facade;
mod query;
mod trace;

pub use facade::CodeGraph;
pub use query::{QueryOptions, QueryResult};
pub use trace::{TraceOptions, TraceResult};
