//! Snapshot restoration.

use mimir_content::ContentStore;
use mimir_error::StorageError;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::snapshot::Snapshot;

/// Restore a snapshot to a target directory.
pub async fn restore_snapshot<S: ContentStore>(
    snapshot: &Snapshot,
    content_store: &S,
    target_dir: &Path,
) -> Result<RestoreStats, StorageError> {
    let mut stats = RestoreStats::default();

    for (path, hash) in &snapshot.file_hashes {
        let target_path = target_dir.join(path);

        // Ensure parent directory exists
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::WriteFailed {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
        }

        // Get content from store
        let content = content_store
            .get(hash)
            .await?
            .ok_or(StorageError::ContentNotFound { hash: *hash })?;

        // Write to file
        let mut file =
            fs::File::create(&target_path)
                .await
                .map_err(|e| StorageError::WriteFailed {
                    path: target_path.clone(),
                    source: e,
                })?;

        file.write_all(&content)
            .await
            .map_err(|e| StorageError::WriteFailed {
                path: target_path.clone(),
                source: e,
            })?;

        stats.files_restored += 1;
        stats.bytes_written += content.len() as u64;
    }

    Ok(stats)
}

/// Statistics about a restore operation.
#[derive(Debug, Default)]
pub struct RestoreStats {
    /// Number of files restored.
    pub files_restored: usize,
    /// Total bytes written.
    pub bytes_written: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_content::MemoryContentStore;
    use mimir_types::{Language, SnapshotId};
    use std::collections::HashMap;
    use tempfile::TempDir;

    use crate::snapshot::SnapshotMetadata;

    #[tokio::test]
    async fn test_restore_snapshot() {
        let store = MemoryContentStore::new();
        let temp_dir = TempDir::new().unwrap();

        // Store some content
        let content1 = b"fn main() {}";
        let content2 = b"pub fn lib() {}";

        let hash1 = store.store(content1).await.unwrap();
        let hash2 = store.store(content2).await.unwrap();

        // Create snapshot
        let mut file_hashes = HashMap::new();
        file_hashes.insert("src/main.rs".into(), hash1);
        file_hashes.insert("src/lib.rs".into(), hash2);

        let snapshot = Snapshot::new(
            SnapshotId::new(1),
            file_hashes,
            None,
            SnapshotMetadata {
                files_count: 2,
                total_size: (content1.len() + content2.len()) as u64,
                languages: HashMap::from([(Language::Rust, 2)]),
            },
        );

        // Restore
        let stats = restore_snapshot(&snapshot, &store, temp_dir.path())
            .await
            .unwrap();

        assert_eq!(stats.files_restored, 2);

        // Verify files exist
        let main_rs = temp_dir.path().join("src/main.rs");
        let lib_rs = temp_dir.path().join("src/lib.rs");

        assert!(main_rs.exists());
        assert!(lib_rs.exists());

        // Verify content
        let restored_main = fs::read(&main_rs).await.unwrap();
        assert_eq!(restored_main, content1);
    }
}
