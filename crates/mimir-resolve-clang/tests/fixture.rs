//! CPP1 of `docs/CPP.md`: the edges of a hand-made project, against a list written by hand. Skipped with a
//! message when `clangd-indexer` is not installed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_parse_c::CFamilyAdapter;
use mimir_parse_core::LanguageAdapter;
use mimir_resolve::{ProjectFile, Refresh, SemanticSource, link};
use mimir_resolve_clang::ClangSource;
use mimir_types::{FileId, IdGenerator, Language, Symbol};

const SOURCES: [&str; 7] = [
    "src/app.cpp",
    "src/main.c",
    "src/other.c",
    "src/shape.cpp",
    "src/shape.hpp",
    "src/util.c",
    "src/util.h",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/c-workspace")
}

fn installed() -> bool {
    let found = std::process::Command::new("clangd-indexer")
        .arg("--version")
        .output()
        .is_ok();
    if !found {
        eprintln!("skipped: clangd-indexer is not installed");
    }
    found
}

/// A copy of the fixture with a database of its own, as a database holds absolute paths.
fn project() -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("src")).unwrap();
    for path in SOURCES {
        std::fs::copy(workspace().join(path), project.path().join(path)).unwrap();
    }
    write_database(
        project.path(),
        &project.path().join("compile_commands.json"),
    );
    project
}

fn write_database(project: &Path, to: &Path) {
    let directory = project.canonicalize().unwrap();
    let entries: Vec<String> = SOURCES
        .iter()
        .filter(|path| !path.ends_with('h') && !path.ends_with("hpp"))
        .map(|path| {
            let (compiler, standard) = if path.ends_with(".c") {
                ("clang", "-std=c17")
            } else {
                ("clang++", "-std=c++20")
            };
            format!(
                r#"{{"directory":"{}","file":"{path}","arguments":["{compiler}","{standard}","-Isrc","-c","{path}"]}}"#,
                directory.display()
            )
        })
        .collect();
    std::fs::write(to, format!("[{}]", entries.join(","))).unwrap();
}

