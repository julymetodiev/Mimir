//! Every row of the table in `docs/TYPESCRIPT.md`, section 3, checked by hand against a running
//! `tsrs`. Without `tsrs` on the path it is skipped with a message, like the `rust-analyzer` tests.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_parse_core::LanguageAdapter;
use mimir_parse_ts::ScriptAdapter;
use mimir_resolve::{ProjectFile, Refresh, SemanticSource, link};
use mimir_resolve_tsrs::Tsrs;
use mimir_types::{FileId, IdGenerator, Language, Symbol};

const SOURCES: [&str; 5] = ["types.ts", "lib.ts", "main.ts", "widget.tsx", "plain.js"];

type Edge = (String, String);

fn adapter(path: &str) -> ScriptAdapter {
    match Language::from_path(Path::new(path)) {
        Language::Tsx => ScriptAdapter::tsx(),
        Language::JavaScript => ScriptAdapter::javascript(),
        _ => ScriptAdapter::typescript(),
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture/project")
}

fn build(root: &Path) -> (CodeGraph, Vec<ProjectFile>) {
    let (graph, ids) = (CodeGraph::new(), IdGenerator::new());
    let mut files = Vec::new();
    for path in SOURCES {
        let adapter = adapter(path);
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

fn label(symbol: &Symbol) -> String {
    symbol.qualified_name.segments().join(".")
}

fn edges(graph: &CodeGraph, files: &[ProjectFile], kind: EdgeKind) -> BTreeSet<Edge> {
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

fn tsrs_is_there() -> bool {
    let there = std::process::Command::new("tsrs")
        .arg("--version")
        .output()
        .is_ok();
    if !there {
        eprintln!("skipped: tsrs is not installed");
    }
    there
}

fn linked(root: &Path, source: &Tsrs) -> (CodeGraph, Vec<ProjectFile>) {
    let (graph, files) = build(root);
    let indexes = source.load(root, root, &files, Refresh::Edits);
    link(&graph, &files, &indexes);
    (graph, files)
}

#[test]
fn every_row_of_the_table_holds() {
    if !tsrs_is_there() {
        return;
    }
    let (graph, files) = linked(&root(), &Tsrs::default());

    let calls = list(&[
        ("Circle.constructor", "Base"),
        ("Circle.area", "helper"),
        ("Circle.reveal", "Circle.#secret"),
        ("run", "Circle.constructor"),
        ("run", "Circle.area"),
        ("run", "Shape.area"),
        ("run", "helper"),
        ("run", "over"),
        ("run", "arrow"),
        ("run", "Plain"),
        ("accessors", "Circle.size"),
        ("both", "Circle.size"),
        ("second", "first"),
        ("Old.run", "second"),
    ]);
    assert_eq!(edges(&graph, &files, EdgeKind::Calls), calls);

    let values = list(&[
        ("run", "helper"),
        ("holder", "helper"),
        ("App", "Comp"),
        ("either", "Circle.area"),
        ("chain", "Circle.area"),
        ("either", "Square.area"),
        ("chain", "Square.area"),
    ]);
    let found = edges(&graph, &files, EdgeKind::UsesAsValue);
    assert_eq!(found, values, "values");

    let implemented = list(&[("Circle.area", "Shape.area")]);
    assert_eq!(edges(&graph, &files, EdgeKind::Implements), implemented);

    let constants = edges(&graph, &files, EdgeKind::UsesConstant);
    assert_eq!(constants, list(&[("run", "LIMIT")]), "constants");

    let fields = edges(&graph, &files, EdgeKind::UsesField);
    assert!(
        fields.contains(&("fields".into(), "Shape.name".into())),
        "{fields:?}"
    );
    assert!(
        fields.contains(&("Circle.constructor".into(), "Circle.r".into())),
        "{fields:?}"
    );

    let types = edges(&graph, &files, EdgeKind::UsesType);
    for used in ["Shape", "Id", "Color.Red", "Circle"] {
        assert!(types.iter().any(|(_, t)| t == used), "{used} in {types:?}");
    }
}

/// What a getter and a setter each run. Both are named `size`, so their lines tell them apart.
#[test]
fn a_read_runs_the_getter_and_a_write_the_setter() {
    if !tsrs_is_there() {
        return;
    }
    let (graph, files) = linked(&root(), &Tsrs::default());
    let targets = |from: &str| -> BTreeSet<u32> {
        files
            .iter()
            .flat_map(|f| graph.symbols_in_file(f.file_id))
            .filter(|s| s.name == from)
            .flat_map(|s| graph.get_outgoing(s.id))
            .filter(|(target, edge)| edge.kind == EdgeKind::Calls && target.name == "size")
            .map(|(target, _)| target.range.start_line)
            .collect()
    };
    let (getter, setter) = (13, 16);
    assert_eq!(targets("accessors"), [getter, setter].into());
    assert_eq!(targets("both"), [getter, setter].into());
}

/// An edit changes what a site in another file means, and the graph follows.
#[test]
fn an_edit_is_seen_by_the_files_that_import_it() {
    if !tsrs_is_there() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    for (name, text) in [
        (
            "types.ts",
            "export interface Shape {\n}\nexport interface Other {\n}\n",
        ),
        ("lib.ts", "export const empty = 1;\n"),
        ("main.ts", "export function run() {}\n"),
        ("widget.tsx", "export function w() {}\n"),
        ("plain.js", "function p() {}\n"),
    ] {
        std::fs::write(root.join(name), text).unwrap();
    }
    let write_lib = |text: &str| std::fs::write(root.join("lib.ts"), text).unwrap();
    std::fs::write(
        root.join("main.ts"),
        "import { make } from \"./lib\";\nexport function run() {\n  make().go();\n}\n",
    )
    .unwrap();
    write_lib(
        "export class A {\n  go() {}\n}\nexport class B {\n  go() {}\n}\nexport function make(): A {\n  return new A();\n}\n",
    );
    let source = Tsrs::default();
    let calls = |source: &Tsrs| {
        let (graph, files) = linked(&root, source);
        edges(&graph, &files, EdgeKind::Calls)
            .into_iter()
            .filter(|(from, _)| from == "run")
            .map(|(_, to)| to)
            .collect::<BTreeSet<_>>()
    };
    let names = |list: &[&str]| list.iter().map(|n| n.to_string()).collect::<BTreeSet<_>>();
    assert_eq!(calls(&source), names(&["A.go", "make"]));

    // Only `lib.ts` changes, and `run` in `main.ts` now means another `go`.
    write_lib(
        "export class A {\n  go() {}\n}\nexport class B {\n  go() {}\n}\nexport function make(): B {\n  return new B();\n}\n",
    );
    assert_eq!(calls(&source), names(&["B.go", "make"]));

    // And it is what a clean start gives.
    assert_eq!(calls(&Tsrs::default()), names(&["B.go", "make"]));
}

#[test]
fn without_answers_only_syntax_rules_apply() {
    let (graph, files) = build(&root());
    link(&graph, &files, &[]);
    let calls = edges(&graph, &files, EdgeKind::Calls);
    assert!(
        calls.contains(&("second".into(), "first".into())),
        "{calls:?}"
    );
    assert!(
        !calls.iter().any(|(_, to)| to.contains('.')),
        "no methods: {calls:?}"
    );
}
