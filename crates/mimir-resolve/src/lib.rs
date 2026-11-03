//! Resolution: from references found in source to edges in the code graph.
//!
//! A file a [`SemanticSource`] knows about gets its call edges from that source; any other file
//! from syntax rules, which leave a call out rather than guess its target.

mod definitions;
mod edges;
mod lookup;
mod port;
mod script;
mod semantic;
mod syntax;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_parse_core::ExtractedReference;
use mimir_types::{FileId, Language, SymbolId, SymbolKind};

pub use lookup::type_name;
pub use port::{
    LiveDocument, NoSemanticSource, Occurrence, Refresh, SemanticIndex, SemanticSource,
    SemanticSources, Target,
};
pub use script::resolve_import;

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
    /// Edges from a symbol to a type, trait or variant it names.
    pub type_uses: usize,
    /// Edges from a symbol to a field it uses.
    pub field_uses: usize,
    /// Import edges.
    pub imports: usize,
    /// Files covered by a semantic index.
    pub semantic_files: usize,
    /// Of those, files edited since the index, resolved by a running analyzer.
    pub live_files: usize,
    /// Files resolved by syntax rules only.
    pub syntax_files: usize,
}

/// Replace every edge in the graph with edges resolved from `files` and `indexes`. Each family of
/// languages is linked by itself: a name in one says nothing of a name in another.
pub fn link(graph: &CodeGraph, files: &[ProjectFile], indexes: &[SemanticIndex]) -> LinkReport {
    let mut families: HashMap<Language, Vec<ProjectFile>> = HashMap::new();
    for file in files {
        families
            .entry(Language::from_path(&file.path).family())
            .or_default()
            .push(file.clone());
    }
    let mut edges = EdgeSet::default();
    let mut report = LinkReport {
        // Only an index can be behind: with no documents, answers are all there is.
        live_files: indexes
            .iter()
            .filter(|index| !index.documents.is_empty())
            .map(|index| index.answers.len())
            .sum(),
        ..LinkReport::default()
    };
    for (family, files) in &families {
        link_family(graph, *family, files, indexes, &mut edges, &mut report);
    }
    graph.replace_edges(edges.into_vec());
    report
}

fn link_family(
    graph: &CodeGraph,
    family: Language,
    files: &[ProjectFile],
    indexes: &[SemanticIndex],
    edges: &mut EdgeSet,
    report: &mut LinkReport,
) {
    let table = lookup::SymbolTable::new(graph, files);
    let script = family == Language::TypeScript;
    let count = |edges: &EdgeSet, kind| edges.count(kind);
    let before = |edges: &EdgeSet| {
        [
            EdgeKind::Calls,
            EdgeKind::Implements,
            EdgeKind::UsesAsValue,
            EdgeKind::UsesConstant,
            EdgeKind::UsesType,
            EdgeKind::UsesField,
        ]
        .map(|kind| count(edges, kind))
    };
    let [calls, implementations, values, constants, types, fields] = before(edges);

    let mut constant_uses = Vec::new();
    let covered = semantic::resolve(&table, files, indexes, edges, &mut constant_uses);
    let semantic_calls = count(edges, EdgeKind::Calls) - calls;
    if script {
        script::resolve_calls(&table, files, &covered, edges);
    } else {
        syntax::resolve_shorthand_values(&table, files, &covered, edges);
        syntax::resolve_calls(&table, files, &covered, edges);
    }
    let syntax_calls = count(edges, EdgeKind::Calls) - calls - semantic_calls;
    if !script {
        syntax::resolve_implementations(&table, files, &covered, edges);
        syntax::resolve_constant_uses(&table, files, &covered, &mut constant_uses);
        syntax::resolve_format_reads(&table, files, &mut constant_uses);
    }
    add_uses_of_function_holders(graph, edges, &constant_uses);
    if !script {
        syntax::resolve_data_uses(&table, files, &covered, edges);
    }
    let linked = edges.len();
    if !script {
        syntax::resolve_imports(&table, files, edges);
    }

    let semantic_files = files
        .iter()
        .filter(|f| covered.contains(&f.file_id))
        .count();
    report.semantic_calls += semantic_calls;
    report.syntax_calls += syntax_calls;
    report.implementations += count(edges, EdgeKind::Implements) - implementations;
    report.function_values += count(edges, EdgeKind::UsesAsValue) - values;
    report.constant_uses += count(edges, EdgeKind::UsesConstant) - constants;
    report.type_uses += count(edges, EdgeKind::UsesType) - types;
    report.field_uses += count(edges, EdgeKind::UsesField) - fields;
    report.imports += edges.len() - linked;
    report.semantic_files += semantic_files;
    report.syntax_files += files.len() - semantic_files;
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
