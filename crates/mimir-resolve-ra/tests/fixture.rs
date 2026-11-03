//! Resolution against a small workspace whose calls were listed by hand, so neither
//! resolver is graded by itself. `fixtures/resolve-workspace` holds the traps: same names
//! across types and crates, calls inside macros, trait methods, nested functions.

mod expected;

use expected::{
    CALLS, CONSTANT_READS, IMPLEMENTS, IMPLEMENTS_BY_SYNTAX, NEEDS_TYPES, SOURCES, VALUES,
    VALUES_NEED_TYPES,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mimir_graph::EdgeKind;
use mimir_graph::{CodeGraph, TraceOptions, TraceRelationship};
use mimir_parse_core::LanguageAdapter;
use mimir_parse_rust::RustAdapter;
use mimir_resolve::{ProjectFile, Refresh, ScipIndex, link};
use mimir_resolve_ra::{Analyzers, load_or_generate};
use mimir_types::{FileId, IdGenerator, Symbol, SymbolId, SymbolKind};

type Edge = (String, String);

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/resolve-workspace")
}

fn expected(calls: &[(&str, &str)]) -> BTreeSet<Edge> {
    calls
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn index() -> (CodeGraph, Vec<ProjectFile>) {
    index_at(&workspace())
}

fn index_at(workspace: &Path) -> (CodeGraph, Vec<ProjectFile>) {
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
            references: adapter.extract_references(&tree, file_id),
            source: source.as_str().into(),
        });
    }
    (graph, files)
}

