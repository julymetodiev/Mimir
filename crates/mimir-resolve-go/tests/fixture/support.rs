//! Building a graph of Go files, and reading its edges as names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_parse_core::LanguageAdapter;
use mimir_parse_go::GoAdapter;
use mimir_resolve::ProjectFile;
use mimir_types::{FileId, IdGenerator, Symbol};

pub type Edge = (String, String);

pub fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/go-workspace")
}

pub fn expected(edges: &[(&str, &str)]) -> BTreeSet<Edge> {
    edges
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

/// Parse these files of a directory into a new graph.
pub fn index_at(directory: &Path, sources: &[&str]) -> (CodeGraph, Vec<ProjectFile>) {
    let (graph, ids) = (CodeGraph::new(), IdGenerator::new());
    let mut files = Vec::new();
    for path in sources {
        let source = std::fs::read_to_string(directory.join(path)).unwrap();
        let file_id: FileId = ids.next();
        let tree = GoAdapter.parse(&source).unwrap();
        let mut symbols = GoAdapter.extract_symbols(&tree, file_id);
        GoAdapter.qualify(Path::new(path), &mut symbols);
        for s in symbols {
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
            references: GoAdapter.extract_references(&tree, file_id).into(),
            source: source.as_str().into(),
        });
    }
    (graph, files)
}

pub fn edges_of_kind(graph: &CodeGraph, files: &[ProjectFile], kind: EdgeKind) -> BTreeSet<Edge> {
    let mut edges = BTreeSet::new();
    for file in files {
        for symbol in graph.symbols_in_file(file.file_id) {
            for (target, edge) in graph.get_outgoing(symbol.id) {
                if edge.kind == kind {
                    edges.insert((
                        symbol.qualified_name.to_string(),
                        target.qualified_name.to_string(),
                    ));
                }
            }
        }
    }
    edges
}

/// The edges of every kind that semantic resolution gives.
pub fn all_edges(graph: &CodeGraph, files: &[ProjectFile]) -> [BTreeSet<Edge>; 6] {
    [
        EdgeKind::Calls,
        EdgeKind::UsesAsValue,
        EdgeKind::UsesConstant,
        EdgeKind::Implements,
        EdgeKind::UsesType,
        EdgeKind::UsesField,
    ]
    .map(|kind| edges_of_kind(graph, files, kind))
}

/// Whether the tools an index needs are here. Without them a test says so and is skipped.
pub fn tools_installed(tools: &[(&str, &str)]) -> bool {
    for (tool, flag) in tools {
        if std::process::Command::new(tool).arg(flag).output().is_err() {
            eprintln!("skipped: {tool} is not installed");
            return false;
        }
    }
    true
}

/// A copy of a tree of files that may be edited, as a git repository or not.
pub fn copy_of(from: &Path, to: &Path, paths: &[&str]) {
    for path in paths {
        std::fs::create_dir_all(to.join(path).parent().unwrap()).unwrap();
        std::fs::copy(from.join(path), to.join(path)).unwrap();
    }
}
