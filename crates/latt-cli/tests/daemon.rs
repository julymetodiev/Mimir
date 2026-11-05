//! The binary from outside: nobody starts a daemon, and every command works.

mod common;

use assert_cmd::Command;
use common::{latt, project, stop};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn test_every_query_works_without_starting_anything() {
    let project = project();
    let dir = project.path();

    latt(dir)
        .args(["trace", "-s", "helper", "--callers"])
        .assert()
        .success()
        .stdout(contains("← compute"));
    latt(dir)
        .args(["search", "--pattern", "comp", "--code"])
        .assert()
        .success()
        .stdout(contains("compute"))
        .stdout(contains("helper()"));
    latt(dir)
        .args(["read", "-s", "compute"])
        .assert()
        .success()
        .stdout(contains("helper()"));
    latt(dir).args(["status"]).assert().success();
    assert!(dir.join(".latt/.gitignore").exists());

    stop(dir);
    latt(dir)
        .args(["daemon", "status"])
        .assert()
        .success()
        .stdout(contains("not running"));
}

#[test]
fn test_sync_saves_the_graph_and_reports_the_link() {
    let project = project();
    let dir = project.path();

    latt(dir)
        .arg("sync")
        .assert()
        .success()
        .stdout(contains("from syntax only (1 files)"));
    assert!(!dir.join(".latt/graph/graph.json").exists());
    latt(dir)
        .args(["sync", "--json"])
        .assert()
        .success()
        .stdout(contains("Graph saved to"));
    assert!(dir.join(".latt/graph/graph.json").exists());
    stop(dir);
}

#[test]
fn test_a_project_with_a_long_path_works() {
    let project = project();
    let deep = project.path().join("a".repeat(60)).join("b".repeat(60));
    fs::create_dir_all(deep.join("src")).unwrap();
    fs::create_dir(deep.join(".git")).unwrap();
    fs::write(deep.join("src/lib.rs"), "pub fn deep_one() {}\n").unwrap();

    latt(&deep)
        .args(["search", "--pattern", "deep_one"])
        .assert()
        .success()
        .stdout(contains("deep_one"));
    stop(&deep);
}

#[test]
fn test_outside_a_project_says_so() {
    let dir = TempDir::new().unwrap();
    latt(dir.path())
        .args(["trace", "-s", "x"])
        .assert()
        .failure()
        .stderr(contains("latt init"));
}

#[test]
fn test_an_unknown_symbol_and_a_bad_pattern_are_errors() {
    let project = project();
    let dir = project.path();
    latt(dir)
        .args(["trace", "-s", "no_such_symbol"])
        .assert()
        .failure()
        .stderr(contains("no_such_symbol"));
    latt(dir)
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
    latt(dir)
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
        latt(dir)
            .args(["read", "-s", "C::run", "--file", file])
            .assert()
            .success()
            .stdout(contains(file));
    }
    latt(dir)
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
    latt(dir)
        .args(["search", "--pattern", "^sync_", "--kind", "function"])
        .assert()
        .success()
        .stdout(contains("sync_one"))
        .stdout(contains("Total: 3 matches"));
    latt(dir)
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
    latt(dir)
        .args(["search", "--pattern", "helper"])
        .assert()
        .success();
    let log = fs::read_to_string(dir.join(".latt/daemon.log")).unwrap();
    assert!(!log.contains("tool.py"), "{log}");
    stop(dir);
}

#[test]
fn test_a_rebuild_replaces_the_daemon_and_the_same_build_keeps_it() {
    let project = project();
    let dir = project.path();
    let binaries = TempDir::new().unwrap();
    let built = assert_cmd::cargo::cargo_bin!("latt");
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
            .env("LATT_NO_SCIP", "1")
            .args(["search", "--pattern", "helper"])
            .assert()
            .success();
    };
    // A daemon makes its socket as it starts, so a new one means a new daemon.
    let root = dir.canonicalize().unwrap();
    let daemon = || {
        let socket = latt_protocol::socket_path(&root).unwrap();
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

    latt(dir)
        .args(["grep", "helper"])
        .assert()
        .success()
        .stdout(contains("notes.md:1:see helper in the docs"))
        .stdout(contains("src/lib.rs:6:    helper() [in compute]"));
    latt(dir)
        .args(["grep", "helper", "--path", "*.md", "--limit", "5"])
        .assert()
        .success()
        .stdout(contains("notes.md"))
        .stdout(predicates::str::contains("src/lib.rs").not());
    latt(dir)
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
    latt(dir)
        .args(["trace", "-s", "MAX", "--callers"])
        .assert()
        .success()
        .stdout(contains("← reader (function) [reads it]"));
    latt(dir)
        .args(["trace", "-s", "reader", "--callees"])
        .assert()
        .success()
        .stdout(contains("MAX").not());
    stop(dir);
}

#[test]
fn test_a_reader_gone_ends_the_program_without_a_panic() {
    let project = project();
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_latt"))
        .args(["daemon", "status"])
        .current_dir(project.path())
        .stdout(writer)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(141));
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[test]
fn test_files_and_cat_work_from_a_subdirectory() {
    let project = project();
    let dir = project.path();
    latt(dir)
        .args(["files", "*.toml"])
        .assert()
        .success()
        .stdout(contains("Cargo.toml"))
        .stdout(contains("src/lib.rs").not());
    let inner = dir.join("src");
    latt(&inner)
        .args(["cat", "lib.rs", "--first", "2", "--last", "2"])
        .assert()
        .success()
        .stdout(contains("2│     1"))
        .stdout(contains("Showing 1 of 7 lines"));
    stop(dir);
}
