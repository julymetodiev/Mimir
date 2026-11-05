//! Building the fixture's graph, and reading its edges as names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mimir_graph::{CodeGraph, EdgeKind, TraceOptions};
use mimir_parse_core::LanguageAdapter;
use mimir_parse_rust::RustAdapter;
use mimir_resolve::ProjectFile;
use mimir_types::{FileId, IdGenerator, Symbol, SymbolId, SymbolKind};

use crate::expected::SOURCES;

pub type Edge = (String, String);

pub fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/resolve-workspace")
}

pub fn expected(calls: &[(&str, &str)]) -> BTreeSet<Edge> {
    calls
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

pub fn index() -> (CodeGraph, Vec<ProjectFile>) {
    index_at(&workspace())
}

pub fn index_at(workspace: &Path) -> (CodeGraph, Vec<ProjectFile>) {
    let (graph, ids) = (CodeGraph::new(), IdGenerator::new());
    let adapter = RustAdapter::new();
    let mut files = Vec::new();
    for path in SOURCES {
        let source = std::fs::read_to_string(workspace.join(path)).unwrap();
        let file_id: FileId = ids.next();
        let tree = adapter.parse(&source).unwrap();
        for s in adapter.extract_symbols(&tree, file_id) {
            graph.add_symbol(Symbol {
                id: ids.next(),
                name: s.name,
                qualified_name: s.qualified_name,
                kind: s.kind,
                language: s.language,
                file_id,
                range: s.range,
                span: s.span,
                signature: s.signature,
                visibility: s.visibility,
                doc: s.doc,
            });
        }
        files.push(ProjectFile {
            file_id,
            path: PathBuf::from(path),
            references: adapter.extract_references(&tree, file_id).into(),
            source: source.as_str().into(),
        });
    }
    (graph, files)
}

/// `crate::owner::name` of a symbol.
pub fn label(symbol: &Symbol, files: &[ProjectFile]) -> String {
    let file = files.iter().find(|f| f.file_id == symbol.file_id).unwrap();
    let krate = file
        .path
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    let segments = symbol.qualified_name.segments();
    match segments.len().checked_sub(2).map(|i| segments[i].as_str()) {
        Some(parent) => {
            let parent = parent.strip_prefix("impl ").unwrap_or(parent);
            let owner = parent.rsplit(" for ").next().unwrap();
            format!("{krate}::{owner}::{}", symbol.name)
        }
        None => format!("{krate}::{}", symbol.name),
    }
}

pub fn trace_options(
    include_callers: bool,
    include_callees: bool,
    max_depth: usize,
) -> TraceOptions {
    TraceOptions {
        max_depth: Some(max_depth),
        include_callers,
        include_callees,
    }
}

pub fn callees_of(
    graph: &CodeGraph,
    id: mimir_types::SymbolId,
) -> Vec<std::sync::Arc<mimir_types::Symbol>> {
    graph
        .callees(id)
        .into_iter()
        .filter_map(|callee| graph.get_symbol(callee))
        .collect()
}

pub fn call_edges(graph: &CodeGraph, files: &[ProjectFile]) -> BTreeSet<Edge> {
    let mut edges = BTreeSet::new();
    for file in files {
        for symbol in graph.symbols_in_file(file.file_id) {
            for callee in callees_of(graph, symbol.id) {
                edges.insert((label(&symbol, files), label(&callee, files)));
            }
        }
    }
    edges
}

pub fn implementation_edges(graph: &CodeGraph, files: &[ProjectFile]) -> BTreeSet<Edge> {
    edges_of_kind(graph, files, EdgeKind::Implements)
}

pub fn edges_of_kind(graph: &CodeGraph, files: &[ProjectFile], kind: EdgeKind) -> BTreeSet<Edge> {
    let mut edges = BTreeSet::new();
    for file in files {
        for symbol in graph.symbols_in_file(file.file_id) {
            for (target, edge) in graph.get_outgoing(symbol.id) {
                if edge.kind == kind {
                    edges.insert((label(&symbol, files), label(&target, files)));
                }
            }
        }
    }
    edges
}

pub fn find(graph: &CodeGraph, files: &[ProjectFile], wanted: &str) -> SymbolId {
    files
        .iter()
        .flat_map(|f| graph.symbols_in_file(f.file_id))
        .find(|s| {
            label(s, files) == wanted
                && matches!(
                    s.kind,
                    SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Constant
                        | SymbolKind::TypeAlias
                        | SymbolKind::Field
                )
        })
        .unwrap_or_else(|| panic!("no symbol {wanted}"))
        .id
}
