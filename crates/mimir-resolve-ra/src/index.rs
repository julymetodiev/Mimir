//! What is Rust's own in the index `rust-analyzer scip` writes: Cargo roots, and proc macros that
//! failed. What an index does after that is in `mimir_resolve_scip`, and edited files go to a
//! running analyzer (see [`crate::live`]).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mimir_resolve::{ProjectFile, Refresh, SemanticIndex};
use mimir_resolve_scip::{Answered, IndexFiles, IndexLanguage, ScipError, load_root, wait_for};
use mimir_types::Language;
use tracing::warn;

use crate::live::Analyzers;

const MAX_ROOTS: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(600);
/// What the log of an index holds when a proc macro failed and said so.
const MACRO_FAILURE: &str = "proc-macro tried to print";

/// The indexes for a project's Rust files: from the cache, or by running `rust-analyzer`.
/// Files in no index, because the tool is missing or failed, keep their syntax edges.
pub fn load_or_generate(
    project: &Path,
    cache_dir: &Path,
    files: &[ProjectFile],
    analyzers: &mut Analyzers,
    refresh: Refresh,
) -> Vec<SemanticIndex> {
    let rust: Vec<&Path> = files
        .iter()
        .map(|f| f.path.as_path())
        .filter(|p| p.is_relative() && p.extension().is_some_and(|e| e == "rs"))
        .collect();
    let mut language = Rust { analyzers };
    let mut indexes: Vec<SemanticIndex> = Vec::new();
    let mut tried: HashSet<PathBuf> = HashSet::new();
    while tried.len() < MAX_ROOTS {
        let uncovered = rust
            .iter()
            .filter(|p| !indexes.iter().any(|i| i.covers(p)))
            .find_map(|p| next_cargo_root(project, p, &tried, &indexes));
        let Some(root) = uncovered else {
            break;
        };
        tried.insert(root.clone());
        match load_root(&mut language, project, cache_dir, &root, files, refresh) {
            Ok(index) => indexes.push(index),
            Err(ScipError::ToolMissing(e)) => {
                warn!("No semantic index, using syntax rules: {e}");
                break;
            }
            Err(ScipError::Failed(e)) => {
                warn!("No semantic index for {:?}, using syntax rules: {e}", root)
            }
            Err(ScipError::NotSaved) => {}
        }
    }
    indexes
}

/// The outermost untried directory above a file with a `Cargo.toml`, relative to the project.
/// Outermost, because a member crate's index would stop at the crate.
///
/// A crate an index already reaches is left out. Its files missing from that index are behind
/// `cfg` or a feature, or in no module tree, and indexing the crate alone does not find them.
fn next_cargo_root(
    project: &Path,
    file: &Path,
    tried: &HashSet<PathBuf>,
    indexes: &[SemanticIndex],
) -> Option<PathBuf> {
    let dirs: Vec<&Path> = file.ancestors().skip(1).collect();
    dirs.into_iter()
        .rev()
        .find(|dir| {
            !tried.contains(*dir)
                && project.join(dir).join("Cargo.toml").is_file()
                && !indexes.iter().any(|index| index.reaches(dir))
        })
        .map(Path::to_path_buf)
}

struct Rust<'a> {
    analyzers: &'a mut Analyzers,
}

