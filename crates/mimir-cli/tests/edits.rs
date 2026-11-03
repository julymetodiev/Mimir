//! Edits from outside: through the daemon, on the right lines, or refused.

mod common;

use common::{mimir, project, stop};
use predicates::str::contains;
use std::fs;
use tempfile::TempDir;

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
fn test_an_edit_of_a_shared_name_asks_which_and_a_file_answers() {
    let project = project();
    let dir = project.path();
    fs::write(dir.join("src/lib.rs"), "pub fn run() -> u32 {\n    1\n}\n").unwrap();
    fs::write(
        dir.join("src/other.rs"),
        "pub fn run() -> u32 {\n    2\n}\n",
    )
    .unwrap();
    let edit = |extra: &[&str]| {
        let mut command = mimir(dir);
        command.args(["edit", "--symbol", "run", "--mode", "symbol"]);
        command.args(["--content", "pub fn run() -> u32 {\n    3\n}"]);
        command.args(extra);
        command.assert()
    };

    edit(&[])
        .failure()
        .stderr(contains("src/other.rs"))
        .stderr(contains("src/lib.rs"));
    assert!(
        !fs::read_to_string(dir.join("src/lib.rs"))
            .unwrap()
            .contains("    3")
    );
    assert!(
        !fs::read_to_string(dir.join("src/other.rs"))
            .unwrap()
            .contains("    3")
    );

    edit(&["--file", "src/other.rs"]).success();
    assert!(
        fs::read_to_string(dir.join("src/other.rs"))
            .unwrap()
            .contains("    3")
    );
    assert!(
        fs::read_to_string(dir.join("src/lib.rs"))
            .unwrap()
            .contains("    1")
    );

    mimir(dir)
        .args(["read", "-s", "run", "--file", "src/lib.rs"])
        .assert()
        .success()
        .stdout(contains("src/lib.rs"));
    stop(dir);
}

#[test]
fn test_new_code_goes_after_a_symbol_at_every_level() {
    let project = project();
    let dir = project.path();
    fs::write(
        dir.join("src/lib.rs"),
        "pub enum Kind {\n    One,\n    Two,\n}\n\npub struct S {\n    pub a: u32,\n}\n\nimpl S {\n    pub fn m(&self) {} // note\n}\n\npub fn f() {}\n",
    )
    .unwrap();
    let after = |symbol: &str, content: &str| {
        mimir(dir)
            .args([
                "edit",
                "--mode",
                "after",
                "--symbol",
                symbol,
                "--content",
                content,
            ])
            .assert()
    };

    after("Kind::One", "    Between,").success();
    after("a", "    pub b: u32,").success();
    after("m", "    pub fn n(&self) {}\n").success();
    after("f", "pub fn g() {}").success();

    assert_eq!(
        fs::read_to_string(dir.join("src/lib.rs")).unwrap(),
        "pub enum Kind {\n    One,\n    Between,\n    Two,\n}\n\npub struct S {\n    pub a: u32,\n    pub b: u32,\n}\n\nimpl S {\n    pub fn m(&self) {} // note\n\n    pub fn n(&self) {}\n}\n\npub fn f() {}\n\npub fn g() {}\n"
    );
    mimir(dir)
        .args(["read", "-s", "g"])
        .assert()
        .success()
        .stdout(contains("pub fn g() {}"));
    stop(dir);
}

#[test]
fn test_a_qualified_name_chooses_among_variants_in_one_file() {
    let project = project();
    let dir = project.path();
    fs::write(
        dir.join("src/lib.rs"),
        "pub enum A {\n    Read,\n}\n\npub enum B {\n    Read,\n}\n",
    )
    .unwrap();
    mimir(dir)
        .args([
            "edit",
            "--mode",
            "symbol",
            "--symbol",
            "Read",
            "--content",
            "    X,",
        ])
        .assert()
        .failure()
        .stderr(contains("could mean"));
    mimir(dir)
        .args([
            "edit",
            "--mode",
            "symbol",
            "--symbol",
            "B::Read",
            "--content",
            "    X,",
        ])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.join("src/lib.rs")).unwrap(),
        "pub enum A {\n    Read,\n}\n\npub enum B {\n    X,\n}\n"
    );
    stop(dir);
}

#[test]
fn test_a_file_git_ignores_stays_out_of_the_graph() {
    let project = project();
    let dir = project.path();
    fs::write(dir.join(".gitignore"), "skipped/\n").unwrap();
    mimir(dir).arg("sync").assert().success();

    mimir(dir)
        .args([
            "write",
            "skipped/written.rs",
            "--content",
            "pub fn written() {}",
        ])
        .assert()
        .success();
    assert!(dir.join("skipped/written.rs").exists());

    // Written together, so once the file it does follow is in, the others have been decided.
    fs::write(dir.join("skipped/seen.rs"), "pub fn seen_too() {}\n").unwrap();
    fs::write(dir.join("src/seen.rs"), "pub fn seen() {}\n").unwrap();
    let appeared = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let found = mimir(dir)
            .args(["search", "--pattern", "^seen$"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&found.stdout).contains("src/seen.rs")
    });
    assert!(appeared, "the watcher did not report src/seen.rs");

    mimir(dir)
        .args(["search", "--pattern", "^(written|seen_too)$"])
        .assert()
        .success()
        .stdout(contains("No symbols found"));
    stop(dir);
}
