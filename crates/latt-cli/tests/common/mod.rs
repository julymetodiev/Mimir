//! What the tests of the binary share.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A git repository with one crate. The socket's directory is the system's, so tests stay apart by
/// their project's path.
pub fn project() -> TempDir {
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

/// Lantern Lattice with syntax rules only, so these tests do not wait for an index.
pub fn latt(dir: &Path) -> Command {
    let mut command = cargo_bin_cmd!("latt");
    command.current_dir(dir).env("LATT_NO_SCIP", "1");
    command
}

pub fn stop(dir: &Path) {
    latt(dir).args(["daemon", "stop"]).assert().success();
}
