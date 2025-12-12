//! File entry structure.

use chrono::{DateTime, Utc};
use mimir_types::{ContentHash, FileId, Language};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Information about an indexed file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Unique identifier for this file.
    pub id: FileId,

    /// Path relative to project root.
    pub path: PathBuf,

    /// Detected programming language.
    pub language: Language,

    /// Hash of the file content.
    pub content_hash: ContentHash,

    /// Size in bytes.
    pub size_bytes: u64,

    /// Last modification time from filesystem.
    pub modified_at: DateTime<Utc>,

    /// When this file was indexed.
    pub indexed_at: DateTime<Utc>,
}

impl FileEntry {
    /// Create a new file entry.
    pub fn new(
        id: FileId,
        path: PathBuf,
        language: Language,
        content_hash: ContentHash,
        size_bytes: u64,
        modified_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            path,
            language,
            content_hash,
            size_bytes,
            modified_at,
            indexed_at: Utc::now(),
        }
    }

    /// Get the file extension.
    pub fn extension(&self) -> Option<&str> {
        self.path.extension().and_then(|ext| ext.to_str())
    }

    /// Get the file name.
    pub fn file_name(&self) -> Option<&str> {
        self.path.file_name().and_then(|name| name.to_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_entry() {
        let entry = FileEntry::new(
            FileId::new(1),
            PathBuf::from("src/main.rs"),
            Language::Rust,
            ContentHash::compute(b"content"),
            100,
            Utc::now(),
        );

        assert_eq!(entry.extension(), Some("rs"));
        assert_eq!(entry.file_name(), Some("main.rs"));
    }
}
