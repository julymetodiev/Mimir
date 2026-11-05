//! Directory scanner.

use chrono::{DateTime, TimeZone, Utc};
use ignore::WalkBuilder;
use latt_edit::is_temporary;
use latt_types::{ContentHash, Language};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::warn;

/// A file over this is generated, not written, and stays out of the graph.
const MAX_FILE_BYTES: u64 = 500 * 1024;

/// Information about a scanned file.
#[derive(Debug, Clone)]
pub struct ScannedFile {
    /// Path relative to scan root.
    pub path: PathBuf,

    /// Absolute path.
    pub absolute_path: PathBuf,

    /// Detected language.
    pub language: Language,

    /// File size in bytes.
    pub size: u64,

    /// Last modification time.
    pub modified_at: DateTime<Utc>,

    /// Content hash (computed during scan).
    pub content_hash: ContentHash,
}

/// The walk of a scan: what it does not visit is not part of the graph.
fn walker(root: &Path) -> WalkBuilder {
    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(true) // Respect hidden files
        .git_ignore(true) // Respect .gitignore
        .git_global(true) // Respect global gitignore
        .git_exclude(true); // Respect .git/info/exclude
    walker
}

/// Whether a scan would visit this path. A file reported or edited one at a time goes into the graph
/// only if so, or a scan would take it out again. Only the way down to it is walked.
pub fn visits(root: &Path, path: &Path) -> bool {
    let wanted = path.to_path_buf();
    walker(root)
        .filter_entry(move |entry| wanted.starts_with(entry.path()))
        .build()
        .flatten()
        .any(|entry| entry.path() == path)
}

/// Whether a scan would take this file into the graph, whichever way it came to be noticed.
pub fn takes(root: &Path, path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|file| file.is_file() && file.len() <= MAX_FILE_BYTES)
        && !go_tool_skips(path.strip_prefix(root).unwrap_or(path))
        && visits(root, path)
}

/// A Go file in a directory the Go tool does not build: `vendor`, `testdata`, or a name that starts
/// with `_`. One that starts with `.` no scan visits anyway.
fn go_tool_skips(relative: &Path) -> bool {
    Language::from_path(relative) == Language::Go
        && relative
            .parent()
            .into_iter()
            .flat_map(Path::components)
            .any(|part| {
                let part = part.as_os_str().to_string_lossy();
                part == "vendor" || part == "testdata" || part.starts_with('_')
            })
}

/// Remove temporary files that a crash during a write left in the project. One younger than `age`
/// may belong to a write still going on, so it stays.
pub fn remove_leftover_writes(root: &Path, age: Duration) {
    let mut walker = walker(root);
    walker.hidden(false);
    for entry in walker.build().flatten() {
        let name = entry.file_name().to_string_lossy();
        if !is_temporary(&name) {
            continue;
        }
        let old_enough = entry
            .metadata()
            .ok()
            .and_then(|file| file.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|elapsed| elapsed >= age);
        if old_enough && let Err(e) = std::fs::remove_file(entry.path()) {
            warn!("Could not remove {}: {e}", entry.path().display());
        }
    }
}

/// What is known of files without reading them: size, time in milliseconds and hash, by full path.
pub type Known = HashMap<PathBuf, (u64, i64, ContentHash)>;

/// Directory scanner with gitignore support.
pub struct Scanner {
    root: PathBuf,
}

impl Scanner {
    /// Create a new scanner for the given root directory.
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Scan the directory and return all matching files. A file `known` with the same size and time
    /// is not read: its hash is taken from there. A tool that changes a file and keeps both is
    /// missed until a scan with nothing known.
    pub async fn scan(&self, known: Known) -> std::io::Result<Vec<ScannedFile>> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || scan(&root, &known))
            .await
            .map_err(std::io::Error::other)
    }
}

