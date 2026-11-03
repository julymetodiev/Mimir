//! The index `rust-analyzer scip` writes, kept beside the graph with a list of the sources it was
//! made from. An unchanged project does not pay for it again, and edited files go to a running
//! analyzer (see [`crate::live`]) until too many have changed.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mimir_resolve::{ProjectFile, Refresh, SemanticIndex};
use mimir_types::ContentHash;
use tracing::{info, warn};

use crate::decode::decode;
use crate::live::{Analyzers, stop};

const MAX_ROOTS: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(600);
/// An edit can change what files outside it resolve to, so edits may not pile up forever.
const MAX_EDITED_FILES: usize = 50;
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
        match load_root(project, cache_dir, &root, files, analyzers, refresh) {
            Ok(index) => indexes.push(index),
            Err(LoadError::ToolMissing(e)) => {
                warn!("No semantic index, using syntax rules: {e}");
                break;
            }
            Err(LoadError::Failed(e)) => {
                warn!("No semantic index for {:?}, using syntax rules: {e}", root)
            }
            Err(LoadError::NotSaved) => {}
        }
    }
    indexes
}

enum LoadError {
    /// `rust-analyzer` is not installed, so no other root can do better.
    ToolMissing(String),
    Failed(String),
    /// [`Refresh::Saved`] found no index.
    NotSaved,
}

impl From<String> for LoadError {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

fn remove_if_there(path: &Path) -> Result<(), LoadError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(LoadError::Failed(format!("{}: {e}", path.display())))
        }
        _ => Ok(()),
    }
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

fn load_root(
    project: &Path,
    cache_dir: &Path,
    root: &Path,
    files: &[ProjectFile],
    analyzers: &mut Analyzers,
    refresh: Refresh,
) -> Result<SemanticIndex, LoadError> {
    let abs = project.join(root);
    let snapshot = Snapshot::of(project, root, files);
    let stem = format!(
        "scip-{}",
        ContentHash::compute(root.to_string_lossy().as_bytes()).to_hex()
    );
    let (index_file, key_file) = (
        cache_dir.join(format!("{stem}.scip")),
        cache_dir.join(format!("{stem}.key")),
    );
    let (log_file, failed_file) = (
        cache_dir.join(format!("{stem}.log")),
        cache_dir.join(format!("{stem}.macros")),
    );
    let read = || -> Result<SemanticIndex, LoadError> {
        let bytes = fs::read(&index_file).map_err(|e| LoadError::Failed(e.to_string()))?;
        let mut index = decode(&bytes)?;
        index.root = root.to_path_buf();
        Ok(index)
    };

    let saved = fs::read_to_string(&key_file)
        .ok()
        .filter(|_| index_file.is_file())
        .map(|text| Snapshot::parse(&text));
    let failed = || macro_failures(&failed_file, files);
    if refresh == Refresh::Saved {
        let saved = saved.ok_or(LoadError::NotSaved)?;
        let mut index = read()?;
        let edited = edited_files(&index, &saved, &snapshot, files);
        index.unanswered = edited
            .into_iter()
            .chain(failed())
            .map(|f| f.path.clone())
            .collect();
        return Ok(index);
    }
    let usable = saved.filter(|saved| match refresh {
        Refresh::Full => *saved == snapshot,
        _ => saved.manifests == snapshot.manifests,
    });
    if let Some(saved) = usable {
        let mut index = read()?;
        let edited = edited_files(&index, &saved, &snapshot, files);
        if edited.len() <= MAX_EDITED_FILES {
            let edits_only = edited.is_empty();
            let asked: Vec<_> = union(edited, failed());
            if asked.is_empty() {
                return Ok(index);
            }
            match analyzers.resolve(project, &abs, files, &asked) {
                Ok(live) => {
                    index.answers = live;
                    return Ok(index);
                }
                // A new index would not help files whose macros may fail again.
                Err(e) if edits_only => {
                    warn!(
                        "No running analyzer for {:?}, keeping what the index says: {e}",
                        root
                    );
                    return Ok(index);
                }
                Err(e) => warn!(
                    "No running analyzer for {:?}, writing a new index: {e}",
                    root
                ),
            }
        }
    }

    fs::create_dir_all(cache_dir).map_err(|e| LoadError::Failed(e.to_string()))?;
    remove_if_there(&key_file)?;
    remove_if_there(&failed_file)?;
    let started = Instant::now();
    run_rust_analyzer(&abs, &index_file, &log_file)?;
    fs::write(&key_file, snapshot.text()).map_err(|e| LoadError::Failed(e.to_string()))?;
    info!(
        "Semantic index for {:?} written in {:?}",
        root,
        started.elapsed()
    );

    let mut index = read()?;
    let log = fs::read_to_string(&log_file).unwrap_or_default();
    let failures: Vec<&str> = log
        .lines()
        .filter(|line| line.contains(MACRO_FAILURE))
        .collect();
    if !failures.is_empty() {
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
        let list: String = suspects
            .iter()
            .map(|f| format!("{}\n", f.path.display()))
            .collect();
        fs::write(&failed_file, list).map_err(|e| LoadError::Failed(e.to_string()))?;
        match analyzers.resolve(project, &abs, files, &suspects) {
            Ok(live) => index.answers = live,
            Err(e) => warn!(
                "No running analyzer for {:?}, keeping what the index says: {e}",
                root
            ),
        }
    }
    Ok(index)
}

