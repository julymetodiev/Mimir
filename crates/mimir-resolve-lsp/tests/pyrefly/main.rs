//! Every row of the table in `docs/PYTHON.md`, section 3, checked by hand against a running `pyrefly`.
//! Without `pyrefly` on the path it is skipped with a message, like the other servers' tests.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_parse_core::LanguageAdapter;
use mimir_parse_python::PythonAdapter;
use mimir_resolve::{ProjectFile, Refresh, SemanticSource, link};
use mimir_resolve_lsp::{AskedSource, pyrefly};
use mimir_types::{FileId, IdGenerator, Symbol};

const SOURCES: [&str; 3] = ["lib.py", "main.py", "reexport.py"];

type Edge = (String, String);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pyrefly/project")
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
            references: adapter.extract_references(&tree, file_id).into(),
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

fn pyrefly_is_there() -> bool {
    let there = std::process::Command::new("pyrefly")
        .arg("--version")
        .output()
        .is_ok();
    if !there {
        eprintln!("skipped: pyrefly is not installed");
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
    if !pyrefly_is_there() {
        return;
    }
    let (graph, files) = linked(&root(), &SOURCES, &pyrefly());

    let calls = list(&[
        ("Sub.area", "Circle.area"),
        ("run", "Circle.__init__"),
        ("run", "Circle.area"),
        ("run", "Circle.make"),
        ("run", "Circle.helper"),
        ("run", "Circle.size"),
        ("run", "Shape.area"),
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
    if !pyrefly_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("a.py"),
        "class C:\n    @property\n    def size(self) -> int:\n        return 1\n\n    @size.setter\n    def size(self, v: int) -> None:\n        pass\n\n\ndef read(c: C):\n    return c.size\n\n\ndef write(c: C):\n    c.size = 2\n",
    )
    .unwrap();
    let (graph, files) = linked(&root, &["a.py"], &pyrefly());
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
    if !pyrefly_is_there() {
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
    let source = pyrefly();
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
    assert_eq!(calls(&pyrefly()), names(&["B.go", "make"]));
}

/// Lines that move in a file move a member inherited from it through another file.
#[test]
fn moved_lines_are_seen_through_a_file_in_between() {
    if !pyrefly_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let lib = "class Base:\n    def go(self):\n        pass\n";
    std::fs::write(root.join("lib.py"), lib).unwrap();
    std::fs::write(
        root.join("mid.py"),
        "from lib import Base\n\n\nclass Mid(Base):\n    pass\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.py"),
        "from mid import Mid\n\n\ndef run():\n    Mid().go()\n",
    )
    .unwrap();
    let source = pyrefly();
    let calls = |source: &AskedSource| {
        let (graph, files) = linked(&root, &["lib.py", "mid.py", "main.py"], source);
        edges(&graph, &files, EdgeKind::Calls)
            .into_iter()
            .filter(|(from, to)| from == "run" && to.ends_with("go"))
            .map(|(_, to)| to)
            .collect::<BTreeSet<_>>()
    };
    let go = BTreeSet::from(["Base.go".to_string()]);
    assert_eq!(calls(&source), go);

    std::fs::write(root.join("lib.py"), format!("# moved\n{lib}")).unwrap();
    assert_eq!(calls(&source), go);
}

/// Only a project with files of the language and a list of packages is told.
#[test]
fn a_project_without_its_packages_is_told_so() {
    if !pyrefly_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.py"), "def f():\n    pass\n").unwrap();
    let source = pyrefly();
    linked(&root, &["a.py"], &source);
    assert!(source.notes(&root).is_empty(), "no list of packages");

    std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"a\"\n").unwrap();
    assert_eq!(source.notes(&root).len(), 1);

    std::fs::create_dir(root.join(".venv")).unwrap();
    assert!(source.notes(&root).is_empty(), "packages are there");

    let unused = pyrefly();
    std::fs::remove_dir(root.join(".venv")).unwrap();
    assert!(unused.notes(&root).is_empty(), "it answers for no file yet");
}

/// A column after other than ASCII is counted differently on the wire, and lands in the same place.
#[test]
fn a_call_after_other_than_ascii_is_found() {
    if !pyrefly_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("a.py"),
        "class Я:\n    def ключ(self) -> int:\n        return 1\n\n\ndef run(я: Я):\n    t = \"ü€😀\"; я.ключ()\n",
    )
    .unwrap();
    let (graph, files) = linked(&root, &["a.py"], &pyrefly());
    let calls = edges(&graph, &files, EdgeKind::Calls);
    assert!(
        calls.contains(&("run".into(), "Я.ключ".into())),
        "{calls:?}"
    );
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
