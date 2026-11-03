//! The binary from outside: nobody starts a daemon, and every command works.

mod common;

use assert_cmd::Command;
use common::{mimir, project, stop};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn test_every_query_works_without_starting_anything() {
    let project = project();
    let dir = project.path();

    mimir(dir)
        .args(["trace", "-s", "helper", "--callers"])
        .assert()
        .success()
        .stdout(contains("← compute"));
    mimir(dir)
        .args(["search", "--pattern", "comp", "--code"])
        .assert()
        .success()
        .stdout(contains("compute"))
        .stdout(contains("helper()"));
    mimir(dir)
        .args(["read", "-s", "compute"])
        .assert()
        .success()
        .stdout(contains("helper()"));
    mimir(dir).args(["status"]).assert().success();
    assert!(dir.join(".mimir/.gitignore").exists());

    stop(dir);
    mimir(dir)
        .args(["daemon", "status"])
        .assert()
        .success()
        .stdout(contains("not running"));
}

#[test]
fn test_sync_saves_the_graph_and_reports_the_link() {
    let project = project();
    let dir = project.path();

    mimir(dir)
        .arg("sync")
        .assert()
        .success()
        .stdout(contains("from syntax only (1 files)"))
        .stdout(contains("Graph saved to"));
    assert!(dir.join(".mimir/graph/graph.json").exists());
    stop(dir);
}

#[test]
fn test_a_project_with_a_long_path_works() {
    let project = project();
    let deep = project.path().join("a".repeat(60)).join("b".repeat(60));
    fs::create_dir_all(deep.join("src")).unwrap();
    fs::create_dir(deep.join(".git")).unwrap();
    fs::write(deep.join("src/lib.rs"), "pub fn deep_one() {}\n").unwrap();

    mimir(&deep)
        .args(["search", "--pattern", "deep_one"])
        .assert()
        .success()
        .stdout(contains("deep_one"));
    stop(&deep);
}

#[test]
fn test_outside_a_project_says_so() {
    let dir = TempDir::new().unwrap();
    mimir(dir.path())
        .args(["trace", "-s", "x"])
        .assert()
        .failure()
        .stderr(contains("mimir init"));
}

#[test]
fn test_an_unknown_symbol_and_a_bad_pattern_are_errors() {
    let project = project();
    let dir = project.path();
    mimir(dir)
        .args(["trace", "-s", "no_such_symbol"])
        .assert()
        .failure()
        .stderr(contains("no_such_symbol"));
    mimir(dir)
        .args(["search", "--pattern", "("])
        .assert()
        .failure()
        .stderr(contains("Invalid pattern"));
    stop(dir);
}

#[test]
fn test_a_name_shared_with_a_field_traces_the_function() {
    let project = project();
    let dir = project.path();
    fs::write(
        dir.join("src/lib.rs"),
        "pub struct Report {\n    pub total: u32,\n}\n\npub fn total() -> u32 {\n    helper()\n}\n\npub fn helper() -> u32 {\n    1\n}\n",
    )
    .unwrap();
    mimir(dir)
        .args(["trace", "-s", "total", "--callees"])
        .assert()
        .success()
        .stdout(contains("Trace for: total (function)"))
        .stdout(contains("→ helper"))
        .stderr(contains("tracing the most likely"));
    stop(dir);
}

#[test]
fn test_a_file_chooses_among_the_same_qualified_name_in_several() {
    let project = project();
    let dir = project.path();
    fs::write(
        dir.join("a.py"),
        "class C:\n    def run(self):\n        pass\n",
    )
    .unwrap();
    fs::write(
        dir.join("b.py"),
        "class C:\n    def run(self):\n        pass\n",
    )
    .unwrap();
    for file in ["a.py", "b.py"] {
        mimir(dir)
            .args(["read", "-s", "C::run", "--file", file])
            .assert()
            .success()
            .stdout(contains(file));
    }
    mimir(dir)
        .args(["trace", "-s", "C::run"])
        .assert()
        .success()
        .stderr(contains("2 symbols"));
    stop(dir);
}

