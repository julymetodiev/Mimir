//! Code graph facade combining symbols and relations.

use crate::relations::{EdgeData, EdgeKind, RelationsHandle};
use crate::symbols::SymbolsHandle;
use mimir_types::{Confidence, FileId, Span, Symbol, SymbolId, Visibility};
use std::collections::HashSet;
use std::sync::Arc;

use crate::query::{QueryOptions, QueryResult};
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

    /// Create from existing handles.
    pub fn from_handles(symbols: SymbolsHandle, relations: RelationsHandle) -> Self {
        Self { symbols, relations }
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

    /// Find symbols with options.
    pub fn find_symbols(&self, name: &str, options: QueryOptions) -> QueryResult<Arc<Symbol>> {
        let mut results = self.symbols.find_by_name(name);

        // Apply filters
        if let Some(lang) = options.language {
            results.retain(|s| s.language == lang);
        }

        if let Some(kind) = options.kind {
            results.retain(|s| s.kind == kind);
        }

        if options.public_only {
            results.retain(|s| s.visibility == Visibility::Public);
        }

        // Apply limit
        let has_more = if let Some(limit) = options.limit {
            let has_more = results.len() > limit;
            results.truncate(limit);
            has_more
        } else {
            false
        };

        QueryResult {
            total: results.len(),
            items: results,
            has_more,
        }
    }

    /// Get all symbols in a file.
    pub fn symbols_in_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        self.symbols.symbols_in_file(file_id)
    }

    /// Remove all symbols from a file.
    pub fn remove_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        let symbols = self.symbols.remove_file(file_id);
        for sym in &symbols {
            self.relations.remove_symbol(sym.id);
        }
        symbols
    }

    // ============ Relation Operations ============

    /// Replace every edge with the given ones in one step.
    pub fn replace_edges(&self, edges: Vec<(SymbolId, SymbolId, EdgeData)>) {
        self.relations.replace_edges(edges);
    }

    /// Add a call relationship between symbols.
    pub fn add_call(&self, from: SymbolId, to: SymbolId, span: Span, confidence: Confidence) {
        self.relations
            .add_edge(from, to, EdgeData::new(EdgeKind::Calls, span, confidence));
    }

    /// Add an import relationship.
    pub fn add_import(&self, from: SymbolId, to: SymbolId, span: Span) {
        self.relations.add_edge(
            from,
            to,
            EdgeData::new(EdgeKind::Imports, span, Confidence::Certain),
        );
    }

    /// Add a type usage relationship.
    pub fn add_type_use(&self, from: SymbolId, to: SymbolId, span: Span, confidence: Confidence) {
        self.relations.add_edge(
            from,
            to,
            EdgeData::new(EdgeKind::UsesType, span, confidence),
        );
    }

    /// Add an inheritance relationship.
    pub fn add_inheritance(&self, from: SymbolId, to: SymbolId, span: Span) {
        self.relations.add_edge(
            from,
            to,
            EdgeData::new(EdgeKind::Extends, span, Confidence::Certain),
        );
    }

    /// Add an implementation relationship.
    pub fn add_implementation(&self, from: SymbolId, to: SymbolId, span: Span) {
        self.relations.add_edge(
            from,
            to,
            EdgeData::new(EdgeKind::Implements, span, Confidence::Certain),
        );
    }

    // ============ Query Operations ============

    /// Get symbols that this symbol calls.
    pub fn get_callees(&self, symbol_id: SymbolId) -> Vec<Arc<Symbol>> {
        self.relations
            .callees(symbol_id)
            .into_iter()
            .filter_map(|id| self.symbols.get(id))
            .collect()
    }

    /// Get symbols that call this symbol.
    pub fn get_callers(&self, symbol_id: SymbolId) -> Vec<Arc<Symbol>> {
        self.relations
            .callers(symbol_id)
            .into_iter()
            .filter_map(|id| self.symbols.get(id))
            .collect()
    }

    /// Get all references from a symbol.
    pub fn get_outgoing(&self, symbol_id: SymbolId) -> Vec<(Arc<Symbol>, EdgeData)> {
        self.relations
            .outgoing(symbol_id)
            .into_iter()
            .filter_map(|(id, edge)| self.symbols.get(id).map(|s| (s, edge)))
            .collect()
    }

    /// Get all references to a symbol.
    pub fn get_incoming(&self, symbol_id: SymbolId) -> Vec<(Arc<Symbol>, EdgeData)> {
        self.relations
            .incoming(symbol_id)
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
            let mut visited = HashSet::new();
            visited.insert(symbol_id);
            self.trace_callers(symbol_id, 1, max_depth, &mut visited, &mut result.callers);
        }

        // Trace callees
        if options.include_callees {
            let mut visited = HashSet::new();
            visited.insert(symbol_id);
            self.trace_callees(symbol_id, 1, max_depth, &mut visited, &mut result.callees);
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

    fn trace_callers(
        &self,
        symbol_id: SymbolId,
        depth: usize,
        max_depth: usize,
        visited: &mut HashSet<SymbolId>,
        result: &mut Vec<TraceNode>,
    ) {
        if depth > max_depth {
            return;
        }

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
        let callers: Vec<_> = direct.chain(by_value).chain(via_trait).collect();

        for (caller_id, relationship) in callers {
            if !visited.insert(caller_id) {
                continue;
            }

            if let Some(symbol) = self.symbols.get(caller_id) {
                result.push(TraceNode {
                    symbol,
                    depth,
                    relationship,
                });
                self.trace_callers(caller_id, depth + 1, max_depth, visited, result);
            }
        }
    }

    fn trace_callees(
        &self,
        symbol_id: SymbolId,
        depth: usize,
        max_depth: usize,
        visited: &mut HashSet<SymbolId>,
        result: &mut Vec<TraceNode>,
    ) {
        if depth > max_depth {
            return;
        }

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
        let targets: Vec<_> = direct.chain(by_value).chain(implementations).collect();

        for (target_id, relationship) in targets {
            if !visited.insert(target_id) {
                continue;
            }

            if let Some(symbol) = self.symbols.get(target_id) {
                result.push(TraceNode {
                    symbol,
                    depth,
                    relationship,
                });
                self.trace_callees(target_id, depth + 1, max_depth, visited, result);
            }
        }
    }

    // ============ Stats ============

    /// Get the number of symbols in the graph.
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.relations.edge_count()
    }

    /// Clear the entire graph.
    pub fn clear(&self) {
        self.symbols.clear();
        self.relations.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::{IdGenerator, Language, QualifiedName, Range, SymbolKind};

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
    fn test_call_relationships() {
        let ids = ids();
        let graph = CodeGraph::new();
        let file_id: FileId = ids.next();

        let main = make_symbol("main", &ids, file_id);
        let helper = make_symbol("helper", &ids, file_id);

        let main_id = graph.add_symbol(main);
        let helper_id = graph.add_symbol(helper);

        graph.add_call(main_id, helper_id, Span::new(10, 20), Confidence::High);

        let callees = graph.get_callees(main_id);
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].name, "helper");

        let callers = graph.get_callers(helper_id);
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].name, "main");
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

        graph.add_call(a_id, b_id, Span::new(10, 20), Confidence::High);
        graph.add_call(b_id, c_id, Span::new(30, 40), Confidence::High);

        // Trace from b
        let result = graph.trace(b_id, TraceOptions::new()).unwrap();

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

    #[test]
    fn test_find_with_options() {
        let ids = ids();
        let graph = CodeGraph::new();
        let file_id: FileId = ids.next();

        let sym1 = make_symbol("process", &ids, file_id);
        let mut sym2 = make_symbol("process", &ids, file_id);
        sym2.visibility = Visibility::Private;

        graph.add_symbol(sym1);
        graph.add_symbol(sym2);

        // Find all
        let all = graph.find_symbol("process");
        assert_eq!(all.len(), 2);

        // Find public only
        let public = graph.find_symbols("process", QueryOptions::new().public_only());
        assert_eq!(public.items.len(), 1);
    }
}
