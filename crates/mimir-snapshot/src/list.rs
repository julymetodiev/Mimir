//! Snapshot listing.

use mimir_error::StorageError;
use std::path::Path;
use tokio::fs;

use crate::snapshot::Snapshot;

/// List all snapshots in a directory.
pub async fn list_snapshots(snapshots_dir: &Path) -> Result<Vec<Snapshot>, StorageError> {
    let mut snapshots = Vec::new();

    if !snapshots_dir.exists() {
        return Ok(snapshots);
    }

    let mut entries = fs::read_dir(snapshots_dir).await.map_err(|e| {
        StorageError::ReadFailed {
            path: snapshots_dir.to_path_buf(),
            source: e,
        }
    })?;

    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        StorageError::ReadFailed {
            path: snapshots_dir.to_path_buf(),
            source: e,
        }
    })? {
        let path = entry.path();

        if path.is_dir() {
            let manifest_path = path.join("manifest.json");
            if manifest_path.exists() {
                let content =
                    fs::read_to_string(&manifest_path).await.map_err(|e| {
                        StorageError::ReadFailed {
                            path: manifest_path.clone(),
                            source: e,
                        }
                    })?;

                let snapshot: Snapshot = serde_json::from_str(&content).map_err(|e| {
                    StorageError::Deserialization(format!(
                        "Failed to parse {}: {}",
                        manifest_path.display(),
                        e
                    ))
                })?;

                snapshots.push(snapshot);
            }
        }
    }

    // Sort by creation time, newest first
    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(snapshots)
}

/// Get a single snapshot by ID.
pub async fn get_snapshot(
    snapshots_dir: &Path,
    id: &str,
) -> Result<Option<Snapshot>, StorageError> {
    let snapshot_dir = snapshots_dir.join(format!("snap_{}", id));
    let manifest_path = snapshot_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&manifest_path).await.map_err(|e| {
        StorageError::ReadFailed {
            path: manifest_path.clone(),
            source: e,
        }
    })?;

    let snapshot: Snapshot = serde_json::from_str(&content).map_err(|e| {
        StorageError::Deserialization(format!(
            "Failed to parse {}: {}",
            manifest_path.display(),
            e
        ))
    })?;

    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_empty() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots = list_snapshots(temp_dir.path()).await.unwrap();
        assert!(snapshots.is_empty());
    }

    #[tokio::test]
    async fn test_list_nonexistent() {
        let snapshots = list_snapshots(Path::new("/nonexistent/path")).await.unwrap();
        assert!(snapshots.is_empty());
    }
}
