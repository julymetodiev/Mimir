//! Snapshot creation.

use mimir_error::StorageError;
use mimir_types::{ContentHash, Language, SnapshotId};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::snapshot::{Snapshot, SnapshotMetadata};

/// Information about a file to include in a snapshot.
pub struct FileInfo {
    pub path: PathBuf,
    pub content_hash: ContentHash,
    pub size: u64,
    pub language: Language,
}

/// Create a snapshot from file information.
pub fn create_snapshot(
    id: SnapshotId,
    files: Vec<FileInfo>,
    description: Option<String>,
) -> Result<Snapshot, StorageError> {
    let mut file_hashes = HashMap::new();
    let mut languages: HashMap<Language, usize> = HashMap::new();
    let mut total_size = 0u64;

    for file in &files {
        file_hashes.insert(file.path.clone(), file.content_hash);
        total_size += file.size;
        *languages.entry(file.language).or_insert(0) += 1;
    }

    let metadata = SnapshotMetadata {
        files_count: files.len(),
        total_size,
        languages,
    };

    Ok(Snapshot::new(id, file_hashes, description, metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_snapshot() {
        let files = vec![
            FileInfo {
                path: PathBuf::from("src/main.rs"),
                content_hash: ContentHash::compute(b"main"),
                size: 100,
                language: Language::Rust,
            },
            FileInfo {
                path: PathBuf::from("src/lib.rs"),
                content_hash: ContentHash::compute(b"lib"),
                size: 200,
                language: Language::Rust,
            },
        ];

        let snapshot =
            create_snapshot(SnapshotId::new(1), files, Some("Test".to_string())).unwrap();

        assert_eq!(snapshot.file_count(), 2);
        assert_eq!(snapshot.metadata.total_size, 300);
        assert_eq!(snapshot.metadata.languages.get(&Language::Rust), Some(&2));
    }
}
