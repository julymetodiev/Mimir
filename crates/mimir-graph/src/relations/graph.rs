//! Relation graph implementation using petgraph.

use arc_swap::ArcSwap;
use mimir_types::SymbolId;
use petgraph::Direction;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::Arc;

use crate::relations::edge::EdgeData;

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

    /// Add symbols in one step. Readers keep the whole graph as it was, so a step copies all of it:
    /// one for each symbol would make loading a project quadratic in its size.
    pub fn add_symbols(&self, symbol_ids: impl IntoIterator<Item = SymbolId>) {
        let current = self.graph.load();
        let new: Vec<SymbolId> = symbol_ids
            .into_iter()
            .filter(|id| !current.symbol_to_node.contains_key(id))
            .collect();
        if new.is_empty() {
            return;
        }
        let mut new_inner = (**current).clone();
        for id in new {
            new_inner
                .symbol_to_node
                .entry(id)
                .or_insert_with(|| new_inner.graph.add_node(id));
        }
        self.graph.store(Arc::new(new_inner));
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

    /// Remove symbols and all their edges, in one step.
    pub fn remove_symbols(&self, symbol_ids: &[SymbolId]) {
        let current = self.graph.load();
        if !symbol_ids
            .iter()
            .any(|id| current.symbol_to_node.contains_key(id))
        {
            return;
        }
        let mut new_inner = (**current).clone();
        for id in symbol_ids {
            if let Some(node) = new_inner.symbol_to_node.remove(id) {
                new_inner.graph.remove_node(node);
            }
        }
        self.graph.store(Arc::new(new_inner));
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
        use crate::relations::edge::EdgeKind;

        self.outgoing(symbol_id)
            .into_iter()
            .filter(|(_, e)| e.kind == EdgeKind::Calls)
            .map(|(s, _)| s)
            .collect()
    }

    /// Get symbols that call this symbol.
    pub fn callers(&self, symbol_id: SymbolId) -> Vec<SymbolId> {
        use crate::relations::edge::EdgeKind;

        self.incoming(symbol_id)
            .into_iter()
            .filter(|(_, e)| e.kind == EdgeKind::Calls)
            .map(|(s, _)| s)
            .collect()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        let current = self.graph.load();
        current.graph.edge_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relations::edge::EdgeKind;
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
    }

    #[test]
    fn symbols_added_at_once_keep_what_is_there() {
        let ids = ids();
        let graph = RelationGraph::new();
        let (a, b, c): (SymbolId, SymbolId, SymbolId) = (ids.next(), ids.next(), ids.next());
        let node = graph.add_symbol(a);
        graph.add_edge(
            a,
            b,
            EdgeData::new(EdgeKind::Calls, Span::new(0, 1), Confidence::Certain),
        );

        graph.add_symbols([a, b, c, c]);

        assert_eq!(graph.add_symbol(a), node, "not added again");
        assert_eq!(graph.callees(a), [b]);
        assert_eq!(graph.graph.load().graph.node_count(), 3);
    }

    #[test]
    fn symbols_removed_at_once_take_their_edges() {
        let ids = ids();
        let graph = RelationGraph::new();
        let (a, b, c): (SymbolId, SymbolId, SymbolId) = (ids.next(), ids.next(), ids.next());
        graph.add_symbols([a, b, c]);
        graph.add_edge(
            a,
            b,
            EdgeData::new(EdgeKind::Calls, Span::new(0, 1), Confidence::Certain),
        );
        graph.add_edge(
            c,
            b,
            EdgeData::new(EdgeKind::Calls, Span::new(0, 1), Confidence::Certain),
        );

        graph.remove_symbols(&[a, b]);

        assert!(graph.callees(c).is_empty());
        assert!(graph.callers(b).is_empty());
        assert_eq!(graph.graph.load().graph.node_count(), 1);
        let unknown: SymbolId = ids.next();
        graph.remove_symbols(&[unknown]);
        assert_eq!(graph.graph.load().graph.node_count(), 1);
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

        graph.remove_symbols(&[sym1]);
    }
}
