//! Snapshot data structures.

use chrono::{DateTime, Utc};
use mimir_types::{ContentHash, Language, SnapshotId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A point-in-time snapshot of the project state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unique identifier for this snapshot.
    pub id: SnapshotId,

    /// When the snapshot was created.
    pub created_at: DateTime<Utc>,

    /// Optional description.
    pub description: Option<String>,

    /// Map of file paths to their content hashes.
    pub file_hashes: HashMap<PathBuf, ContentHash>,

    /// Metadata about the snapshot.
    pub metadata: SnapshotMetadata,
}

/// Metadata about a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotMetadata {
    /// Number of files in the snapshot.
    pub files_count: usize,

    /// Total size of all files in bytes (uncompressed).
    pub total_size: u64,

    /// Number of files per language.
    pub languages: HashMap<Language, usize>,
}

impl Snapshot {
    /// Create a new snapshot.
    pub fn new(
        id: SnapshotId,
        file_hashes: HashMap<PathBuf, ContentHash>,
        description: Option<String>,
        metadata: SnapshotMetadata,
    ) -> Self {
        Self {
            id,
            created_at: Utc::now(),
            description,
            file_hashes,
            metadata,
        }
    }

    /// Get the number of files in this snapshot.
    pub fn file_count(&self) -> usize {
        self.file_hashes.len()
    }

    /// Check if a file is included in this snapshot.
    pub fn contains_file(&self, path: &PathBuf) -> bool {
        self.file_hashes.contains_key(path)
    }

    /// Get the content hash for a file.
    pub fn get_hash(&self, path: &PathBuf) -> Option<&ContentHash> {
        self.file_hashes.get(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_creation() {
        let mut file_hashes = HashMap::new();
        file_hashes.insert(
            PathBuf::from("src/main.rs"),
            ContentHash::compute(b"fn main() {}"),
        );

        let metadata = SnapshotMetadata {
            files_count: 1,
            total_size: 12,
            languages: HashMap::from([(Language::Rust, 1)]),
        };

        let snapshot = Snapshot::new(
            SnapshotId::new(1),
            file_hashes,
            Some("Initial snapshot".to_string()),
            metadata,
        );

        assert_eq!(snapshot.file_count(), 1);
        assert!(snapshot.contains_file(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = Snapshot::new(
            SnapshotId::new(42),
            HashMap::new(),
            None,
            SnapshotMetadata::default(),
        );

        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(snapshot.id, restored.id);
    }
}
