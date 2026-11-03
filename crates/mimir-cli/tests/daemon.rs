//! The binary from outside: nobody starts a daemon, and every command works.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A git repository with one crate. The socket's directory is the system's, so tests stay apart by
/// their project's path.
fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn helper() -> u32 {\n    1\n}\n\npub fn compute() -> u32 {\n    helper()\n}\n",
    )
    .unwrap();
    dir
}

/// Mimir with syntax rules only, so these tests do not wait for an index.
fn mimir(dir: &Path) -> Command {
    let mut command = cargo_bin_cmd!("mimir");
    command.current_dir(dir).env("MIMIR_NO_SCIP", "1");
    command
}

fn stop(dir: &Path) {
    mimir(dir).args(["daemon", "stop"]).assert().success();
}

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
fn test_an_edit_is_seen_by_the_next_sync() {
    let project = project();
    let dir = project.path();

    mimir(dir).arg("sync").assert().success();
    fs::write(
        dir.join("src/other.rs"),
        "pub fn other() -> u32 {\n    crate::helper()\n}\n",
    )
    .unwrap();
    mimir(dir)
        .arg("sync")
        .assert()
        .success()
        .stdout(contains("1 added"));
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
fn test_an_edit_goes_through_the_daemon_and_the_graph_follows() {
    let project = project();
    let dir = project.path();

    mimir(dir)
        .args([
            "edit",
            "--symbol",
            "helper",
            "--mode",
            "symbol",
            "--content",
            "pub fn helper() -> u32 {\n    2\n}\n\npub fn extra() -> u32 {\n    helper()\n}",
        ])
        .assert()
        .success()
        .stdout(contains("src/lib.rs"));
    assert!(
        fs::read_to_string(dir.join("src/lib.rs"))
            .unwrap()
            .contains("    2\n")
    );

    mimir(dir)
        .args(["trace", "-s", "helper", "--callers"])
        .assert()
        .success()
        .stdout(contains("← extra"))
        .stdout(contains("← compute"));
    stop(dir);
}

#[test]
fn test_an_edit_of_a_file_changed_behind_the_graph_lands_in_the_right_place() {
    let project = project();
    let dir = project.path();
    mimir(dir)
        .args(["read", "-s", "compute"])
        .assert()
        .success();

    // The lines of `compute` move before anything could have told the daemon.
    let moved = format!(
        "// a new first line\n{}",
        fs::read_to_string(dir.join("src/lib.rs")).unwrap()
    );
    fs::write(dir.join("src/lib.rs"), moved).unwrap();
    mimir(dir)
        .args([
            "edit",
            "--symbol",
            "compute",
            "--mode",
            "symbol",
            "--content",
            "pub fn compute() -> u32 {\n    3\n}",
        ])
        .assert()
        .success();

    let edited = fs::read_to_string(dir.join("src/lib.rs")).unwrap();
    assert!(edited.starts_with("// a new first line\n"));
    assert!(edited.contains("pub fn helper() -> u32 {\n    1\n}"));
    assert!(edited.contains("pub fn compute() -> u32 {\n    3\n}"));
    stop(dir);
}

#[test]
fn test_a_preview_changes_nothing_and_a_file_outside_is_refused() {
    let project = project();
    let dir = project.path();
    let before = fs::read_to_string(dir.join("src/lib.rs")).unwrap();

    mimir(dir)
        .args([
            "edit",
            "--symbol",
            "helper",
            "--mode",
            "symbol",
            "--content",
            "fn x() {}",
            "--preview",
        ])
        .assert()
        .success()
        .stdout(contains("src/lib.rs"));
    assert_eq!(fs::read_to_string(dir.join("src/lib.rs")).unwrap(), before);

    let outside = TempDir::new().unwrap();
    let file = outside.path().join("other.rs");
    fs::write(&file, "fn y() {}\n").unwrap();
    mimir(dir)
        .args([
            "edit",
            "--mode",
            "insert",
            "--start",
            "0",
            "--content",
            "//",
        ])
        .arg("--file")
        .arg(&file)
        .assert()
        .failure()
        .stderr(contains("outside the project"));
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
