//! Resolution against a small workspace whose calls were listed by hand, so neither
//! resolver is graded by itself. `fixtures/resolve-workspace` holds the traps: same names
//! across types and crates, calls inside macros, trait methods, nested functions.

mod edits;
mod expected;
mod support;

use expected::{
    CALLS, CONSTANT_READS, DATA_USES_BY_SYNTAX, FIELD_USES, IMPLEMENTS, IMPLEMENTS_BY_SYNTAX,
    NEEDS_TYPES, SOURCES, TYPE_USES, VALUES, VALUES_NEED_TYPES,
};
use std::collections::BTreeSet;

use mimir_graph::{EdgeKind, TraceRelationship};
use mimir_resolve::{Refresh, SemanticIndex, link};
use mimir_resolve_ra::{Analyzers, load_or_generate};
use support::{
    call_edges, callees_of, edges_of_kind, expected, find, implementation_edges, index, label,
    trace_options, workspace,
};

#[test]
fn syntax_rules_never_guess() {
    let (graph, files) = index();
    let indexes: [SemanticIndex; 0] = [];
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
    // Types and fields: what syntax links is right, and what it cannot know it leaves out.
    let data: BTreeSet<_> = edges_of_kind(&graph, &files, EdgeKind::UsesType)
        .union(&edges_of_kind(&graph, &files, EdgeKind::UsesField))
        .cloned()
        .collect();
    let all_data: BTreeSet<_> = expected(TYPE_USES)
        .union(&expected(FIELD_USES))
        .cloned()
        .collect();
    let wrong: Vec<_> = data.difference(&all_data).collect();
    assert!(wrong.is_empty(), "uses that are not there: {wrong:#?}");
    let reached: Vec<_> = expected(DATA_USES_BY_SYNTAX)
        .into_iter()
        .filter(|edge| !data.contains(edge))
        .collect();
    assert!(reached.is_empty(), "uses syntax should reach: {reached:#?}");
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
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesType),
        expected(TYPE_USES)
    );
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesField),
        expected(FIELD_USES)
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

    // Two fields share a name, and each has its own users.
    let users = |wanted: &str| {
        let id = find(&graph, &files, wanted);
        let trace = graph.trace(id, trace_options(true, false, 1)).unwrap();
        let mut users: Vec<_> = trace
            .callers
            .iter()
            .map(|n| {
                assert_eq!(n.relationship, TraceRelationship::User);
                label(&n.symbol, &files)
            })
            .collect();
        users.sort();
        users
    };
    assert_eq!(
        users("alpha::Config::limit"),
        ["alpha::Config::own_limit", "beta::reads_field"]
    );
    assert_eq!(
        users("alpha::Other::limit"),
        ["beta::builds_other", "beta::shorthand_field"]
    );
    assert_eq!(users("alpha::Alias"), ["beta::through_alias"]);
    let holder = find(&graph, &files, "beta::reads_field");
    let trace = graph.trace(holder, trace_options(false, true, 1)).unwrap();
    assert!(
        trace.callees.is_empty(),
        "a type or a field is not a callee"
    );

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
