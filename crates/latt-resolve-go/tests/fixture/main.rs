//! Resolution against a small module whose edges were listed by hand (GO1), edits against a new index
//! (GO3), and two modules of a `go.work` (GO5). What needs the tools is skipped without them; syntax
//! rules and how an index is read need none.

mod edits;
mod expected;
mod hand_made;
mod modules;
mod support;

use expected::{
    CALLS, CALLS_BY_SYNTAX, CONSTANT_READS, FIELD_USES, IMPLEMENTS, SOURCES, TYPE_USES, VALUES,
    VALUES_BY_SYNTAX,
};
use latt_graph::EdgeKind;
use latt_resolve::{Refresh, SemanticSource, link};
use latt_resolve_go::GoSource;
use support::{edges_of_kind, expected, index_at, tools_installed, workspace};

#[test]
fn syntax_rules_never_guess() {
    let (graph, files) = index_at(&workspace(), &SOURCES);
    // Only what a module is imported as: no index and no server.
    let indexes = [latt_resolve::SemanticIndex {
        import_path: Some("example.com/fixture".to_string()),
        ..Default::default()
    }];
    let report = link(&graph, &files, &indexes);

    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::Calls),
        expected(CALLS_BY_SYNTAX)
    );
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesAsValue),
        expected(VALUES_BY_SYNTAX)
    );
    assert_eq!(report.syntax_files, SOURCES.len());
    assert_eq!(report.semantic_files, 0);
}

#[test]
fn the_index_gives_every_edge_of_the_list() {
    if !tools_installed(&[("go", "version"), ("scip-go", "--version")]) {
        return;
    }
    let (graph, files) = index_at(&workspace(), &SOURCES);
    let cache = tempfile::tempdir().unwrap();
    let source = GoSource::default();
    let indexes = source.load(&workspace(), cache.path(), &files, Refresh::Edits);
    assert_eq!(indexes.len(), 1, "the fixture is one module");
    let report = link(&graph, &files, &indexes);

    let notes = source.notes(&workspace());
    assert!(
        notes
            .iter()
            .any(|note| note.starts_with("Go: 2 files are outside the build")),
        "{notes:?}"
    );
    assert_eq!(report.syntax_files, 2, "win_windows.go and tagged.go");

    for (kind, wanted, name) in [
        (EdgeKind::Calls, CALLS, "calls"),
        (EdgeKind::UsesAsValue, VALUES, "values"),
        (EdgeKind::Implements, IMPLEMENTS, "implementations"),
        (EdgeKind::UsesConstant, CONSTANT_READS, "constants"),
        (EdgeKind::UsesType, TYPE_USES, "types"),
        (EdgeKind::UsesField, FIELD_USES, "fields"),
    ] {
        let found = edges_of_kind(&graph, &files, kind);
        let wanted = expected(wanted);
        let extra: Vec<_> = found.difference(&wanted).collect();
        let missing: Vec<_> = wanted.difference(&found).collect();
        assert!(
            extra.is_empty() && missing.is_empty(),
            "{name}: extra {extra:#?}, missing {missing:#?}"
        );
    }
}

#[test]
fn a_package_that_does_not_compile_is_reported() {
    if !tools_installed(&[("go", "version"), ("scip-go", "--version")]) {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("go.mod"), "module x\n\ngo 1.22\n").unwrap();
    std::fs::write(
        project.path().join("a.go"),
        "package x\n\nfunc F() int { return undefined() }\n\nfunc G() int { return H() }\n\nfunc H() int { return 1 }\n",
    )
    .unwrap();
    let (graph, files) = index_at(project.path(), &["a.go"]);
    let cache = tempfile::tempdir().unwrap();
    let source = GoSource::default();
    let indexes = source.load(project.path(), cache.path(), &files, Refresh::Edits);
    link(&graph, &files, &indexes);

    let notes = source.notes(project.path());
    assert!(
        notes.iter().any(|note| note.contains("undefined")),
        "{notes:?}"
    );
    let calls = edges_of_kind(&graph, &files, EdgeKind::Calls);
    assert_eq!(calls, expected(&[("G", "H")]), "what resolved in it stays");
}
