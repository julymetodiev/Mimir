//! Relation graph implementation using petgraph.

use arc_swap::ArcSwap;
use mimir_types::SymbolId;
use petgraph::Direction;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::Arc;

use crate::edge::EdgeData;

/// A graph of relationships between symbols.
///
/// Uses petgraph's StableGraph for stable node indices and
/// ArcSwap for atomic read access without locking.
#[derive(Debug)]
pub struct RelationGraph {
    /// The underlying graph wrapped in ArcSwap for atomic swaps.
    graph: ArcSwap<GraphInner>,
}

#[derive(Debug, Clone)]
struct GraphInner {
    /// The actual graph structure.
    graph: StableGraph<SymbolId, EdgeData>,
    /// Map from SymbolId to NodeIndex.
    symbol_to_node: HashMap<SymbolId, NodeIndex>,
}

impl Default for RelationGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl RelationGraph {
    /// Create a new empty relation graph.
    pub fn new() -> Self {
        Self {
            graph: ArcSwap::new(Arc::new(GraphInner {
                graph: StableGraph::new(),
                symbol_to_node: HashMap::new(),
            })),
        }
    }

    /// Add a symbol node to the graph.
    ///
    /// Returns the node index for the symbol.
    pub fn add_symbol(&self, symbol_id: SymbolId) -> NodeIndex {
        let current = self.graph.load();

        // Check if symbol already exists
        if let Some(&node) = current.symbol_to_node.get(&symbol_id) {
            return node;
        }

        // Clone and modify
        let mut new_inner = (**current).clone();
        let node = new_inner.graph.add_node(symbol_id);
        new_inner.symbol_to_node.insert(symbol_id, node);

        self.graph.store(Arc::new(new_inner));
        node
    }

    /// Add an edge between two symbols.
    ///
    /// Automatically adds symbol nodes if they don't exist.
    pub fn add_edge(&self, from: SymbolId, to: SymbolId, data: EdgeData) {
        let current = self.graph.load();
        let mut new_inner = (**current).clone();

        // Ensure both nodes exist
        let from_node = *new_inner
            .symbol_to_node
            .entry(from)
            .or_insert_with(|| new_inner.graph.add_node(from));
        let to_node = *new_inner
            .symbol_to_node
            .entry(to)
            .or_insert_with(|| new_inner.graph.add_node(to));

        // Add the edge
        new_inner.graph.add_edge(from_node, to_node, data);

        self.graph.store(Arc::new(new_inner));
    }

    /// Replace every edge with the given ones in one step, keeping the symbol nodes.
    pub fn replace_edges(&self, edges: Vec<(SymbolId, SymbolId, EdgeData)>) {
        let current = self.graph.load();
        let mut new_inner = (**current).clone();
        new_inner.graph.clear_edges();

        for (from, to, data) in edges {
            let from_node = *new_inner
                .symbol_to_node
                .entry(from)
                .or_insert_with(|| new_inner.graph.add_node(from));
            let to_node = *new_inner
                .symbol_to_node
                .entry(to)
                .or_insert_with(|| new_inner.graph.add_node(to));
            new_inner.graph.add_edge(from_node, to_node, data);
        }

        self.graph.store(Arc::new(new_inner));
    }

    /// Remove all edges from a symbol.
    pub fn remove_symbol_edges(&self, symbol_id: SymbolId) {
        let current = self.graph.load();

        if let Some(&node) = current.symbol_to_node.get(&symbol_id) {
            let mut new_inner = (**current).clone();

            // Collect edges to remove
            let edges_to_remove: Vec<_> = new_inner.graph.edges(node).map(|e| e.id()).collect();

            for edge in edges_to_remove {
                new_inner.graph.remove_edge(edge);
            }

            self.graph.store(Arc::new(new_inner));
        }
    }

    /// Remove a symbol and all its edges.
    pub fn remove_symbol(&self, symbol_id: SymbolId) {
        let current = self.graph.load();

        if let Some(&node) = current.symbol_to_node.get(&symbol_id) {
            let mut new_inner = (**current).clone();
            new_inner.graph.remove_node(node);
            new_inner.symbol_to_node.remove(&symbol_id);
            self.graph.store(Arc::new(new_inner));
        }
    }

