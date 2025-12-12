//! Relation graph for the Mimir code graph.
//!
//! This crate provides the edge/reference storage using petgraph:
//! - `RelationGraph`: The main graph structure
//! - `RelationsHandle`: Thread-safe handle for accessing relations

mod edge;
mod graph;
mod handle;

pub use edge::{EdgeData, EdgeKind};
pub use graph::RelationGraph;
pub use handle::RelationsHandle;