/// What a scan of this directory finds. What is in no language costs no call for its size or time.
fn scan(root: &Path, known: &Known) -> Vec<ScannedFile> {
    let mut files = Vec::new();
    for entry in walker(root).build().flatten() {
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        let language = Language::from_path(path);
        if !language.is_known() || go_tool_skips(path.strip_prefix(root).unwrap_or(path)) {
            continue;
        }
        // Through a link, which may be to a directory or to nothing.
        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| {
                Utc.timestamp_opt(d.as_secs() as i64, d.subsec_nanos())
                    .single()
            })
            .unwrap_or_else(Utc::now);
        let content_hash = match known.get(path) {
            Some((size, modified, hash))
                if *size == metadata.len() && *modified == modified_at.timestamp_millis() =>
            {
                *hash
            }
            _ => {
                let Ok(content) = std::fs::read(path) else {
                    continue;
                };
                ContentHash::compute(&content)
            }
        };
        files.push(ScannedFile {
            path: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
            absolute_path: path.to_path_buf(),
            language,
            size: metadata.len(),
            modified_at,
            content_hash,
        });
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_only_old_temporary_files_are_removed() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("src")).unwrap();
        for name in [
            "src/.latt-write-abc123",
            "src/.latt-write-long-name",
            "src/kept.rs",
        ] {
            std::fs::write(root.join(name), "text").unwrap();
        }

        remove_leftover_writes(root, Duration::from_secs(3600));
        assert!(root.join("src/.latt-write-abc123").exists(), "too young");

        remove_leftover_writes(root, Duration::ZERO);
        assert!(!root.join("src/.latt-write-abc123").exists());
        assert!(root.join("src/.latt-write-long-name").exists());
        assert!(root.join("src/kept.rs").exists());
    }

    #[test]
    fn test_a_go_file_the_go_tool_does_not_build_is_left_out() {
        for (path, skipped) in [
            ("a.go", false),
            ("inner/a.go", false),
            ("vendor/x/a.go", true),
            ("inner/testdata/a.go", true),
            ("_mock/a.go", true),
            ("inner/_old/a.go", true),
            ("vendor/a.rs", false),
            ("testdata/a.py", false),
        ] {
            assert_eq!(go_tool_skips(Path::new(path)), skipped, "{path}");
        }
    }

    #[test]
    fn test_visits_what_a_scan_visits() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("skipped/deep")).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join(".gitignore"), "skipped/\n").unwrap();

        for (path, visited) in [
            ("src/new.rs", true),
            ("top.rs", true),
            ("skipped/new.rs", false),
            ("skipped/deep/new.rs", false),
            (".hidden.rs", false),
        ] {
            let path = root.join(path);
            std::fs::write(&path, "fn a() {}\n").unwrap();
            assert_eq!(visits(root, &path), visited, "{path:?}");
        }
    }

    #[tokio::test]
    async fn test_scan_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let scanner = Scanner::new(temp_dir.path());
        let files = scanner.scan(Known::new()).await.unwrap();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn test_scan_rust_files() {
        let temp_dir = TempDir::new().unwrap();

        // Create some Rust files
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();

        let mut main_rs = std::fs::File::create(src_dir.join("main.rs")).unwrap();
        main_rs.write_all(b"fn main() {}").unwrap();

        let mut lib_rs = std::fs::File::create(src_dir.join("lib.rs")).unwrap();
        lib_rs.write_all(b"pub fn lib() {}").unwrap();

        // Create a non-code file (should be ignored)
        let mut readme = std::fs::File::create(temp_dir.path().join("README.md")).unwrap();
        readme.write_all(b"# Readme").unwrap();

        let scanner = Scanner::new(temp_dir.path());
        let files = scanner.scan(Known::new()).await.unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.language == Language::Rust));
    }

    #[tokio::test]
    async fn test_scan_respects_gitignore() {
        let temp_dir = TempDir::new().unwrap();

        // Create .git directory to make this a git repository
        std::fs::create_dir(temp_dir.path().join(".git")).unwrap();

        // Create .gitignore
        let mut gitignore = std::fs::File::create(temp_dir.path().join(".gitignore")).unwrap();
        gitignore.write_all(b"target/\n").unwrap();

        // Create src file
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();
        let mut main_rs = std::fs::File::create(src_dir.join("main.rs")).unwrap();
        main_rs.write_all(b"fn main() {}").unwrap();

        // Create target file (should be ignored)
        let target_dir = temp_dir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        let mut ignored = std::fs::File::create(target_dir.join("build.rs")).unwrap();
        ignored.write_all(b"ignored").unwrap();

        let scanner = Scanner::new(temp_dir.path());
        let files = scanner.scan(Known::new()).await.unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].path.to_string_lossy().contains("main.rs"));
    }
}
