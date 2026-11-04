//! `mimir impact` on a real git repository, with syntax rules only.

mod common;

use common::{mimir, project, stop};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success());
}

/// The project of the other tests, committed.
fn committed() -> TempDir {
    let dir = project();
    fs::remove_dir(dir.path().join(".git")).unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "first"]);
    dir
}

fn impact(dir: &Path, arguments: &[&str]) -> Value {
    let output = mimir(dir)
        .args(["impact", "--json"])
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn names(list: &Value, key: &str) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|entry| match key {
            "symbol" => entry["symbol"]["name"].as_str().unwrap().to_string(),
            _ => entry[key].as_str().unwrap().to_string(),
        })
        .collect()
}

const LIB: &str = "src/lib.rs";

#[test]
fn a_changed_function_comes_with_what_depends_on_it() {
    let dir = committed();
    let text = fs::read_to_string(dir.path().join(LIB)).unwrap();
    fs::write(dir.path().join(LIB), text.replace("    1\n", "    2\n")).unwrap();

    let answer = impact(dir.path(), &[]);
    assert_eq!(names(&answer["changed"], "symbol"), ["helper"]);
    assert_eq!(answer["changed"][0]["added"], false);
    assert_eq!(
        names(&answer["changed"][0]["dependents"], "symbol"),
        ["compute"]
    );
    stop(dir.path());
}

#[test]
fn lines_moved_by_a_comment_change_no_symbol() {
    let dir = committed();
    let text = fs::read_to_string(dir.path().join(LIB)).unwrap();
    fs::write(dir.path().join(LIB), format!("// moved\n\n{text}")).unwrap();

    let answer = impact(dir.path(), &[]);
    assert_eq!(answer["changed"], serde_json::json!([]));
    assert_eq!(answer["removed"], serde_json::json!([]));
    assert_eq!(answer["other_files"], serde_json::json!([LIB]));
    stop(dir.path());
}

#[test]
fn a_comment_on_top_of_a_python_file_changes_no_symbol_as_in_rust() {
    let dir = committed();
    let text = "def helper():\n    return 1\n";
    fs::write(dir.path().join("a.py"), text).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "python"]);
    fs::write(dir.path().join("a.py"), format!("# moved\n\n{text}")).unwrap();

    let answer = impact(dir.path(), &[]);
    assert_eq!(answer["changed"], serde_json::json!([]));
    assert_eq!(answer["other_files"], serde_json::json!(["a.py"]));
    stop(dir.path());
}

#[test]
fn a_removed_function_says_where_its_name_is_still_written() {
    let dir = committed();
    fs::write(
        dir.path().join(LIB),
        "pub fn compute() -> u32 {\n    helper()\n}\n",
    )
    .unwrap();

    let answer = impact(dir.path(), &[]);
    assert_eq!(names(&answer["removed"], "name"), ["helper"]);
    let removed = &answer["removed"][0];
    assert_eq!(removed["line"], 1);
    assert_eq!(removed["named_total"], 1);
    assert_eq!(removed["named_at"][0]["path"], LIB);
    assert_eq!(removed["named_at"][0]["line"], 2);
    stop(dir.path());
}

#[test]
fn a_function_moved_to_another_file_is_not_removed() {
    let dir = committed();
    let helper = "pub fn helper() -> u32 {\n    1\n}\n";
    fs::write(
        dir.path().join(LIB),
        "mod other;\npub use other::helper;\n\npub fn compute() -> u32 {\n    helper()\n}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), helper).unwrap();

    let answer = impact(dir.path(), &[]);
    assert_eq!(answer["removed"], serde_json::json!([]));
    let changed = names(&answer["changed"], "symbol");
    assert!(!changed.contains(&"helper".to_string()), "{changed:?}");
    stop(dir.path());
}

#[test]
fn a_new_file_is_all_added() {
    let dir = committed();
    fs::write(dir.path().join("src/new.rs"), "pub fn fresh() {}\n").unwrap();

    let answer = impact(dir.path(), &[]);
    assert_eq!(names(&answer["changed"], "symbol"), ["fresh"]);
    assert_eq!(answer["changed"][0]["added"], true);
    stop(dir.path());
}

#[test]
fn a_base_compares_with_where_a_branch_split_off() {
    let dir = committed();
    git(dir.path(), &["branch", "-M", "main"]);
    git(dir.path(), &["checkout", "-qb", "work"]);
    let text = fs::read_to_string(dir.path().join(LIB)).unwrap();
    fs::write(dir.path().join(LIB), text.replace("    1\n", "    2\n")).unwrap();
    git(dir.path(), &["commit", "-qam", "second"]);

    assert_eq!(impact(dir.path(), &[])["changed"], serde_json::json!([]));
    let answer = impact(dir.path(), &["--base", "main"]);
    assert_eq!(names(&answer["changed"], "symbol"), ["helper"]);
    stop(dir.path());
}

#[test]
fn a_base_that_is_no_commit_or_an_option_is_an_error() {
    let dir = committed();
    for base in ["nothing", "--output=x"] {
        mimir(dir.path())
            .args(["impact", &format!("--base={base}")])
            .assert()
            .failure();
    }
    assert!(!dir.path().join("x").exists());
    stop(dir.path());
}