/// `crate::owner::name` of a symbol.
fn label(symbol: &Symbol, files: &[ProjectFile]) -> String {
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

fn trace_options(include_callers: bool, include_callees: bool, max_depth: usize) -> TraceOptions {
    TraceOptions {
        max_depth: Some(max_depth),
        include_callers,
        include_callees,
    }
}

fn callees_of(
    graph: &CodeGraph,
    id: mimir_types::SymbolId,
) -> Vec<std::sync::Arc<mimir_types::Symbol>> {
    graph
        .relations()
        .callees(id)
        .into_iter()
        .filter_map(|callee| graph.get_symbol(callee))
        .collect()
}

fn call_edges(graph: &CodeGraph, files: &[ProjectFile]) -> BTreeSet<Edge> {
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

fn implementation_edges(graph: &CodeGraph, files: &[ProjectFile]) -> BTreeSet<Edge> {
    edges_of_kind(graph, files, EdgeKind::Implements)
}

fn edges_of_kind(graph: &CodeGraph, files: &[ProjectFile], kind: EdgeKind) -> BTreeSet<Edge> {
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

#[test]
fn syntax_rules_never_guess() {
    let (graph, files) = index();
    let indexes: [ScipIndex; 0] = [];
    let report = link(&graph, &files, &indexes);

    let linked = call_edges(&graph, &files);
    let all = expected(CALLS);
    let needs_types = expected(NEEDS_TYPES);
    let wrong: Vec<_> = linked.difference(&all).collect();
    assert!(wrong.is_empty(), "edges that are not calls: {wrong:#?}");
    let reachable: BTreeSet<_> = all.difference(&needs_types).cloned().collect();
    let missing: Vec<_> = reachable.difference(&linked).collect();
    assert!(
        missing.is_empty(),
        "calls syntax should reach: {missing:#?}"
    );
    assert_eq!(
        implementation_edges(&graph, &files),
        expected(IMPLEMENTS_BY_SYNTAX)
    );
    let by_syntax: BTreeSet<_> = expected(VALUES)
        .difference(&expected(VALUES_NEED_TYPES))
        .cloned()
        .collect();
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesAsValue),
        by_syntax
    );
    // Without types a name with two declarations is left out, and no read lands on the wrong one.
    let reads = edges_of_kind(&graph, &files, EdgeKind::UsesConstant);
    assert!(reads.is_subset(&expected(CONSTANT_READS)), "{reads:?}");
    assert!(
        !reads
            .iter()
            .any(|(from, _)| from == "beta::uses_other_limit"),
        "two constants could be meant: {reads:?}"
    );
    assert!(reads.contains(&("beta::uses_plain".to_string(), "beta::PLAIN".to_string())));
    assert_eq!(report.semantic_files, 0);
    assert_eq!(report.syntax_files, SOURCES.len());
}

#[test]
fn semantic_index_finds_every_call() {
    if std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipped: rust-analyzer is not installed");
        return;
    }
    let (graph, files) = index();
    let cache = std::env::temp_dir().join(format!("mimir-resolve-fixture-{}", std::process::id()));
    let indexes = load_or_generate(
        &workspace(),
        &cache,
        &files,
        &mut Analyzers::default(),
        Refresh::Edits,
    );
    let _ = std::fs::remove_dir_all(&cache);
    assert_eq!(indexes.len(), 1, "the fixture is one Cargo workspace");

    let report = link(&graph, &files, &indexes);
    assert_eq!(report.semantic_files, SOURCES.len());
    assert_eq!(call_edges(&graph, &files), expected(CALLS));
    for caller in ["alpha::one_calls", "alpha::two_calls"] {
        let caller = find(&graph, &files, caller);
        let callees = callees_of(&graph, caller);
        assert_eq!(callees.len(), 1);
        assert_eq!(
            callees[0].file_id,
            graph.get_symbol(caller).unwrap().file_id,
            "the helper of its own file"
        );
    }
    assert_eq!(implementation_edges(&graph, &files), expected(IMPLEMENTS));
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesAsValue),
        expected(VALUES)
    );
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesConstant),
        expected(CONSTANT_READS)
    );

    let plain = find(&graph, &files, "beta::PLAIN");
    let trace = graph.trace(plain, trace_options(true, false, 1)).unwrap();
    assert_eq!(
        trace
            .callers
            .iter()
            .map(|n| (label(&n.symbol, &files), n.relationship))
            .collect::<Vec<_>>(),
        [("beta::uses_plain".to_string(), TraceRelationship::Reader)]
    );
    let reader = find(&graph, &files, "beta::uses_plain");
    let trace = graph.trace(reader, trace_options(false, true, 1)).unwrap();
    assert!(trace.callees.is_empty(), "a read is not a callee");

    let names = |nodes: &[mimir_graph::TraceNode], relationship| {
        let mut names: Vec<_> = nodes
            .iter()
            .filter(|n| n.relationship == relationship)
            .map(|n| label(&n.symbol, &files))
            .collect();
        names.sort();
        names
    };

    // The implementation is reached by whoever calls the trait's method.
    let memory_load = find(&graph, &files, "alpha::Memory::load");
    let trace = graph
        .trace(memory_load, trace_options(true, false, 1))
        .unwrap();
    assert_eq!(
        names(&trace.callers, TraceRelationship::CallerViaTrait),
        ["alpha::Store::load_twice", "beta::dyn_method_as_value"]
    );
    assert!(names(&trace.callers, TraceRelationship::Caller).is_empty());

    // Under a call through the trait, each implementation shows, and what it calls.
    let through_trait = find(&graph, &files, "beta::through_trait");
    let trace = graph
        .trace(through_trait, trace_options(false, true, 4))
        .unwrap();
    assert_eq!(
        names(&trace.callees, TraceRelationship::Implementation),
        ["alpha::Disk::load", "alpha::Memory::load"]
    );
    assert!(
        names(&trace.callees, TraceRelationship::Callee).contains(&"alpha::Disk::new".to_string())
    );

    // A function held as a value shows on both sides, marked as such.
    let get = find(&graph, &files, "alpha::Disk::get");
    let trace = graph.trace(get, trace_options(true, false, 1)).unwrap();
    assert_eq!(
        names(&trace.callers, TraceRelationship::CallerByValue),
        ["beta::method_as_value"]
    );
    let holder = find(&graph, &files, "beta::method_as_value");
    let trace = graph.trace(holder, trace_options(false, true, 1)).unwrap();
    assert_eq!(
        names(&trace.callees, TraceRelationship::CalleeByValue),
        ["alpha::Disk::get"]
    );

    // A function in a table is reached through whoever uses the table.
    let helper = find(&graph, &files, "beta::helper");
    let trace = graph.trace(helper, trace_options(true, false, 3)).unwrap();
    let reached = names(&trace.callers, TraceRelationship::CallerByValue);
    for holder in [
        "beta::HANDLERS",
        "beta::run_handlers",
        "beta::NESTED",
        "beta::run_nested",
    ] {
        assert!(
            reached.contains(&holder.to_string()),
            "{holder} not in {reached:?}"
        );
    }
}