    /// Get all outgoing edges from a symbol.
    pub fn outgoing(&self, symbol_id: SymbolId) -> Vec<(SymbolId, EdgeData)> {
        let current = self.graph.load();

        current
            .symbol_to_node
            .get(&symbol_id)
            .map(|&node| {
                current
                    .graph
                    .edges_directed(node, Direction::Outgoing)
                    .map(|e| (current.graph[e.target()], e.weight().clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all incoming edges to a symbol.
    pub fn incoming(&self, symbol_id: SymbolId) -> Vec<(SymbolId, EdgeData)> {
        let current = self.graph.load();

        current
            .symbol_to_node
            .get(&symbol_id)
            .map(|&node| {
                current
                    .graph
                    .edges_directed(node, Direction::Incoming)
                    .map(|e| (current.graph[e.source()], e.weight().clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get symbols that this symbol calls.
    pub fn callees(&self, symbol_id: SymbolId) -> Vec<SymbolId> {
        use crate::edge::EdgeKind;

        self.outgoing(symbol_id)
            .into_iter()
            .filter(|(_, e)| matches!(e.kind, EdgeKind::Calls | EdgeKind::Instantiates))
            .map(|(s, _)| s)
            .collect()
    }

    /// Get symbols that call this symbol.
    pub fn callers(&self, symbol_id: SymbolId) -> Vec<SymbolId> {
        use crate::edge::EdgeKind;

        self.incoming(symbol_id)
            .into_iter()
            .filter(|(_, e)| matches!(e.kind, EdgeKind::Calls | EdgeKind::Instantiates))
            .map(|(s, _)| s)
            .collect()
    }

    /// Check if a symbol exists in the graph.
    pub fn contains(&self, symbol_id: SymbolId) -> bool {
        let current = self.graph.load();
        current.symbol_to_node.contains_key(&symbol_id)
    }

    /// Get the number of symbols in the graph.
    pub fn symbol_count(&self) -> usize {
        let current = self.graph.load();
        current.graph.node_count()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        let current = self.graph.load();
        current.graph.edge_count()
    }

    /// Get all symbol IDs in the graph.
    pub fn symbols(&self) -> Vec<SymbolId> {
        let current = self.graph.load();
        current.symbol_to_node.keys().copied().collect()
    }

    /// Clear the graph.
    pub fn clear(&self) {
        self.graph.store(Arc::new(GraphInner {
            graph: StableGraph::new(),
            symbol_to_node: HashMap::new(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::EdgeKind;
    use mimir_types::{Confidence, IdGenerator, Span};

    fn ids() -> IdGenerator {
        IdGenerator::new()
    }

    #[test]
    fn test_add_symbol() {
        let ids = ids();
        let graph = RelationGraph::new();
        let sym1: SymbolId = ids.next();
        let sym2: SymbolId = ids.next();

        graph.add_symbol(sym1);
        graph.add_symbol(sym2);

        assert!(graph.contains(sym1));
        assert!(graph.contains(sym2));
        assert_eq!(graph.symbol_count(), 2);
    }

    #[test]
    fn test_add_edge() {
        let ids = ids();
        let graph = RelationGraph::new();
        let sym1: SymbolId = ids.next();
        let sym2: SymbolId = ids.next();

        graph.add_edge(
            sym1,
            sym2,
            EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High),
        );

        assert!(graph.contains(sym1));
        assert!(graph.contains(sym2));
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_outgoing_incoming() {
        let ids = ids();
        let graph = RelationGraph::new();
        let caller: SymbolId = ids.next();
        let callee: SymbolId = ids.next();

        graph.add_edge(
            caller,
            callee,
            EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High),
        );

        let outgoing = graph.outgoing(caller);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].0, callee);

        let incoming = graph.incoming(callee);
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].0, caller);
    }

    #[test]
    fn test_callers_callees() {
        let ids = ids();
        let graph = RelationGraph::new();
        let main: SymbolId = ids.next();
        let helper1: SymbolId = ids.next();
        let helper2: SymbolId = ids.next();

        // main calls helper1 and helper2
        graph.add_edge(
            main,
            helper1,
            EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High),
        );
        graph.add_edge(
            main,
            helper2,
            EdgeData::new(EdgeKind::Calls, Span::new(30, 40), Confidence::High),
        );

        let callees = graph.callees(main);
        assert_eq!(callees.len(), 2);
        assert!(callees.contains(&helper1));
        assert!(callees.contains(&helper2));

        let callers = graph.callers(helper1);
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0], main);
    }

    #[test]
    fn test_remove_symbol() {
        let ids = ids();
        let graph = RelationGraph::new();
        let sym1: SymbolId = ids.next();
        let sym2: SymbolId = ids.next();

        graph.add_edge(
            sym1,
            sym2,
            EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High),
        );

        assert_eq!(graph.symbol_count(), 2);
        graph.remove_symbol(sym1);
        assert!(!graph.contains(sym1));
        assert_eq!(graph.symbol_count(), 1);
    }

    #[test]
    fn test_clear() {
        let ids = ids();
        let graph = RelationGraph::new();
        let sym1: SymbolId = ids.next();
        let sym2: SymbolId = ids.next();

        graph.add_edge(
            sym1,
            sym2,
            EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High),
        );

        assert_eq!(graph.symbol_count(), 2);
        graph.clear();
        assert_eq!(graph.symbol_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }
}
