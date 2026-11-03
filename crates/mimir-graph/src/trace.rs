//! Trace functionality for call chain analysis.

use mimir_types::Symbol;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Options for tracing symbol relationships.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceOptions {
    /// Maximum depth for call chain traversal.
    pub max_depth: Option<usize>,

    /// Include callers (symbols that call this symbol).
    pub include_callers: bool,

    /// Include callees (symbols that this symbol calls).
    pub include_callees: bool,
}

/// A node in the trace result call chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNode {
    /// The symbol at this node.
    pub symbol: Arc<Symbol>,

    /// Depth from the root symbol.
    pub depth: usize,

    /// Relationship to parent node.
    pub relationship: TraceRelationship,
}

/// The relationship between trace nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceRelationship {
    /// This is the root symbol.
    Root,
    /// This symbol calls the parent.
    Caller,
    /// This symbol is called by the parent.
    Callee,
    /// Calls a trait method this symbol implements, so it may reach this symbol at runtime.
    CallerViaTrait,
    /// Implements the trait method above it, so a call may land here at runtime.
    Implementation,
    /// Holds this function as a value, so it may call it at runtime.
    CallerByValue,
    /// Held as a value by the symbol above it, which may call it at runtime.
    CalleeByValue,
}

/// Result of a trace operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceResult {
    /// The root symbol being traced.
    pub root: Arc<Symbol>,

    /// Call chain going up (callers).
    pub callers: Vec<TraceNode>,

    /// Call chain going down (callees).
    pub callees: Vec<TraceNode>,

    /// Maximum depth reached.
    pub max_depth_reached: usize,
}

impl TraceResult {
    /// Create a new trace result.
    pub fn new(root: Arc<Symbol>) -> Self {
        Self {
            root,
            callers: Vec::new(),
            callees: Vec::new(),
            max_depth_reached: 0,
        }
    }
}