/// An edit answered by a running analyzer gives what a new index gives.
#[test]
fn edited_files_match_a_new_index() {
    if std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipped: rust-analyzer is not installed");
        return;
    }
    let scratch = std::env::temp_dir().join(format!("mimir-resolve-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    for path in ["Cargo.toml", "alpha/Cargo.toml", "beta/Cargo.toml"]
        .into_iter()
        .chain(SOURCES)
    {
        std::fs::create_dir_all(scratch.join(path).parent().unwrap()).unwrap();
        std::fs::copy(workspace().join(path), scratch.join(path)).unwrap();
    }
    let edit = |path: &str, from: &str, to: &str| {
        let text = std::fs::read_to_string(scratch.join(path)).unwrap();
        assert!(text.contains(from), "{from}");
        std::fs::write(scratch.join(path), text.replacen(from, to, 1)).unwrap();
    };
    let edges = |graph: &CodeGraph, files: &[ProjectFile]| {
        [
            EdgeKind::Calls,
            EdgeKind::UsesAsValue,
            EdgeKind::UsesConstant,
            EdgeKind::Implements,
        ]
        .map(|kind| edges_of_kind(graph, files, kind))
    };

    let cache = scratch.join("cache");
    let mut analyzers = Analyzers::default();
    let (_, files) = index_at(&scratch);
    load_or_generate(&scratch, &cache, &files, &mut analyzers, Refresh::Edits);

    // Lines move in both files, a function is added, and a call and a value change.
    edit(
        "alpha/src/lib.rs",
        "pub struct Disk;",
        "pub fn added() -> u32 {\n    helper()\n}\n\npub struct Disk;",
    );
    edit(
        "beta/src/lib.rs",
        "pub fn direct() -> u32 {\n    helper()",
        "pub fn direct() -> u32 {\n    alpha::added()",
    );
    edit(
        "beta/src/lib.rs",
        "disks.iter().map(Disk::get)",
        "disks.iter().map(Disk::load_value)",
    );
    edit(
        "alpha/src/lib.rs",
        "    fn read(&self) -> u32 {",
        "    pub fn load_value(&self) -> u32 {\n        self.get()\n    }\n    fn read(&self) -> u32 {",
    );

    // Before any analyzer answers, syntax rules stand in for the edited files, and only those.
    let (graph, files) = index_at(&scratch);
    let indexes = load_or_generate(
        &scratch,
        &cache,
        &files,
        &mut Analyzers::default(),
        Refresh::Saved,
    );
    let report = link(&graph, &files, &indexes);
    assert_eq!((report.live_files, report.syntax_files), (0, 2));
    let saved = edges(&graph, &files);

    let (graph, files) = index_at(&scratch);
    let indexes = load_or_generate(&scratch, &cache, &files, &mut analyzers, Refresh::Edits);
    let report = link(&graph, &files, &indexes);
    assert_eq!(report.live_files, 2, "both edited files go to the analyzer");
    let live = edges(&graph, &files);

    let (graph, files) = index_at(&scratch);
    let indexes = load_or_generate(&scratch, &cache, &files, &mut analyzers, Refresh::Full);
    let report = link(&graph, &files, &indexes);
    assert_eq!(report.live_files, 0);
    let full = edges(&graph, &files);
    let _ = std::fs::remove_dir_all(&scratch);

    let wrong: Vec<_> = saved[0].difference(&full[0]).collect();
    assert!(wrong.is_empty(), "calls syntax got wrong: {wrong:#?}");
    assert_eq!(live[0], full[0], "calls");
    assert_eq!(live[2], full[2], "implementations");
    // Only a value among a macro's tokens has to wait for the next index.
    let missing: Vec<_> = full[1].difference(&live[1]).collect();
    assert_eq!(
        missing,
        [&(
            "beta::value_in_macro".to_string(),
            "beta::helper".to_string()
        )]
    );
    assert!(live[1].difference(&full[1]).next().is_none());
}

fn find(graph: &CodeGraph, files: &[ProjectFile], wanted: &str) -> SymbolId {
    files
        .iter()
        .flat_map(|f| graph.symbols_in_file(f.file_id))
        .find(|s| {
            label(s, files) == wanted
                && matches!(
                    s.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Constant
                )
        })
        .unwrap_or_else(|| panic!("no symbol {wanted}"))
        .id
}