fn union<'a>(mut files: Vec<&'a ProjectFile>, more: Vec<&'a ProjectFile>) -> Vec<&'a ProjectFile> {
    for file in more {
        if !files.iter().any(|f| f.path == file.path) {
            files.push(file);
        }
    }
    files
}

/// The files recorded as possibly hit by a proc macro that failed during the index.
fn macro_failures<'a>(list: &Path, files: &'a [ProjectFile]) -> Vec<&'a ProjectFile> {
    let Ok(list) = fs::read_to_string(list) else {
        return Vec::new();
    };
    let listed: HashSet<&Path> = list.lines().map(Path::new).collect();
    files
        .iter()
        .filter(|f| listed.contains(f.path.as_path()))
        .collect()
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

/// Files whose index entries no longer hold: changed ones it covered, and new ones.
/// A changed file it did not cover is behind `cfg`, where an analyzer knows no more than it did.
fn edited_files<'a>(
    index: &SemanticIndex,
    saved: &Snapshot,
    now: &Snapshot,
    files: &'a [ProjectFile],
) -> Vec<&'a ProjectFile> {
    files
        .iter()
        .filter(|f| {
            let current = now.sources.get(&f.path);
            let before = saved.sources.get(&f.path);
            current.is_some() && current != before && (before.is_none() || index.covers(&f.path))
        })
        .collect()
}

/// What an index was made from: the hash of the manifests, and of each source by path.
#[derive(Debug, Default, PartialEq, Eq)]
struct Snapshot {
    manifests: String,
    sources: HashMap<PathBuf, String>,
}

impl Snapshot {
    fn text(&self) -> String {
        let mut sources: Vec<_> = self.sources.iter().collect();
        sources.sort();
        let mut text = format!("{}\n", self.manifests);
        for (path, hash) in sources {
            text.push_str(&format!("{hash} {}\n", path.display()));
        }
        text
    }

    fn parse(text: &str) -> Self {
        let mut lines = text.lines();
        let manifests = lines.next().unwrap_or("").to_string();
        let sources = lines
            .filter_map(|line| line.split_once(' '))
            .map(|(hash, path)| (PathBuf::from(path), hash.to_string()))
            .collect();
        Self { manifests, sources }
    }

    fn of(project: &Path, root: &Path, files: &[ProjectFile]) -> Self {
        let mut sources: Vec<(&Path, ContentHash)> = files
            .iter()
            .filter(|f| f.path.starts_with(root))
            .map(|f| (f.path.as_path(), ContentHash::compute(f.source.as_bytes())))
            .collect();
        let mut manifests: HashSet<PathBuf> = sources
            .iter()
            .flat_map(|(path, _)| path.ancestors().skip(1))
            .filter(|dir| dir.starts_with(root))
            .flat_map(|dir| [dir.join("Cargo.toml"), dir.join("Cargo.lock")])
            .collect();
        manifests.insert(root.join("Cargo.lock"));
        let mut manifests: Vec<_> = manifests.into_iter().collect();
        manifests.sort();
        let mut text = String::new();
        for manifest in manifests {
            if let Ok(bytes) = fs::read(project.join(&manifest)) {
                text.push_str(&format!(
                    "{} {}\n",
                    manifest.display(),
                    ContentHash::compute(&bytes).to_hex()
                ));
            }
        }
        sources.sort_by_key(|(path, _)| *path);
        Self {
            manifests: ContentHash::compute(text.as_bytes()).to_hex(),
            sources: sources
                .into_iter()
                .map(|(path, hash)| (path.to_path_buf(), hash.to_hex()))
                .collect(),
        }
    }
}

fn run_rust_analyzer(root: &Path, output: &Path, log: &Path) -> Result<(), LoadError> {
    let mut child = Command::new("rust-analyzer")
        .arg("--log-file")
        .arg(log)
        .args(["scip", "."])
        .arg("--output")
        .arg(output)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| LoadError::ToolMissing(format!("cannot run rust-analyzer: {e}")))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("rust-analyzer exited with {status}").into()),
            None if started.elapsed() > TIMEOUT => {
                stop(&mut child);
                return Err("rust-analyzer timed out".to_string().into());
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn a_snapshot_survives_being_saved() {
        let mut snapshot = Snapshot {
            manifests: "m".to_string(),
            ..Snapshot::default()
        };
        snapshot
            .sources
            .insert(PathBuf::from("a b/c.rs"), "h".to_string());
        assert_eq!(Snapshot::parse(&snapshot.text()), snapshot);
    }
}
