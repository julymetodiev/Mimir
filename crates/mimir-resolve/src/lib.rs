//! Resolution: from references found in source to edges in the code graph.
//!
//! Every path that builds a graph (CLI sync, single-file trace, the daemon's actor)
//! goes through [`link`]. A file covered by a semantic index gets its call edges from
//! that index; any other file gets them from [`syntax`], which leaves a call out
//! rather than guess its target.

pub mod live;
mod lookup;
pub mod scip;
pub mod syntax;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use mimir_graph::CodeGraph;
use mimir_parse_core::ExtractedReference;
use mimir_relations::{EdgeData, EdgeKind};
use mimir_types::{Confidence, FileId, Span, SymbolId};

pub use live::Analyzers;
pub use scip::{Refresh, ScipIndex, load_or_generate};

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

    let covered = scip::resolve(&table, files, indexes, &mut edges);
    let semantic_calls = edges.count(EdgeKind::Calls);
    syntax::resolve_shorthand_values(&table, files, &covered, &mut edges);
    syntax::resolve_calls(&table, files, &covered, &mut edges);
    let syntax_calls = edges.count(EdgeKind::Calls) - semantic_calls;
    syntax::resolve_implementations(&table, files, &covered, &mut edges);
    let implementations = edges.count(EdgeKind::Implements);
    let function_values = edges.count(EdgeKind::UsesAsValue);
    let linked_calls = edges.len();
    syntax::resolve_imports(&table, files, &mut edges);
    let imports = edges.len() - linked_calls;

    graph.replace_edges(edges.into_vec());

    let semantic_files = files.iter().filter(|f| covered.contains(&f.file_id)).count();
    LinkReport {
        semantic_calls,
        syntax_calls,
        implementations,
        function_values,
        imports,
        semantic_files,
        live_files: indexes.iter().map(|index| index.live.len()).sum(),
        syntax_files: files.len() - semantic_files,
    }
}

/// Edges with no repeats: one per (from, to, kind), at the first site found.
#[derive(Default)]
pub(crate) struct EdgeSet {
    seen: HashSet<(SymbolId, SymbolId, EdgeKind)>,
    edges: Vec<(SymbolId, SymbolId, EdgeData)>,
}

impl EdgeSet {
    pub(crate) fn add(
        &mut self,
        from: SymbolId,
        to: SymbolId,
        kind: EdgeKind,
        span: Span,
        confidence: Confidence,
    ) {
        if self.seen.insert((from, to, kind)) {
            self.edges.push((from, to, EdgeData::new(kind, span, confidence)));
        }
    }

    fn len(&self) -> usize {
        self.edges.len()
    }

    fn count(&self, kind: EdgeKind) -> usize {
        self.edges.iter().filter(|(_, _, data)| data.kind == kind).count()
    }

    fn into_vec(self) -> Vec<(SymbolId, SymbolId, EdgeData)> {
        self.edges
    }
}

/// File ids by project-relative path.
pub(crate) fn files_by_path(files: &[ProjectFile]) -> HashMap<&std::path::Path, &ProjectFile> {
    files.iter().map(|f| (f.path.as_path(), f)).collect()
}