#[test]
fn test_search_counts_all_matches_and_functions_include_methods() {
    let project = project();
    let dir = project.path();
    fs::write(
        dir.join("src/lib.rs"),
        "pub struct S;\nimpl S {\n    pub fn sync_one(&self) {}\n}\npub fn sync_two() {}\npub fn sync_three() {}\n",
    )
    .unwrap();
    mimir(dir)
        .args(["search", "--pattern", "^sync_", "--kind", "function"])
        .assert()
        .success()
        .stdout(contains("sync_one"))
        .stdout(contains("Total: 3 matches"));
    mimir(dir)
        .args(["search", "--pattern", "^sync_", "--limit", "2"])
        .assert()
        .success()
        .stdout(contains("Showing 2 of 3 matches"));
    stop(dir);
}

#[test]
fn test_a_file_in_another_language_is_no_warning() {
    let project = project();
    let dir = project.path();
    fs::write(dir.join("tool.py"), "print('hello')\n").unwrap();
    mimir(dir)
        .args(["search", "--pattern", "helper"])
        .assert()
        .success();
    let log = fs::read_to_string(dir.join(".mimir/daemon.log")).unwrap();
    assert!(!log.contains("tool.py"), "{log}");
    stop(dir);
}

#[test]
fn test_a_rebuild_replaces_the_daemon_and_the_same_build_keeps_it() {
    let project = project();
    let dir = project.path();
    let binaries = TempDir::new().unwrap();
    let built = assert_cmd::cargo::cargo_bin!("mimir");
    let (old, new) = (binaries.path().join("old"), binaries.path().join("new"));
    fs::copy(built, &old).unwrap();
    fs::copy(built, &new).unwrap();
    // A copy can keep the original's time; a rebuild does not.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&new)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let run = |binary: &Path| {
        Command::new(binary)
            .current_dir(dir)
            .env("MIMIR_NO_SCIP", "1")
            .args(["search", "--pattern", "helper"])
            .assert()
            .success();
    };
    // A daemon makes its socket as it starts, so a new one means a new daemon.
    let root = dir.canonicalize().unwrap();
    let daemon = || {
        let socket = mimir_protocol::socket_path(&root).unwrap();
        fs::metadata(socket).unwrap().modified().unwrap()
    };

    run(&old);
    let first = daemon();
    run(&old);
    assert_eq!(daemon(), first, "the same build started again");

    run(&new);
    assert_ne!(daemon(), first, "the old daemon stayed");
    Command::new(&new)
        .current_dir(dir)
        .args(["daemon", "stop"])
        .assert()
        .success();
}

#[test]
fn test_grep_finds_text_anywhere_and_names_the_symbol() {
    let project = project();
    let dir = project.path();
    fs::write(dir.join("notes.md"), "see helper in the docs\n").unwrap();

    mimir(dir)
        .args(["grep", "helper"])
        .assert()
        .success()
        .stdout(contains("notes.md:1:see helper in the docs"))
        .stdout(contains("src/lib.rs:6:    helper() [in compute]"));
    mimir(dir)
        .args(["grep", "helper", "--path", "*.md", "--limit", "5"])
        .assert()
        .success()
        .stdout(contains("notes.md"))
        .stdout(predicates::str::contains("src/lib.rs").not());
    mimir(dir)
        .args(["grep", "("])
        .assert()
        .failure()
        .stderr(contains("Invalid pattern"));
    stop(dir);
}

#[test]
fn test_trace_of_a_constant_shows_who_reads_it() {
    let project = project();
    let dir = project.path();
    fs::write(
        dir.join("src/lib.rs"),
        "pub const MAX: u32 = 1;\n\npub fn reader() -> u32 {\n    MAX\n}\n",
    )
    .unwrap();
    mimir(dir)
        .args(["trace", "-s", "MAX", "--callers"])
        .assert()
        .success()
        .stdout(contains("← reader (function) [reads it]"));
    mimir(dir)
        .args(["trace", "-s", "reader", "--callees"])
        .assert()
        .success()
        .stdout(contains("MAX").not());
    stop(dir);
}

#[test]
fn test_files_and_cat_work_from_a_subdirectory() {
    let project = project();
    let dir = project.path();
    mimir(dir)
        .args(["files", "*.toml"])
        .assert()
        .success()
        .stdout(contains("Cargo.toml"))
        .stdout(contains("src/lib.rs").not());
    let inner = dir.join("src");
    mimir(&inner)
        .args(["cat", "lib.rs", "--first", "2", "--last", "2"])
        .assert()
        .success()
        .stdout(contains("2│     1"))
        .stdout(contains("Showing 1 of 7 lines"));
    stop(dir);
}