fn parse(project: &Path) -> (CodeGraph, Vec<ProjectFile>) {
    let (graph, ids) = (CodeGraph::new(), IdGenerator::new());
    let mut files = Vec::new();
    for path in SOURCES {
        let source = std::fs::read_to_string(project.join(path)).unwrap();
        let adapter = if Language::from_path(Path::new(path)) == Language::C {
            CFamilyAdapter::c()
        } else {
            CFamilyAdapter::cpp()
        };
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

/// Edges as `file caller` and `callee:line`: two functions of one name are told apart by the line.
fn edges(graph: &CodeGraph, files: &[ProjectFile], kind: EdgeKind) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    for file in files {
        let name = file.path.file_name().unwrap().to_string_lossy();
        for symbol in graph.symbols_in_file(file.file_id) {
            for (target, edge) in graph.get_outgoing(symbol.id) {
                if edge.kind == kind {
                    found.insert((
                        format!("{name} {}", symbol.qualified_name),
                        format!("{}:{}", target.qualified_name, target.range.start_line),
                    ));
                }
            }
        }
    }
    found
}

fn expected(list: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn resolved(project: &Path) -> (CodeGraph, Vec<ProjectFile>, ClangSource) {
    let cache = project.join(".cache-of-mimir");
    let (graph, files) = parse(project);
    let source = ClangSource::default();
    let indexes = source.load(project, &cache, &files, Refresh::Edits);
    link(&graph, &files, &indexes);
    (graph, files, source)
}

#[test]
fn every_edge_of_the_list_holds() {
    if !installed() {
        return;
    }
    let project = project();
    let (graph, files, source) = resolved(project.path());
    assert_eq!(source.notes(project.path()), Vec::<String>::new());
    let calls = edges(&graph, &files, EdgeKind::Calls);
    let want = expected(&[
        // Overloads and a specialization, each to its own.
        ("app.cpp main", "geo::pick:8"),
        ("app.cpp main", "geo::pick:9"),
        ("app.cpp main", "geo::Box::put:10"),
        ("app.cpp main", "geo::Box<char>::put:10"),
        ("app.cpp main", "geo::Box::get:11"),
        ("app.cpp main", "geo::biggest:9"),
        // A virtual call to what is named, and a call on a known type to its own.
        ("app.cpp main", "geo::Shape::area:3"),
        ("app.cpp main", "geo::Circle::area:5"),
        // At the name of a variable, where the index has it.
        ("app.cpp main", "geo::Circle::Circle:4"),
        // Each constructor of a class with several, by what this call runs.
        ("app.cpp main", "geo::Two::Two:12"),
        ("app.cpp main", "geo::Two::Two:13"),
        ("app.cpp main", "geo::Two::Two:14"),
        ("app.cpp main", "geo::Made::Made:15"),
        // What a parser reads as a declaration of a function.
        ("app.cpp main", "geo::Solo::Solo:16"),
        // After text beyond ASCII on its line.
        ("app.cpp main", "geo::measure:7"),
        ("shape.cpp geo::measure", "geo::Shape::area:3"),
        // From a file to another, with a declaration in a header between them.
        ("main.c main", "shared_add:3"),
        ("main.c main", "feature_only:9"),
        ("main.c main", "in_header:7"),
        ("util.c twice", "shared_add:3"),
        // Through a macro, to the function it runs.
        ("main.c main", "twice:4"),
        // A function known only in its file, each to its own though they share a name.
        ("util.c shared_add", "local_one:2"),
        ("other.c other", "local_one:2"),
        ("app.cpp main", "helper:4"),
    ]);
    let missing: Vec<_> = want.difference(&calls).collect();
    let extra: Vec<_> = calls.difference(&want).collect();
    assert!(missing.is_empty(), "missing: {missing:?}");
    assert!(extra.is_empty(), "not on the list: {extra:?}");

    assert_eq!(
        edges(&graph, &files, EdgeKind::Implements),
        expected(&[
            ("shape.cpp geo::Circle::area", "geo::Shape::area:3"),
            ("shape.cpp geo::Square::area", "geo::Shape::area:3"),
        ])
    );
    // A function named is no call.
    assert!(
        edges(&graph, &files, EdgeKind::UsesAsValue)
            .contains(&("main.c main".to_string(), "twice:4".to_string()))
    );
    assert!(
        edges(&graph, &files, EdgeKind::UsesAsValue)
            .contains(&("app.cpp main".to_string(), "helper:4".to_string())),
        "a local function named is no call"
    );
    let fields = edges(&graph, &files, EdgeKind::UsesField);
    assert!(
        fields.contains(&("main.c main".to_string(), "ops::count:4".to_string())),
        "{fields:?}"
    );
    let types = edges(&graph, &files, EdgeKind::UsesType);
    assert!(
        types.contains(&("app.cpp main".to_string(), "geo::Circle:4".to_string())),
        "{types:?}"
    );
}

#[test]
fn clangd_leaves_nothing_in_the_project() {
    if !installed() {
        return;
    }
    let project = project();
    resolved(project.path());
    assert!(!project.path().join(".cache").exists());
}

/// CPP5: whatever the order files are in, the same edges.
#[test]
fn the_order_of_files_changes_nothing() {
    if !installed() {
        return;
    }
    let project = project();
    let all = |reversed: bool| {
        let cache = tempfile::tempdir().unwrap();
        let (graph, mut files) = parse(project.path());
        if reversed {
            files.reverse();
        }
        let source = ClangSource::default();
        let indexes = source.load(project.path(), cache.path(), &files, Refresh::Edits);
        link(&graph, &files, &indexes);
        [EdgeKind::Calls, EdgeKind::UsesAsValue, EdgeKind::Implements]
            .map(|kind| edges(&graph, &files, kind))
    };
    assert_eq!(all(false), all(true));
}

/// What a fresh index says of an edited project is what `clangd` says after the edit.
#[test]
fn after_an_edit_clangd_answers_as_a_new_index_does() {
    if !installed() {
        return;
    }
    let project = project();
    let cache = project.path().join(".cache-of-mimir");
    let (_, files) = parse(project.path());
    let source = ClangSource::default();
    source.load(project.path(), &cache, &files, Refresh::Edits);

    // Lines moved above, a new function, a call of it from another file, a new local function.
    let edit = |path: &str, change: &dyn Fn(&str) -> String| {
        let at = project.path().join(path);
        let text = std::fs::read_to_string(&at).unwrap();
        std::fs::write(at, change(&text)).unwrap();
    };
    edit("src/util.c", &|text| {
        format!(
            "// moved\n// moved\n{text}\nstatic int added_local(int a) {{ return a; }}\nint added(int a) {{ return added_local(a); }}\n"
        )
    });
    edit("src/other.c", &|text| {
        format!(
            "// moved\n{text}\nint added(int a);\nint calls_added(int a) {{ return added(a) + other(a); }}\n"
        )
    });
    edit("src/shape.cpp", &|text| format!("// moved\n{text}"));
    edit("src/app.cpp", &|text| format!("// moved\n{text}"));

    let (graph, files) = parse(project.path());
    let indexes = source.load(project.path(), &cache, &files, Refresh::Edits);
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].answers.len(), 4, "the edited files were asked");
    link(&graph, &files, &indexes);

    let fresh = tempfile::tempdir().unwrap();
    let (new_graph, new_files) = parse(project.path());
    let new = ClangSource::default().load(project.path(), fresh.path(), &new_files, Refresh::Edits);
    link(&new_graph, &new_files, &new);
    for kind in [EdgeKind::Calls, EdgeKind::UsesAsValue, EdgeKind::Implements] {
        assert_eq!(
            edges(&graph, &files, kind),
            edges(&new_graph, &new_files, kind),
            "{kind:?}"
        );
    }
    let calls = edges(&graph, &files, EdgeKind::Calls);
    for (from, to) in [
        ("other.c calls_added", "added:9"),
        ("util.c added", "added_local:13"),
    ] {
        let pair = (from.to_string(), to.to_string());
        assert!(
            calls
                .iter()
                .any(|(a, b)| *a == pair.0 && b.starts_with(pair.1.split(':').next().unwrap())),
            "{pair:?}: {calls:?}"
        );
    }
}

