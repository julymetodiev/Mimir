//! Edited files: what a running `gopls` gives against what a new index gives (GO3).

use latt_resolve::{Refresh, SemanticSource, link};
use latt_resolve_go::GoSource;

use crate::expected::SOURCES;
use crate::support::{all_edges, copy_of, index_at, tools_installed, workspace};

const KINDS: [&str; 6] = [
    "calls",
    "values",
    "constants",
    "implementations",
    "types",
    "fields",
];

#[test]
fn edited_files_match_a_new_index() {
    if !tools_installed(&[
        ("go", "version"),
        ("scip-go", "--version"),
        ("gopls", "version"),
    ]) {
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("project");
    copy_of(&workspace(), &project, &SOURCES);
    copy_of(&workspace(), &project, &["go.mod"]);
    let edit = |path: &str, from: &str, to: &str| {
        let text = std::fs::read_to_string(project.join(path)).unwrap();
        assert!(text.contains(from), "{from}");
        std::fs::write(project.join(path), text.replacen(from, to, 1)).unwrap();
    };

    let cache = scratch.path().join("cache");
    let source = GoSource::default();
    let (_, files) = index_at(&project, &SOURCES);
    source.load(&project, &cache, &files, Refresh::Edits);

    // Lines move in two files, a new function, a call of it from another file, and a method that
    // makes a new implementation.
    edit(
        "shapes.go",
        "func helper() int { return 1 }",
        "// Moved.\n\nfunc added() int { return helper() }\n\nfunc helper() int { return 1 }",
    );
    edit(
        "uses.go",
        "func direct() int { return helper() }",
        "// Moved.\nfunc direct() int { return added() }",
    );
    edit(
        "shapes.go",
        "type MyInt int",
        "type Other struct{}\n\nfunc (o Other) Area() float64 { return 0 }\n\ntype MyInt int",
    );

    let (graph, files) = index_at(&project, &SOURCES);
    let indexes = source.load(&project, &cache, &files, Refresh::Edits);
    let report = link(&graph, &files, &indexes);
    assert_eq!(report.live_files, 2, "both edited files go to gopls");
    let live = all_edges(&graph, &files);

    let (graph, files) = index_at(&project, &SOURCES);
    let indexes = source.load(&project, &cache, &files, Refresh::Full);
    let report = link(&graph, &files, &indexes);
    assert_eq!(report.live_files, 0);
    let full = all_edges(&graph, &files);

    for (kind, name) in KINDS.iter().enumerate() {
        assert_eq!(live[kind], full[kind], "{name}");
    }
}