impl IndexLanguage for Rust<'_> {
    fn stem_prefix(&self) -> &'static str {
        "scip"
    }

    fn owns(&self, path: &Path) -> bool {
        Language::from_path(path) == Language::Rust
    }

    fn manifests(&self, root: &Path, sources: &[&Path]) -> Vec<PathBuf> {
        let mut manifests: HashSet<PathBuf> = sources
            .iter()
            .flat_map(|path| path.ancestors().skip(1))
            .filter(|dir| dir.starts_with(root))
            .flat_map(|dir| [dir.join("Cargo.toml"), dir.join("Cargo.lock")])
            .collect();
        manifests.insert(root.join("Cargo.lock"));
        manifests.into_iter().collect()
    }

    fn write_index(&mut self, root: &Path, paths: &IndexFiles) -> Result<(), ScipError> {
        let mut child = Command::new("rust-analyzer")
            .arg("--log-file")
            .arg(&paths.log)
            .args(["scip", "."])
            .arg("--output")
            .arg(&paths.index)
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ScipError::ToolMissing(format!("cannot run rust-analyzer: {e}")))?;
        wait_for(
            &mut child,
            Instant::now() + TIMEOUT,
            "rust-analyzer",
            &self.analyzers.stopping(),
        )
    }

    fn suspects<'f>(
        &self,
        log: &Path,
        root: &Path,
        index: &SemanticIndex,
        files: &'f [ProjectFile],
    ) -> Vec<&'f ProjectFile> {
        let log = fs::read_to_string(log).unwrap_or_default();
        let failures: Vec<&str> = log
            .lines()
            .filter(|line| line.contains(MACRO_FAILURE))
            .collect();
        if failures.is_empty() {
            return Vec::new();
        }
        let suspects: Vec<&ProjectFile> = files
            .iter()
            .filter(|f| f.path.starts_with(root) && index.covers(&f.path))
            .filter(|f| {
                macros_on_impls(&f.source)
                    .iter()
                    .any(|name| failures.iter().any(|l| names_macro(l, name)))
            })
            .collect();
        warn!(
            "A proc macro failed in the index for {:?}: {} files go to a running analyzer",
            root,
            suspects.len()
        );
        suspects
    }

    fn answer(
        &mut self,
        project: &Path,
        root: &Path,
        files: &[ProjectFile],
        asked: &[&ProjectFile],
    ) -> Result<Answered, String> {
        Ok(Answered {
            documents: self.analyzers.resolve(project, root, files, asked)?,
            unanswered: Vec::new(),
        })
    }
}

/// The attribute macros on `impl`s: the items a macro with state can fail on, as `#[napi]` does
/// on an `impl` reached before its struct. The impl's methods then go unresolved.
fn macros_on_impls(source: &str) -> Vec<&str> {
    let (mut found, mut above) = (Vec::new(), Vec::new());
    for line in source.lines().map(str::trim_start) {
        if let Some(attribute) = line.strip_prefix("#[") {
            let path = attribute.split(['(', ']', ' ']).next().unwrap_or("");
            above.push(path.rsplit("::").next().unwrap_or(path));
        } else if line.starts_with("impl") && line[4..].starts_with([' ', '<']) {
            found.append(&mut above);
        } else if !line.is_empty() && !line.starts_with("//") {
            above.clear();
        }
    }
    found
}

/// Whether a line of the log names a macro, as a whole word.
fn names_macro(line: &str, name: &str) -> bool {
    line.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn a_crate_an_index_reaches_gets_no_index_of_its_own() {
        let project =
            std::env::temp_dir().join(format!("mimir-resolve-roots-{}", std::process::id()));
        for dir in ["", "crates/member", "other"] {
            fs::create_dir_all(project.join(dir)).unwrap();
            fs::write(project.join(dir).join("Cargo.toml"), "").unwrap();
        }
        let index = SemanticIndex {
            root: PathBuf::new(),
            documents: HashMap::from([(PathBuf::from("crates/member/src/lib.rs"), Vec::new())]),
            ..SemanticIndex::default()
        };
        let tried = HashSet::from([PathBuf::new()]);
        let next = |file: &str| {
            next_cargo_root(
                &project,
                Path::new(file),
                &tried,
                std::slice::from_ref(&index),
            )
        };

        let behind_cfg = next("crates/member/src/linux_only.rs");
        let own_workspace = next("other/src/lib.rs");
        let _ = fs::remove_dir_all(&project);

        assert_eq!(behind_cfg, None);
        assert_eq!(own_workspace, Some(PathBuf::from("other")));
    }

    #[test]
    fn finds_the_macros_on_impls() {
        assert_eq!(macros_on_impls("#[napi]\nimpl Core {}\n"), ["napi"]);
        assert_eq!(
            macros_on_impls("#[napi]\n// why\nimpl<T> Core<T> {}\n"),
            ["napi"]
        );
        assert_eq!(
            macros_on_impls("    #[async_trait::async_trait]\n    impl A for B {}\n"),
            ["async_trait"]
        );
        assert!(macros_on_impls("#[napi]\npub struct Core;\nimpl Core {}\n").is_empty());
        assert!(macros_on_impls("#[napi]\nimplement();\n").is_empty());
    }

    #[test]
    fn a_failure_names_its_macro_as_a_whole_word() {
        let line = "ERROR proc-macro tried to print : `napi` macro expand failed.";
        assert!(names_macro(line, "napi"));
        assert!(!names_macro(line, "nap"));
        assert!(!names_macro(line, "async_trait"));
    }
}
