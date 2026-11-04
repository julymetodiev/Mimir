//! Code graph facade combining symbols and relations.

use crate::relations::{EdgeData, EdgeKind, RelationsHandle};
use crate::symbols::SymbolsHandle;
use mimir_types::{FileId, Symbol, SymbolId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::trace::{TraceNode, TraceOptions, TraceRelationship, TraceResult};

/// The main code graph facade.
///
/// Combines the symbol table and relation graph into a unified API.
#[derive(Debug, Clone)]
pub struct CodeGraph {
    /// Symbol storage.
    symbols: SymbolsHandle,
    /// Relation storage.
    relations: RelationsHandle,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGraph {
    /// Create a new empty code graph.
    pub fn new() -> Self {
        Self {
            symbols: SymbolsHandle::new(),
            relations: RelationsHandle::new(),
        }
    }

    /// Get the symbols handle.
    pub fn symbols(&self) -> &SymbolsHandle {
        &self.symbols
    }

    /// Get the relations handle.
    pub fn relations(&self) -> &RelationsHandle {
        &self.relations
    }

    // ============ Symbol Operations ============

    /// Add a symbol to the graph.
    pub fn add_symbol(&self, symbol: Symbol) -> SymbolId {
        let id = symbol.id;
        self.symbols.insert(symbol);
        self.relations.add_symbol(id);
        id
    }

    /// Add symbols in one step, as when a whole project is read.
    pub fn add_symbols(&self, symbols: Vec<Symbol>) {
        let ids: Vec<SymbolId> = symbols.iter().map(|s| s.id).collect();
        for symbol in symbols {
            self.symbols.insert(symbol);
        }
        self.relations.add_symbols(ids);
    }

    /// Get a symbol by ID.
    pub fn get_symbol(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        self.symbols.get(id)
    }

    /// Find symbols by name.
    pub fn find_symbol(&self, name: &str) -> Vec<Arc<Symbol>> {
        self.symbols.find_by_name(name)
    }

    /// Find symbol by qualified name.
    pub fn find_by_qualified_name(&self, qualified: &str) -> Option<Arc<Symbol>> {
        self.symbols.find_by_qualified_name(qualified)
    }

    /// Get all symbols in a file.
    pub fn symbols_in_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        self.symbols.symbols_in_file(file_id)
    }

    /// Remove all symbols from a file.
    pub fn remove_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        let symbols = self.symbols.remove_file(file_id);
        let ids: Vec<SymbolId> = symbols.iter().map(|s| s.id).collect();
        self.relations.remove_symbols(&ids);
        symbols
    }

    // ============ Relation Operations ============

    /// Replace every edge with the given ones in one step.
    pub fn replace_edges(&self, edges: Vec<(SymbolId, SymbolId, EdgeData)>) {
        self.relations.replace_edges(edges);
    }

    // ============ Query Operations ============

    /// Get all references from a symbol.
    pub fn get_outgoing(&self, symbol_id: SymbolId) -> Vec<(Arc<Symbol>, EdgeData)> {
        self.relations
            .outgoing(symbol_id)
            .into_iter()
            .filter_map(|(id, edge)| self.symbols.get(id).map(|s| (s, edge)))
            .collect()
    }

    // ============ Trace Operations ============

    /// Trace the call chain for a symbol.
    pub fn trace(&self, symbol_id: SymbolId, options: TraceOptions) -> Option<TraceResult> {
        let root = self.symbols.get(symbol_id)?;
        let mut result = TraceResult::new(root);

        let max_depth = options.max_depth.unwrap_or(3);

        // Trace callers
        if options.include_callers {
            result.callers = self
                .walk(&[symbol_id], max_depth, |id| {
                    self.callers_of(id, id == symbol_id)
                })
                .concat();
        }

        // Trace callees
        if options.include_callees {
            result.callees = self
                .walk(&[symbol_id], max_depth, |id| self.callees_of(id))
                .concat();
        }

        // Calculate max depth reached
        result.max_depth_reached = result
            .callers
            .iter()
            .map(|n| n.depth)
            .chain(result.callees.iter().map(|n| n.depth))
            .max()
            .unwrap_or(0);

        Some(result)
    }

    /// What depends on each of these symbols, each dependent once, under the first that reaches it
    /// at its shortest depth. The symbols themselves are not among the dependents.
    pub fn dependents(&self, roots: &[SymbolId], max_depth: usize) -> Vec<Vec<TraceNode>> {
        self.walk(roots, max_depth, |id| {
            self.callers_of(id, roots.contains(&id))
        })
    }

    /// What is reached from each symbol, at its shortest depth, so what is reached does not depend
    /// on the order of the edges, nor does the answer. A symbol comes right after the one it was
    /// reached from, with what it leads to under it.
    fn walk(
        &self,
        roots: &[SymbolId],
        max_depth: usize,
        next: impl Fn(SymbolId) -> Vec<(SymbolId, TraceRelationship)>,
    ) -> Vec<Vec<TraceNode>> {
        let mut visited: HashSet<SymbolId> = roots.iter().copied().collect();
        let mut under: HashMap<SymbolId, Vec<(Arc<Symbol>, TraceRelationship)>> = HashMap::new();
        let mut level = roots.to_vec();
        for _ in 0..max_depth {
            let mut next_level = Vec::new();
            for parent in level {
                let mut found = Vec::new();
                for (id, relationship) in next(parent) {
                    if let Some(symbol) = self.symbols.get(id)
                        && visited.insert(id)
                    {
                        found.push((symbol, relationship));
                    }
                }
                found.sort_by_key(|(symbol, _)| {
                    (
                        symbol.file_id.value(),
                        symbol.range.start_line,
                        symbol.range.start_col,
                        symbol.name.clone(),
                    )
                });
                next_level.extend(found.iter().map(|(symbol, _)| symbol.id));
                under.insert(parent, found);
            }
            level = next_level;
        }
        roots
            .iter()
            .map(|root| {
                let mut result = Vec::new();
                push_under(*root, 1, &under, &mut result);
                result
            })
            .collect()
    }

    /// Who leads to a symbol, the surest first: the first is what a symbol found twice is called.
    /// Who reads or uses it is asked of what is traced alone: past that, one class used all over a
    /// project would bury the calls under everything that names it, and none of that is a call.
    fn callers_of(
        &self,
        symbol_id: SymbolId,
        with_uses: bool,
    ) -> Vec<(SymbolId, TraceRelationship)> {
        let value_users = |id| {
            self.relations
                .incoming(id)
                .into_iter()
                .filter(|(_, edge)| edge.kind == EdgeKind::UsesAsValue)
                .map(|(user, _)| user)
        };
        let via_trait = self
            .relations
            .outgoing(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::Implements)
            .flat_map(|(declared, _)| {
                self.relations
                    .callers(declared)
                    .into_iter()
                    .chain(value_users(declared))
            })
            .map(|caller| (caller, TraceRelationship::CallerViaTrait));
        let by_value = value_users(symbol_id).map(|user| (user, TraceRelationship::CallerByValue));
        let direct = self
            .relations
            .callers(symbol_id)
            .into_iter()
            .map(|caller| (caller, TraceRelationship::Caller));
        let readers = self
            .relations
            .incoming(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::UsesConstant)
            .map(|(reader, _)| (reader, TraceRelationship::Reader));
        let users = self
            .relations
            .incoming(symbol_id)
            .into_iter()
            .filter(|(_, edge)| matches!(edge.kind, EdgeKind::UsesType | EdgeKind::UsesField))
            .map(|(user, _)| (user, TraceRelationship::User));
        let mut found: Vec<_> = direct.chain(by_value).chain(via_trait).collect();
        if with_uses {
            found.extend(readers.chain(users));
        }
        found
    }

    /// What a symbol leads to, the surest first: the first is what a symbol found twice is called.
    fn callees_of(&self, symbol_id: SymbolId) -> Vec<(SymbolId, TraceRelationship)> {
        let implementations = self
            .relations
            .incoming(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::Implements)
            .map(|(method, _)| (method, TraceRelationship::Implementation));
        let direct = self
            .relations
            .callees(symbol_id)
            .into_iter()
            .map(|callee| (callee, TraceRelationship::Callee));
        let by_value = self
            .relations
            .outgoing(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::UsesAsValue)
            .map(|(function, _)| (function, TraceRelationship::CalleeByValue));
        direct.chain(by_value).chain(implementations).collect()
    }

    // ============ Stats ============

    /// Get the number of symbols in the graph.
    pub fn symbol_count(&self) -> usize {
        self.symbols.count()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.relations.edge_count()
    }
}

