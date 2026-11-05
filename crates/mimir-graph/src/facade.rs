//! The code graph: one buffer in the format of the file on disk, and queries over it.

use crate::encode::encode;
use crate::file::{FileEntry, GraphFile};
use crate::relations::{EdgeData, EdgeKind};
use arc_swap::ArcSwap;
use mimir_types::{ContentHash, FileId, Symbol, SymbolId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::trace::{TraceNode, TraceOptions, TraceRelationship, TraceResult};

/// The graph. A change makes a new buffer in one step, so a reader keeps a whole one.
pub struct CodeGraph {
    current: ArcSwap<GraphFile>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGraph {
    /// Create a new empty code graph.
    pub fn new() -> Self {
        Self::from_file(built(encode(Vec::new(), &[], &[])))
    }

    /// The graph a file holds.
    pub fn from_file(file: GraphFile) -> Self {
        Self {
            current: ArcSwap::new(Arc::new(file)),
        }
    }

    /// The bytes for a file on disk.
    pub fn bytes(&self) -> Vec<u8> {
        self.current.load().bytes().to_vec()
    }

    // ============ Symbol Operations ============

    /// Remove the symbols of the `gone` files with their edges, record `files` and add `symbols`,
    /// in one step: each step writes the whole graph, so many files must not take one each.
    pub fn change_files(&self, gone: &[FileId], files: Vec<FileEntry>, new: Vec<Symbol>) {
        let old = self.current.load();
        let gone: HashSet<FileId> = gone.iter().copied().collect();
        let mut symbols = old.all_symbols();
        symbols.retain(|symbol| !gone.contains(&symbol.file_id));
        let kept: HashSet<SymbolId> = symbols.iter().map(|symbol| symbol.id).collect();
        let mut edges = old.all_edges();
        edges.retain(|(from, to, _)| kept.contains(from) && kept.contains(to));

        let mut taken = kept;
        for mut symbol in new {
            // Two symbols whose names hash the same are still two symbols.
            while !taken.insert(symbol.id) {
                symbol.id = SymbolId::new(symbol.id.value().wrapping_add(1));
            }
            symbols.push(symbol);
        }

        let mut entries: HashMap<FileId, FileEntry> = old
            .files()
            .into_iter()
            .filter(|entry| !gone.contains(&entry.id))
            .map(|entry| (entry.id, entry))
            .collect();
        entries.extend(files.into_iter().map(|entry| (entry.id, entry)));
        for symbol in &symbols {
            entries.entry(symbol.file_id).or_insert_with(|| FileEntry {
                id: symbol.file_id,
                path: PathBuf::new(),
                hash: ContentHash::compute(&[]),
                size: 0,
                modified: 0,
            });
        }
        let entries: Vec<FileEntry> = entries.into_values().collect();
        self.current
            .store(Arc::new(built(encode(symbols, &edges, &entries))));
    }

    /// Add a symbol to the graph.
    pub fn add_symbol(&self, symbol: Symbol) -> SymbolId {
        let mut id = symbol.id;
        while self.current.load().get_symbol(id).is_some() {
            id = SymbolId::new(id.value().wrapping_add(1));
        }
        self.add_symbols(vec![Symbol { id, ..symbol }]);
        id
    }

    /// Add symbols in one step, as when a whole project is read.
    pub fn add_symbols(&self, symbols: Vec<Symbol>) {
        self.change_files(&[], Vec::new(), symbols);
    }

    /// Every file of the project, as last recorded.
    pub fn files(&self) -> Vec<FileEntry> {
        self.current.load().files()
    }

    /// Every symbol.
    pub fn all_symbols(&self) -> Vec<Arc<Symbol>> {
        arcs(self.current.load().all_symbols())
    }

    /// Get a symbol by ID.
    pub fn get_symbol(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        self.current.load().get_symbol(id).map(Arc::new)
    }

    /// Find symbols by name.
    pub fn find_symbol(&self, name: &str) -> Vec<Arc<Symbol>> {
        arcs(self.current.load().find_symbol(name))
    }

    /// Find the symbols with a qualified name: overloads share one.
    pub fn find_by_qualified_name(&self, qualified: &str) -> Vec<Arc<Symbol>> {
        arcs(self.current.load().find_by_qualified_name(qualified))
    }

    /// Get all symbols in a file.
    pub fn symbols_in_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        arcs(self.current.load().symbols_in_file(file_id))
    }

    /// How many symbols a file has.
    pub fn count_in_file(&self, file_id: FileId) -> usize {
        self.current.load().count_in_file(file_id)
    }

    /// Remove all symbols from a file.
    pub fn remove_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        let symbols = self.symbols_in_file(file_id);
        self.change_files(&[file_id], Vec::new(), Vec::new());
        symbols
    }

    // ============ Relation Operations ============

    /// Replace every edge with the given ones in one step.
    pub fn replace_edges(&self, edges: Vec<(SymbolId, SymbolId, EdgeData)>) {
        let old = self.current.load();
        let bytes = encode(old.all_symbols(), &edges, &old.files());
        self.current.store(Arc::new(built(bytes)));
    }

    /// What a symbol's edges lead to.
    pub fn outgoing(&self, symbol_id: SymbolId) -> Vec<(SymbolId, EdgeData)> {
        self.current.load().outgoing(symbol_id)
    }

    /// What leads to a symbol by an edge.
    pub fn incoming(&self, symbol_id: SymbolId) -> Vec<(SymbolId, EdgeData)> {
        self.current.load().incoming(symbol_id)
    }

    /// What a symbol calls.
    pub fn callees(&self, symbol_id: SymbolId) -> Vec<SymbolId> {
        calls(self.outgoing(symbol_id))
    }

    /// What calls a symbol.
    pub fn callers(&self, symbol_id: SymbolId) -> Vec<SymbolId> {
        calls(self.incoming(symbol_id))
    }

    // ============ Query Operations ============

    /// Get all references from a symbol.
    pub fn get_outgoing(&self, symbol_id: SymbolId) -> Vec<(Arc<Symbol>, EdgeData)> {
        self.outgoing(symbol_id)
            .into_iter()
            .filter_map(|(id, edge)| self.get_symbol(id).map(|s| (s, edge)))
            .collect()
    }

    // ============ Trace Operations ============

    /// Trace the call chain for a symbol.
    pub fn trace(
        &self,
        symbol_id: SymbolId,
        options: TraceOptions,
        paths: &HashMap<FileId, PathBuf>,
    ) -> Option<TraceResult> {
        let root = self.get_symbol(symbol_id)?;
        let mut result = TraceResult::new(root);

        let max_depth = options.max_depth.unwrap_or(3);

        // Trace callers
        if options.include_callers {
            result.callers = self
                .walk(&[symbol_id], max_depth, paths, |id| {
                    self.callers_of(id, id == symbol_id)
                })
                .concat();
        }

        // Trace callees
        if options.include_callees {
            result.callees = self
                .walk(&[symbol_id], max_depth, paths, |id| self.callees_of(id))
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
    pub fn dependents(
        &self,
        roots: &[SymbolId],
        max_depth: usize,
        paths: &HashMap<FileId, PathBuf>,
    ) -> Vec<Vec<TraceNode>> {
        self.walk(roots, max_depth, paths, |id| {
            self.callers_of(id, roots.contains(&id))
        })
    }

    /// What is reached from each symbol, at its shortest depth, so what is reached does not depend
    /// on the order of the edges, nor does the answer. A symbol comes right after the one it was
    /// reached from, with what it leads to under it, in order of path and place.
    fn walk(
        &self,
        roots: &[SymbolId],
        max_depth: usize,
        paths: &HashMap<FileId, PathBuf>,
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
                    if let Some(symbol) = self.get_symbol(id)
                        && visited.insert(id)
                    {
                        found.push((symbol, relationship));
                    }
                }
                found.sort_by_key(|(symbol, _)| {
                    (
                        paths.get(&symbol.file_id),
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
            self.incoming(id)
                .into_iter()
                .filter(|(_, edge)| edge.kind == EdgeKind::UsesAsValue)
                .map(|(user, _)| user)
        };
        let via_trait = self
            .outgoing(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::Implements)
            .flat_map(|(declared, _)| {
                self.callers(declared)
                    .into_iter()
                    .chain(value_users(declared))
            })
            .map(|caller| (caller, TraceRelationship::CallerViaTrait));
        let by_value = value_users(symbol_id).map(|user| (user, TraceRelationship::CallerByValue));
        let direct = self
            .callers(symbol_id)
            .into_iter()
            .map(|caller| (caller, TraceRelationship::Caller));
        let readers = self
            .incoming(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::UsesConstant)
            .map(|(reader, _)| (reader, TraceRelationship::Reader));
        let users = self
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
            .incoming(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::Implements)
            .map(|(method, _)| (method, TraceRelationship::Implementation));
        let direct = self
            .callees(symbol_id)
            .into_iter()
            .map(|callee| (callee, TraceRelationship::Callee));
        let by_value = self
            .outgoing(symbol_id)
            .into_iter()
            .filter(|(_, edge)| edge.kind == EdgeKind::UsesAsValue)
            .map(|(function, _)| (function, TraceRelationship::CalleeByValue));
        direct.chain(by_value).chain(implementations).collect()
    }

    // ============ Stats ============

    /// Get the number of symbols in the graph.
    pub fn symbol_count(&self) -> usize {
        self.current.load().symbol_count()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.current.load().edge_count()
    }
}

/// The graph bytes `encode` made. A failure here is a bug in `encode`, not a state to go on from.
fn built(bytes: Vec<u8>) -> GraphFile {
    match GraphFile::open(bytes) {
        Ok(file) => file,
        Err(reason) => panic!("the graph as written cannot be read: {reason}"),
    }
}

fn arcs(symbols: Vec<Symbol>) -> Vec<Arc<Symbol>> {
    symbols.into_iter().map(Arc::new).collect()
}

fn calls(edges: Vec<(SymbolId, EdgeData)>) -> Vec<SymbolId> {
    edges
        .into_iter()
        .filter(|(_, edge)| edge.kind == EdgeKind::Calls)
        .map(|(id, _)| id)
        .collect()
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
mod tests;
