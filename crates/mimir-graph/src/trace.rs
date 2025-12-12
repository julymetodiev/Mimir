//! Trace functionality for call chain analysis.

use mimir_types::{Symbol, SymbolId};
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

    /// Follow cross-language references.
    pub cross_language: bool,
}

impl TraceOptions {
    /// Create default trace options (depth 3, include both callers and callees).
    pub fn new() -> Self {
        Self {
            max_depth: Some(3),
            include_callers: true,
            include_callees: true,
            cross_language: false,
        }
    }

    /// Set max depth.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Include only callers.
    pub fn callers_only(mut self) -> Self {
        self.include_callers = true;
        self.include_callees = false;
        self
    }

    /// Include only callees.
    pub fn callees_only(mut self) -> Self {
        self.include_callers = false;
        self.include_callees = true;
        self
    }

    /// Enable cross-language tracing.
    pub fn with_cross_language(mut self) -> Self {
        self.cross_language = true;
        self
    }
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

    /// Get all symbols in the trace (root + callers + callees).
    pub fn all_symbols(&self) -> Vec<Arc<Symbol>> {
        let mut symbols = vec![self.root.clone()];
        symbols.extend(self.callers.iter().map(|n| n.symbol.clone()));
        symbols.extend(self.callees.iter().map(|n| n.symbol.clone()));
        symbols
    }

    /// Get all symbol IDs in the trace.
    pub fn all_symbol_ids(&self) -> Vec<SymbolId> {
        self.all_symbols().iter().map(|s| s.id).collect()
    }

    /// Get the total number of symbols in the trace.
    pub fn total_count(&self) -> usize {
        1 + self.callers.len() + self.callees.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_options() {
        let opts = TraceOptions::new().with_max_depth(5).callers_only();

        assert_eq!(opts.max_depth, Some(5));
        assert!(opts.include_callers);
        assert!(!opts.include_callees);
    }

    #[test]
    fn test_trace_options_callees_only() {
        let opts = TraceOptions::new().callees_only();

        assert!(!opts.include_callers);
        assert!(opts.include_callees);
    }
}
