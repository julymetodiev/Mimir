//! Two modules of a `go.work`: a call between them has its edge (GO5).

use mimir_graph::EdgeKind;
use mimir_resolve::{Refresh, SemanticSource, link};
use mimir_resolve_go::GoSource;

use crate::support::{edges_of_kind, expected, index_at, tools_installed};

pub const SOURCES: [&str; 2] = ["a/a.go", "b/b.go"];

pub fn write_two_modules(project: &std::path::Path) {
    for (path, text) in [
        ("go.work", "go 1.22\n\nuse (\n\t./a\n\t./b\n)\n"),
        ("a/go.mod", "module example.com/a\n\ngo 1.22\n"),
        (
            "a/a.go",
            "package a\n\ntype Object interface {\n\tGetName() string\n}\n\nfunc F() int { return 1 }\n",
        ),
        (
            "b/go.mod",
            "module example.com/b\n\ngo 1.22\n\nrequire example.com/a v0.0.0\n",
        ),
        (
            "b/b.go",
            "package b\n\nimport \"example.com/a\"\n\ntype T struct{}\n\nfunc (t T) GetName() string { return \"\" }\n\nfunc G(o a.Object) string {\n\ta.F()\n\treturn o.GetName()\n}\n",
        ),
    ] {
        std::fs::create_dir_all(project.join(path).parent().unwrap()).unwrap();
        std::fs::write(project.join(path), text).unwrap();
    }
}

#[test]
fn a_call_across_modules_has_its_edge() {
    if !tools_installed(&[("go", "version"), ("scip-go", "--version")]) {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    write_two_modules(project.path());
    let (graph, files) = index_at(project.path(), &SOURCES);
    let cache = tempfile::tempdir().unwrap();
    let indexes = GoSource::default().load(project.path(), cache.path(), &files, Refresh::Edits);
    assert_eq!(indexes.len(), 2, "each module has an index");
    link(&graph, &files, &indexes);

    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::Calls),
        expected(&[("b::G", "a::F"), ("b::G", "a::Object::GetName")])
    );
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::Implements),
        expected(&[("b::T::GetName", "a::Object::GetName")])
    );
}

#[test]
fn without_the_tools_a_module_still_says_what_it_is_imported_as() {
    // Syntax rules go from an import to a directory by this, so they hold across modules too.
    let project = tempfile::tempdir().unwrap();
    write_two_modules(project.path());
    let (graph, files) = index_at(project.path(), &SOURCES);
    let indexes = [
        mimir_resolve::SemanticIndex {
            root: "a".into(),
            import_path: Some("example.com/a".to_string()),
            ..Default::default()
        },
        mimir_resolve::SemanticIndex {
            root: "b".into(),
            import_path: Some("example.com/b".to_string()),
            ..Default::default()
        },
    ];
    link(&graph, &files, &indexes);
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::Calls),
        expected(&[("b::G", "a::F")]),
        "no methods"
    );
}
