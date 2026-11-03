//! Every row of the table in `docs/PYTHON.md`, section 3, checked by hand against a running `ty`.
//! Without `ty` on the path it is skipped with a message, like the other servers' tests.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_parse_core::LanguageAdapter;
use mimir_parse_python::PythonAdapter;
use mimir_resolve::{ProjectFile, Refresh, SemanticSource, link};
use mimir_resolve_lsp::{AskedSource, ty};
use mimir_types::{FileId, IdGenerator, Symbol};

const SOURCES: [&str; 3] = ["lib.py", "main.py", "reexport.py"];

type Edge = (String, String);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ty/project")
}

fn build(root: &Path, names: &[&str]) -> (CodeGraph, Vec<ProjectFile>) {
    let (graph, ids) = (CodeGraph::new(), IdGenerator::new());
    let adapter = PythonAdapter;
    let mut files = Vec::new();
    for path in names {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
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

fn edges(graph: &CodeGraph, files: &[ProjectFile], kind: EdgeKind) -> BTreeSet<Edge> {
    let label = |symbol: &Symbol| symbol.qualified_name.segments().join(".");
    let mut found = BTreeSet::new();
    for file in files {
        for symbol in graph.symbols_in_file(file.file_id) {
            for (target, edge) in graph.get_outgoing(symbol.id) {
                if edge.kind == kind {
                    found.insert((label(&symbol), label(&target)));
                }
            }
        }
    }
    found
}

fn list(edges: &[(&str, &str)]) -> BTreeSet<Edge> {
    edges
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn ty_is_there() -> bool {
    let there = std::process::Command::new("ty")
        .arg("--version")
        .output()
        .is_ok();
    if !there {
        eprintln!("skipped: ty is not installed");
    }
    there
}

fn linked(root: &Path, names: &[&str], source: &AskedSource) -> (CodeGraph, Vec<ProjectFile>) {
    let (graph, files) = build(root, names);
    // What is saved must stay out of the project: it is tracked by git here.
    let cache = tempfile::TempDir::new().unwrap();
    let indexes = source.load(root, cache.path(), &files, Refresh::Edits);
    link(&graph, &files, &indexes);
    (graph, files)
}

#[test]
fn every_row_of_the_table_holds() {
    if !ty_is_there() {
        return;
    }
    let (graph, files) = linked(&root(), &SOURCES, &ty());

    let calls = list(&[
        ("Circle.make", "Circle.__init__"),
        ("Sub.area", "Circle.area"),
        ("run", "Circle.__init__"),
        ("run", "Circle.area"),
        ("run", "Circle.make"),
        ("run", "Circle.helper"),
        ("run", "Circle.size"),
        ("run", "Shape.area"),
        ("run", "Sub"),
        ("run", "Sub.area"),
        ("run", "Point"),
        ("run", "over"),
        ("run", "decorated"),
        ("run", "plain"),
        ("run", "make_it"),
        ("make_it", "Circle.__init__"),
        ("decorated", "deco"),
        ("run", "Square"),
        ("top", "plain"),
    ]);
    assert_eq!(edges(&graph, &files, EdgeKind::Calls), calls);

    let values = list(&[
        ("run", "Circle.area"),
        ("run", "Square.area"),
        ("run", "plain"),
        ("alias", "plain"),
    ]);
    assert_eq!(
        edges(&graph, &files, EdgeKind::UsesAsValue),
        values,
        "values"
    );

    let implemented = list(&[("Sub.area", "Circle.area")]);
    assert_eq!(edges(&graph, &files, EdgeKind::Implements), implemented);

    let types = edges(&graph, &files, EdgeKind::UsesType);
    for used in ["Shape", "Circle", "Square"] {
        assert!(types.iter().any(|(_, t)| t == used), "{used} in {types:?}");
    }
    let fields = edges(&graph, &files, EdgeKind::UsesField);
    assert!(
        fields.contains(&("Circle.area".into(), "Circle.r".into())),
        "{fields:?}"
    );
}

/// A read runs the getter and a write the setter. Both are named `size`, so their lines tell them
/// apart, and neither is only possible.
#[test]
fn a_read_runs_the_getter_and_a_write_the_setter() {
    if !ty_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("a.py"),
        "class C:\n    @property\n    def size(self) -> int:\n        return 1\n\n    @size.setter\n    def size(self, v: int) -> None:\n        pass\n\n\ndef read(c: C):\n    return c.size\n\n\ndef write(c: C):\n    c.size = 2\n",
    )
    .unwrap();
    let (graph, files) = linked(&root, &["a.py"], &ty());
    let targets = |from: &str| -> BTreeSet<u32> {
        graph
            .symbols_in_file(files[0].file_id)
            .into_iter()
            .filter(|s| s.name == from)
            .flat_map(|s| graph.get_outgoing(s.id))
            .filter(|(target, edge)| edge.kind == EdgeKind::Calls && target.name == "size")
            .map(|(target, _)| target.range.start_line)
            .collect()
    };
    let (getter, setter) = (2, 6);
    assert_eq!(targets("read"), [getter].into());
    assert_eq!(targets("write"), [setter].into());
}

/// An edit changes what a site in another file means, and the graph follows.
#[test]
fn an_edit_is_seen_by_the_files_that_import_it() {
    if !ty_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("main.py"),
        "from lib import make\n\n\ndef run():\n    make().go()\n",
    )
    .unwrap();
    let write_lib = |returned: &str| {
        let text = format!(
            "class A:\n    def go(self):\n        pass\n\n\nclass B:\n    def go(self):\n        pass\n\n\ndef make() -> {returned}:\n    return {returned}()\n"
        );
        std::fs::write(root.join("lib.py"), text).unwrap();
    };
    write_lib("A");
    let source = ty();
    let calls = |source: &AskedSource| {
        let (graph, files) = linked(&root, &["lib.py", "main.py"], source);
        edges(&graph, &files, EdgeKind::Calls)
            .into_iter()
            .filter(|(from, _)| from == "run")
            .map(|(_, to)| to)
            .collect::<BTreeSet<_>>()
    };
    let names = |list: &[&str]| list.iter().map(|n| n.to_string()).collect::<BTreeSet<_>>();
    assert_eq!(calls(&source), names(&["A.go", "make"]));

    // Only `lib.py` changes, and `run` in `main.py` now means another `go`.
    write_lib("B");
    assert_eq!(calls(&source), names(&["B.go", "make"]));

    // And it is what a clean start gives.
    assert_eq!(calls(&ty()), names(&["B.go", "make"]));
}

/// Only a project with files of the language and a list of packages is told.
#[test]
fn a_project_without_its_packages_is_told_so() {
    if !ty_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.py"), "def f():\n    pass\n").unwrap();
    let source = ty();
    linked(&root, &["a.py"], &source);
    assert!(source.notes(&root).is_empty(), "no list of packages");

    std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"a\"\n").unwrap();
    assert_eq!(source.notes(&root).len(), 1);

    std::fs::create_dir(root.join(".venv")).unwrap();
    assert!(source.notes(&root).is_empty(), "packages are there");

    let unused = ty();
    std::fs::remove_dir(root.join(".venv")).unwrap();
    assert!(unused.notes(&root).is_empty(), "it answers for no file yet");
}

#[test]
fn without_answers_only_syntax_rules_apply() {
    let (graph, files) = build(&root(), &SOURCES);
    link(&graph, &files, &[]);
    let calls = edges(&graph, &files, EdgeKind::Calls);
    assert!(
        calls.contains(&("run".into(), "over".into())),
        "an import of the project: {calls:?}"
    );
    assert!(
        !calls.iter().any(|(_, to)| to.contains('.')),
        "no methods: {calls:?}"
    );
}