/// Add what was reached from a symbol, and under each what was reached from that.
fn push_under(
    parent: SymbolId,
    depth: usize,
    under: &HashMap<SymbolId, Vec<(Arc<Symbol>, TraceRelationship)>>,
    result: &mut Vec<TraceNode>,
) {
    for (symbol, relationship) in under.get(&parent).into_iter().flatten() {
        result.push(TraceNode {
            symbol: Arc::clone(symbol),
            depth,
            relationship: *relationship,
        });
        push_under(symbol.id, depth + 1, under, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::{
        Confidence, IdGenerator, Language, QualifiedName, Range, Span, SymbolKind, Visibility,
    };

    fn ids() -> IdGenerator {
        IdGenerator::new()
    }

    fn make_symbol(name: &str, ids: &IdGenerator, file_id: FileId) -> Symbol {
        Symbol {
            id: ids.next(),
            name: name.to_string(),
            qualified_name: QualifiedName::simple(name),
            kind: SymbolKind::Function,
            language: Language::Rust,
            file_id,
            range: Range::new(1, 0, 10, 1),
            span: Span::new(0, 100),
            signature: None,
            visibility: Visibility::Public,
            doc: None,
        }
    }

    #[test]
    fn test_add_and_find_symbol() {
        let ids = ids();
        let graph = CodeGraph::new();
        let file_id: FileId = ids.next();

        let sym = make_symbol("foo", &ids, file_id);
        let id = graph.add_symbol(sym);

        assert!(graph.get_symbol(id).is_some());
        assert_eq!(graph.get_symbol(id).unwrap().name, "foo");

        let found = graph.find_symbol("foo");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_symbol_found_by_a_long_way_keeps_its_short_one_and_what_is_past_it() {
        // x calls root and also, by way of q and p, comes to it the long way. y calls only x.
        let chains = [
            ("x", "root"),
            ("p", "root"),
            ("q", "p"),
            ("x", "q"),
            ("y", "x"),
        ];
        for reversed in [false, true] {
            let ids = ids();
            let graph = CodeGraph::new();
            let file_id: FileId = ids.next();
            let mut by_name = HashMap::new();
            for name in ["root", "x", "p", "q", "y"] {
                by_name.insert(name, graph.add_symbol(make_symbol(name, &ids, file_id)));
            }
            let mut edges = chains.to_vec();
            if reversed {
                edges.reverse();
            }
            for (from, to) in edges {
                graph.relations().add_edge(
                    by_name[from],
                    by_name[to],
                    EdgeData::new(EdgeKind::Calls, Span::new(0, 1), Confidence::High),
                );
            }
            let options = TraceOptions {
                max_depth: Some(3),
                include_callers: true,
                include_callees: false,
            };
            let callers = graph.trace(by_name["root"], options).unwrap().callers;
            let got: Vec<_> = callers
                .iter()
                .map(|node| (node.symbol.name.as_str(), node.depth))
                .collect();
            assert_eq!(got, [("p", 1), ("q", 2), ("x", 1), ("y", 2)]);
        }
    }

    #[test]
    fn a_use_is_followed_from_what_is_traced_and_no_further() {
        // `kind` is used by `user`, and `user` by `user_of_user`. `caller` calls `user`.
        let ids = ids();
        let graph = CodeGraph::new();
        let file_id: FileId = ids.next();
        let mut by_name = HashMap::new();
        for name in ["kind", "user", "user_of_user", "caller"] {
            by_name.insert(name, graph.add_symbol(make_symbol(name, &ids, file_id)));
        }
        for (from, to, edge_kind) in [
            ("user", "kind", EdgeKind::UsesType),
            ("user_of_user", "user", EdgeKind::UsesType),
            ("caller", "user", EdgeKind::Calls),
        ] {
            graph.relations().add_edge(
                by_name[from],
                by_name[to],
                EdgeData::new(edge_kind, Span::new(0, 1), Confidence::High),
            );
        }
        let options = TraceOptions {
            max_depth: Some(3),
            include_callers: true,
            include_callees: false,
        };
        let callers = graph.trace(by_name["kind"], options).unwrap().callers;
        let got: Vec<_> = callers
            .iter()
            .map(|node| (node.symbol.name.as_str(), node.depth, node.relationship))
            .collect();
        assert_eq!(
            got,
            [
                ("user", 1, TraceRelationship::User),
                ("caller", 2, TraceRelationship::Caller),
            ]
        );
    }

    #[test]
    fn test_trace() {
        let ids = ids();
        let graph = CodeGraph::new();
        let file_id: FileId = ids.next();

        // Create call chain: a -> b -> c
        let a = make_symbol("a", &ids, file_id);
        let b = make_symbol("b", &ids, file_id);
        let c = make_symbol("c", &ids, file_id);

        let a_id = graph.add_symbol(a);
        let b_id = graph.add_symbol(b);
        let c_id = graph.add_symbol(c);

        graph.relations().add_edge(
            a_id,
            b_id,
            EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High),
        );
        graph.relations().add_edge(
            b_id,
            c_id,
            EdgeData::new(EdgeKind::Calls, Span::new(30, 40), Confidence::High),
        );

        // Trace from b
        let result = graph
            .trace(
                b_id,
                TraceOptions {
                    max_depth: Some(3),
                    include_callers: true,
                    include_callees: true,
                },
            )
            .unwrap();

        assert_eq!(result.root.name, "b");
        assert_eq!(result.callers.len(), 1);
        assert_eq!(result.callers[0].symbol.name, "a");
        assert_eq!(result.callees.len(), 1);
        assert_eq!(result.callees[0].symbol.name, "c");
    }

    #[test]
    fn test_remove_file() {
        let ids = ids();
        let graph = CodeGraph::new();
        let file1: FileId = ids.next();
        let file2: FileId = ids.next();

        graph.add_symbol(make_symbol("foo", &ids, file1));
        graph.add_symbol(make_symbol("bar", &ids, file1));
        graph.add_symbol(make_symbol("baz", &ids, file2));

        assert_eq!(graph.symbol_count(), 3);

        let removed = graph.remove_file(file1);
        assert_eq!(removed.len(), 2);
        assert_eq!(graph.symbol_count(), 1);
    }
}
