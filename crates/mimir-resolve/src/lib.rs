//! Resolution: from references found in source to edges in the code graph.
//!
//! A file a [`SemanticSource`] knows about gets its call edges from that source; any other file
//! from syntax rules, which leave a call out rather than guess its target.

mod edges;
mod lookup;
mod port;
mod semantic;
mod syntax;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_parse_core::ExtractedReference;
use mimir_types::{FileId, SymbolId, SymbolKind};

pub use lookup::type_name;
pub use port::{
    LiveDocument, NoSemanticSource, Occurrence, Refresh, ScipIndex, SemanticSource, Target,
};

use edges::{ConstantUse, EdgeSet};

/// A source file and what was extracted from it.
#[derive(Debug, Clone)]
pub struct ProjectFile {
    /// The file's id in the graph.
    pub file_id: FileId,
    /// Path relative to the project root.
    pub path: PathBuf,
    /// The file's text.
    pub source: Arc<str>,
    /// References found in the file.
    pub references: Vec<ExtractedReference>,
}

/// What [`link`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkReport {
    /// Call edges from a semantic index.
    pub semantic_calls: usize,
    /// Call edges from syntax rules.
    pub syntax_calls: usize,
    /// Edges from a method of an impl to the method of its trait.
    pub implementations: usize,
    /// Edges from a symbol to a function it names without calling.
    pub function_values: usize,
    /// Edges from a symbol to a plain constant it reads.
    pub constant_uses: usize,
    /// Import edges.
    pub imports: usize,
    /// Files covered by a semantic index.
    pub semantic_files: usize,
    /// Of those, files edited since the index, resolved by a running analyzer.
    pub live_files: usize,
    /// Files resolved by syntax rules only.
    pub syntax_files: usize,
}

/// Replace every edge in the graph with edges resolved from `files` and `indexes`.
pub fn link(graph: &CodeGraph, files: &[ProjectFile], indexes: &[ScipIndex]) -> LinkReport {
    let table = lookup::SymbolTable::new(graph, files);
    let mut edges = EdgeSet::default();

    let mut constant_uses = Vec::new();
    let covered = semantic::resolve(&table, files, indexes, &mut edges, &mut constant_uses);
    let semantic_calls = edges.count(EdgeKind::Calls);
    syntax::resolve_shorthand_values(&table, files, &covered, &mut edges);
    syntax::resolve_calls(&table, files, &covered, &mut edges);
    let syntax_calls = edges.count(EdgeKind::Calls) - semantic_calls;
    syntax::resolve_implementations(&table, files, &covered, &mut edges);
    let implementations = edges.count(EdgeKind::Implements);
    syntax::resolve_constant_uses(&table, files, &covered, &mut constant_uses);
    add_uses_of_function_holders(graph, &mut edges, &constant_uses);
    let function_values = edges.count(EdgeKind::UsesAsValue);
    let constant_uses = edges.count(EdgeKind::UsesConstant);
    let linked_calls = edges.len();
    syntax::resolve_imports(&table, files, &mut edges);
    let imports = edges.len() - linked_calls;

    graph.replace_edges(edges.into_vec());

    let semantic_files = files
        .iter()
        .filter(|f| covered.contains(&f.file_id))
        .count();
    LinkReport {
        semantic_calls,
        syntax_calls,
        implementations,
        function_values,
        constant_uses,
        imports,
        semantic_files,
        live_files: indexes.iter().map(|index| index.live.len()).sum(),
        syntax_files: files.len() - semantic_files,
    }
}

/// Add an edge for each use of a constant. One that holds a function, directly or through another
/// constant, is a possible call. Any other is only a read, which `trace` shows on the constant and
/// not on the reader: `MAX_SIZE` under every function would hide the rest.
fn add_uses_of_function_holders(graph: &CodeGraph, edges: &mut EdgeSet, uses: &[ConstantUse]) {
    let is_constant = |id: SymbolId| {
        graph
            .get_symbol(id)
            .is_some_and(|s| s.kind == SymbolKind::Constant)
    };
    let mut holders: HashSet<SymbolId> = edges
        .iter()
        .filter(|(from, _, data)| data.kind == EdgeKind::UsesAsValue && is_constant(*from))
        .map(|(from, _, _)| *from)
        .collect();
    loop {
        let mut grew = false;
        for used in uses {
            if holders.contains(&used.constant) && used.from != used.constant {
                edges.add(
                    used.from,
                    used.constant,
                    EdgeKind::UsesAsValue,
                    used.span,
                    used.confidence,
                );
                grew |= is_constant(used.from) && holders.insert(used.from);
            }
        }
        if !grew {
            break;
        }
    }
    for used in uses {
        if !holders.contains(&used.constant) && used.from != used.constant {
            edges.add(
                used.from,
                used.constant,
                EdgeKind::UsesConstant,
                used.span,
                used.confidence,
            );
        }
    }
}
