//! How what an index says becomes edges, with indexes written by hand: the forms `scip-go` gives across
//! modules, and files edited since. This needs no tools.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use latt_graph::EdgeKind;
use latt_resolve::{Occurrence, SemanticIndex, link};

use crate::support::{edges_of_kind, expected, index_at};

const DEFINITION: u64 = 1;

fn at(symbol: &str, line: u32, column: u32, name: &str, roles: u64) -> Occurrence {
    Occurrence {
        line,
        column,
        end_line: line,
        end_column: column + name.len() as u32,
        symbol: symbol.to_string(),
        roles,
    }
}

fn index(root: &str, documents: Vec<(&str, Vec<Occurrence>)>) -> SemanticIndex {
    SemanticIndex {
        root: PathBuf::from(root),
        documents: documents
            .into_iter()
            .map(|(path, occurrences)| (PathBuf::from(path), occurrences))
            .collect::<HashMap<_, _>>(),
        ..SemanticIndex::default()
    }
}

/// Two modules: `b` calls `a`'s function, and an interface's method whose symbol is in the other form.
fn two_modules() -> (tempfile::TempDir, [SemanticIndex; 2]) {
    let project = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "a/a.go",
            "package a\n\nfunc F() int { return 1 }\n\ntype Object interface {\n\tGetName() string\n}\n",
        ),
        (
            "b/b.go",
            "package b\n\nfunc G(o a.Object) string {\n\ta.F()\n\treturn o.GetName()\n}\n\ntype T struct{}\n\nfunc (t T) GetName() string { return \"\" }\n",
        ),
    ] {
        std::fs::create_dir_all(project.path().join(path).parent().unwrap()).unwrap();
        std::fs::write(project.path().join(path), text).unwrap();
    }
    let f = "scip-go gomod example.com/a . a/F().";
    let object = "scip-go gomod example.com/a . a/Object#";
    let in_a = index(
        "a",
        vec![(
            "a.go",
            vec![
                at(f, 2, 5, "F", DEFINITION),
                at(object, 4, 5, "Object", DEFINITION),
                at(
                    "scip-go gomod example.com/a . a/Object#GetName.",
                    5,
                    1,
                    "GetName",
                    DEFINITION,
                ),
            ],
        )],
    );
    let t_get_name = "scip-go gomod example.com/b . b/T#GetName().";
    let mut in_b = index(
        "b",
        vec![(
            "b.go",
            vec![
                at(
                    "scip-go gomod example.com/b . b/G().",
                    2,
                    5,
                    "G",
                    DEFINITION,
                ),
                at(object, 2, 11, "Object", 0),
                at(f, 3, 3, "F", 0),
                // Where it is used in another module, an interface's method ends in `().`.
                at(
                    "scip-go gomod example.com/a . a/Object#GetName().",
                    4,
                    10,
                    "GetName",
                    0,
                ),
                at("scip-go gomod example.com/b . b/T#", 7, 5, "T", DEFINITION),
                at(t_get_name, 9, 11, "GetName", DEFINITION),
            ],
        )],
    );
    in_b.implementations = vec![
        (
            t_get_name.to_string(),
            "scip-go gomod example.com/a . a/Object#GetName().".to_string(),
        ),
        // A pair of types gives no edge.
        (
            "scip-go gomod example.com/b . b/T#".to_string(),
            object.to_string(),
        ),
    ];
    (project, [in_a, in_b])
}

#[test]
fn definitions_are_found_across_the_indexes_of_modules() {
    let (project, indexes) = two_modules();
    let (graph, files) = index_at(project.path(), &["a/a.go", "b/b.go"]);
    let report = link(&graph, &files, &indexes);

    assert_eq!(report.semantic_files, 2);
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::Calls),
        expected(&[("b::G", "a::F"), ("b::G", "a::Object::GetName")])
    );
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::Implements),
        expected(&[("b::T::GetName", "a::Object::GetName")])
    );
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesType),
        expected(&[("b::G", "a::Object")])
    );
}

#[test]
fn a_pair_with_a_side_edited_is_left_to_the_server() {
    let (project, mut indexes) = two_modules();
    let (graph, files) = index_at(project.path(), &["a/a.go", "b/b.go"]);
    indexes[1].unanswered.insert(PathBuf::from("b/b.go"));
    let report = link(&graph, &files, &indexes);

    assert_eq!(report.syntax_files, 1);
    assert!(edges_of_kind(&graph, &files, EdgeKind::Implements).is_empty());
}

/// A method and a field share a name in a file, so an edited file's symbols are not found by order.
#[test]
fn an_edited_file_tells_a_field_from_a_method_of_one_name() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("a.go"),
        "package a\n\n// Lines moved.\n\ntype A struct {\n\tName string\n}\n\ntype B struct{}\n\nfunc (b B) Name() string { return \"\" }\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("c.go"),
        "package a\n\nfunc use(a A, b B) string {\n\ta.Name = \"\"\n\treturn b.Name()\n}\n",
    )
    .unwrap();
    let (graph, files) = index_at(project.path(), &["a.go", "c.go"]);
    let name_of_a = "scip-go gomod x . a/A#Name.";
    let name_of_b = "scip-go gomod x . a/B#Name().";
    let mut only = index(
        "",
        vec![
            (
                "a.go",
                vec![
                    // Positions from before the edit.
                    at("scip-go gomod x . a/A#", 2, 5, "A", DEFINITION),
                    at(name_of_a, 3, 1, "Name", DEFINITION),
                    at("scip-go gomod x . a/B#", 6, 5, "B", DEFINITION),
                    at(name_of_b, 8, 11, "Name", DEFINITION),
                ],
            ),
            (
                "c.go",
                vec![
                    at("scip-go gomod x . a/use().", 2, 5, "use", DEFINITION),
                    at(name_of_a, 3, 3, "Name", 0),
                    at(name_of_b, 4, 10, "Name", 0),
                ],
            ),
        ],
    );
    only.unanswered.insert(PathBuf::from(Path::new("a.go")));
    link(&graph, &files, std::slice::from_ref(&only));

    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::Calls),
        expected(&[("use", "B::Name")])
    );
    assert_eq!(
        edges_of_kind(&graph, &files, EdgeKind::UsesField),
        expected(&[("use", "A::Name")])
    );
}