#[test]
fn a_database_in_build_is_found() {
    if !installed() {
        return;
    }
    let project = project();
    std::fs::remove_file(project.path().join("compile_commands.json")).unwrap();
    std::fs::create_dir(project.path().join("build")).unwrap();
    write_database(
        project.path(),
        &project.path().join("build/compile_commands.json"),
    );
    let (graph, files, source) = resolved(project.path());
    assert_eq!(source.notes(project.path()), Vec::<String>::new());
    assert!(
        edges(&graph, &files, EdgeKind::Calls)
            .contains(&("main.c main".to_string(), "shared_add:3".to_string()))
    );
}

#[test]
fn with_no_database_only_syntax_rules_apply_and_a_person_is_told() {
    let project = project();
    std::fs::remove_file(project.path().join("compile_commands.json")).unwrap();
    let (graph, files, source) = resolved(project.path());
    let notes = source.notes(project.path());
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"),
        "{notes:?}"
    );
    let calls = edges(&graph, &files, EdgeKind::Calls);
    assert!(calls.contains(&("util.c twice".to_string(), "shared_add:3".to_string())));
    assert!(
        !calls.contains(&("main.c main".to_string(), "shared_add:3".to_string())),
        "no edge across files"
    );
}

#[test]
fn loads_at_the_same_time_do_not_get_in_each_other_s_way() {
    if !installed() {
        return;
    }
    let project = project();
    let cache = project.path().join(".cache-of-mimir");
    let (_, files) = parse(project.path());
    let source = ClangSource::default();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                let indexes = source.load(project.path(), &cache, &files, Refresh::Full);
                assert_eq!(indexes.len(), 1, "{:?}", source.notes(project.path()));
            });
        }
    });
    assert_eq!(source.notes(project.path()), Vec::<String>::new());
}

#[test]
fn a_kept_index_is_used_and_a_new_database_writes_another() {
    if !installed() {
        return;
    }
    let project = project();
    let cache = project.path().join(".cache-of-mimir");
    let (_, files) = parse(project.path());
    let source = ClangSource::default();
    assert_eq!(
        source
            .load(project.path(), &cache, &files, Refresh::Saved)
            .len(),
        0
    );
    source.load(project.path(), &cache, &files, Refresh::Edits);
    let kept = source.load(project.path(), &cache, &files, Refresh::Saved);
    assert_eq!(kept.len(), 1);
    assert!(!kept[0].documents.is_empty());
    let scip: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "scip"))
        .collect();
    assert_eq!(scip.len(), 1);
    assert!(
        std::fs::read_dir(&cache)
            .unwrap()
            .filter_map(|e| Some(e.ok()?.path()))
            .all(|p| p.extension().is_none_or(|e| e != "yaml")),
        "what the tool wrote goes"
    );
}
