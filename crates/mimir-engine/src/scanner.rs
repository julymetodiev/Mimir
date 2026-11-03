//! Directory scanner.

use chrono::{DateTime, TimeZone, Utc};
use ignore::WalkBuilder;
use mimir_types::{ContentHash, Language};
use std::path::{Path, PathBuf};
use tokio::fs;

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

/// Directory scanner with gitignore support.
pub struct Scanner {
    root: PathBuf,
    max_file_size: u64,
}

impl Scanner {
    /// Create a new scanner for the given root directory.
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            max_file_size: 500 * 1024, // 500KB default
        }
    }

    /// Set the maximum file size to scan (in bytes).
    pub fn with_max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    /// Scan the directory and return all matching files.
    pub async fn scan(&self) -> std::io::Result<Vec<ScannedFile>> {
        let root = self.root.clone();
        let max_size = self.max_file_size;

        // Use ignore crate for gitignore support
        let walker = WalkBuilder::new(&root)
            .hidden(true) // Respect hidden files
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .build();

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
            if metadata.len() > max_size {
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

    /// Get the root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

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
