//! Directory scanner.

use chrono::{DateTime, TimeZone, Utc};
use ignore::WalkBuilder;
use mimir_edit::is_temporary;
use mimir_types::{ContentHash, Language};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;
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
        && visits(root, path)
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

    /// Scan the directory and return all matching files.
    pub async fn scan(&self) -> std::io::Result<Vec<ScannedFile>> {
        let root = self.root.clone();

        let walker = walker(&root).build();

        let mut files = Vec::new();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Get metadata
            let metadata = match fs::metadata(path).await {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Skip files that are too large
            if metadata.len() > MAX_FILE_BYTES {
                continue;
            }

            // Detect language
            let language = Language::from_path(path);

            // Skip unknown languages (non-code files)
            if !language.is_known() {
                continue;
            }

            // Read and hash content
            let content = match fs::read(path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let content_hash = ContentHash::compute(&content);

            // Get modification time
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| Utc.timestamp_opt(d.as_secs() as i64, 0).single())
                .unwrap_or_else(Utc::now);

            // Compute relative path
            let relative_path = path.strip_prefix(&root).unwrap_or(path).to_path_buf();

            files.push(ScannedFile {
                path: relative_path,
                absolute_path: path.to_path_buf(),
                language,
                size: metadata.len(),
                modified_at,
                content_hash,
            });
        }

        Ok(files)
    }
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
            "src/.mimir-write-abc123",
            "src/.mimir-write-long-name",
            "src/kept.rs",
        ] {
            std::fs::write(root.join(name), "text").unwrap();
        }

        remove_leftover_writes(root, Duration::from_secs(3600));
        assert!(root.join("src/.mimir-write-abc123").exists(), "too young");

        remove_leftover_writes(root, Duration::ZERO);
        assert!(!root.join("src/.mimir-write-abc123").exists());
        assert!(root.join("src/.mimir-write-long-name").exists());
        assert!(root.join("src/kept.rs").exists());
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
        let files = scanner.scan().await.unwrap();
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
        let files = scanner.scan().await.unwrap();

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
        let files = scanner.scan().await.unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].path.to_string_lossy().contains("main.rs"));
    }
}
