use super::*;
use latt_types::{
    Confidence, IdGenerator, Language, QualifiedName, Range, Span, SymbolKind, Visibility,
};

fn ids() -> IdGenerator {
    IdGenerator::new()
}

fn add_edge(graph: &CodeGraph, from: SymbolId, to: SymbolId, data: EdgeData) {
    let mut edges = graph.current.load().all_edges();
    edges.push((from, to, data));
    graph.replace_edges(edges);
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
            add_edge(
                &graph,
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
        let callers = graph
            .trace(by_name["root"], options, &HashMap::new())
            .unwrap()
            .callers;
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
        add_edge(
            &graph,
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
    let callers = graph
        .trace(by_name["kind"], options, &HashMap::new())
        .unwrap()
        .callers;
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

    add_edge(
        &graph,
        a_id,
        b_id,
        EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High),
    );
    add_edge(
        &graph,
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
            &HashMap::new(),
        )
        .unwrap();

    assert_eq!(result.root.name, "b");
    assert_eq!(result.callers.len(), 1);
    assert_eq!(result.callers[0].symbol.name, "a");
    assert_eq!(result.callees.len(), 1);
    assert_eq!(result.callees[0].symbol.name, "c");
}

#[test]
fn two_symbols_with_one_id_are_both_kept() {
    let graph = CodeGraph::new();
    let ids = ids();
    let mut first = make_symbol("first", &ids, FileId::new(1));
    let mut second = make_symbol("second", &ids, FileId::new(2));
    first.id = SymbolId::new(7);
    second.id = SymbolId::new(7);
    graph.add_symbols(vec![first, second]);

    let first = graph.find_symbol("first");
    let second = graph.find_symbol("second");
    assert_eq!(first[0].id, SymbolId::new(7));
    assert_eq!(second[0].id, SymbolId::new(8));
}

#[test]
fn callers_come_in_order_of_path() {
    let graph = CodeGraph::new();
    let ids = ids();
    let (in_b, in_a) = (FileId::new(1), FileId::new(2));
    let root = graph.add_symbol(make_symbol("root", &ids, FileId::new(3)));
    let edge = || EdgeData::new(EdgeKind::Calls, Span::new(0, 1), Confidence::High);
    for (name, file) in [("from_b", in_b), ("from_a", in_a)] {
        let caller = graph.add_symbol(make_symbol(name, &ids, file));
        add_edge(&graph, caller, root, edge());
    }
    let paths = HashMap::from([(in_b, PathBuf::from("b.rs")), (in_a, PathBuf::from("a.rs"))]);

    let options = TraceOptions {
        max_depth: Some(1),
        include_callers: true,
        include_callees: false,
    };
    let callers = graph.trace(root, options, &paths).unwrap().callers;
    let names: Vec<&str> = callers.iter().map(|n| n.symbol.name.as_str()).collect();
    assert_eq!(names, ["from_a", "from_b"]);
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
