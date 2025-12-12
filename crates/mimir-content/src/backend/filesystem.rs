//! Filesystem-based content store with zstd compression.

use async_trait::async_trait;
use mimir_error::StorageError;
use mimir_types::ContentHash;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::compression::decompress;
use crate::traits::ContentStore;

/// A filesystem-based content store with zstd compression.
///
/// Files are stored using content-addressed paths:
/// `{root}/objects/{first_2_hex_chars}/{remaining_hex_chars}`
pub struct FilesystemContentStore {
    root: PathBuf,
    compression_level: i32,
}

impl FilesystemContentStore {
    /// Create a new filesystem content store at the given root directory.
    pub async fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref().to_path_buf();
        let objects_dir = root.join("objects");

        fs::create_dir_all(&objects_dir).await.map_err(|e| StorageError::WriteFailed {
            path: objects_dir.clone(),
            source: e,
        })?;

        Ok(Self {
            root,
            compression_level: crate::compression::DEFAULT_COMPRESSION_LEVEL,
        })
    }

    /// Create a new store with a specific compression level.
    pub async fn with_compression_level(
        root: impl AsRef<Path>,
        level: i32,
    ) -> Result<Self, StorageError> {
        let mut store = Self::new(root).await?;
        store.compression_level = level;
        Ok(store)
    }

    /// Get the path for a given content hash.
    fn hash_to_path(&self, hash: &ContentHash) -> PathBuf {
        let (dir, file) = hash.to_path_parts();
        self.root.join("objects").join(dir).join(file)
    }

    /// Get the root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[async_trait]
impl ContentStore for FilesystemContentStore {
    async fn store(&self, content: &[u8]) -> Result<ContentHash, StorageError> {
        let hash = ContentHash::compute(content);
        let path = self.hash_to_path(&hash);

        // Check if already exists (deduplication)
        if path.exists() {
            return Ok(hash);
        }

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| StorageError::WriteFailed {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        // Compress content
        let compressed =
            crate::compression::compress_with_level(content, self.compression_level)?;

        // Write atomically: write to temp file, then rename
        let temp_path = path.with_extension("tmp");

        let mut file = fs::File::create(&temp_path).await.map_err(|e| StorageError::WriteFailed {
            path: temp_path.clone(),
            source: e,
        })?;

        file.write_all(&compressed).await.map_err(|e| StorageError::WriteFailed {
            path: temp_path.clone(),
            source: e,
        })?;

        file.sync_all().await.map_err(|e| StorageError::WriteFailed {
            path: temp_path.clone(),
            source: e,
        })?;

        // Atomic rename
        fs::rename(&temp_path, &path).await.map_err(|e| StorageError::WriteFailed {
            path: path.clone(),
            source: e,
        })?;

        Ok(hash)
    }

    async fn get(&self, hash: &ContentHash) -> Result<Option<Vec<u8>>, StorageError> {
        let path = self.hash_to_path(hash);

        if !path.exists() {
            return Ok(None);
        }

        let compressed = fs::read(&path).await.map_err(|e| StorageError::ReadFailed {
            path: path.clone(),
            source: e,
        })?;

        let content = decompress(&compressed)?;
        Ok(Some(content))
    }

    async fn exists(&self, hash: &ContentHash) -> Result<bool, StorageError> {
        let path = self.hash_to_path(hash);
        Ok(path.exists())
    }

    async fn delete(&self, hash: &ContentHash) -> Result<bool, StorageError> {
        let path = self.hash_to_path(hash);

        if !path.exists() {
            return Ok(false);
        }

        fs::remove_file(&path).await.map_err(|e| StorageError::DeleteFailed {
            path: path.clone(),
            source: e,
        })?;

        Ok(true)
    }

    async fn list_hashes(&self) -> Result<Vec<ContentHash>, StorageError> {
        let objects_dir = self.root.join("objects");
        let mut hashes = Vec::new();

        if !objects_dir.exists() {
            return Ok(hashes);
        }

        let mut dir_entries = fs::read_dir(&objects_dir).await.map_err(|e| {
            StorageError::ReadFailed {
                path: objects_dir.clone(),
                source: e,
            }
        })?;

        while let Some(entry) = dir_entries.next_entry().await.map_err(|e| {
            StorageError::ReadFailed {
                path: objects_dir.clone(),
                source: e,
            }
        })? {
            let subdir = entry.path();
            if !subdir.is_dir() {
                continue;
            }

            let prefix = entry.file_name().to_string_lossy().to_string();

            let mut file_entries = fs::read_dir(&subdir).await.map_err(|e| {
                StorageError::ReadFailed {
                    path: subdir.clone(),
                    source: e,
                }
            })?;

            while let Some(file_entry) = file_entries.next_entry().await.map_err(|e| {
                StorageError::ReadFailed {
                    path: subdir.clone(),
                    source: e,
                }
            })? {
                let suffix = file_entry.file_name().to_string_lossy().to_string();

                // Skip temp files
                if suffix.ends_with(".tmp") {
                    continue;
                }

                let hex = format!("{}{}", prefix, suffix);
                if let Some(hash) = ContentHash::from_hex(&hex) {
                    hashes.push(hash);
                }
            }
        }

        Ok(hashes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_temp_store() -> (FilesystemContentStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemContentStore::new(temp_dir.path()).await.unwrap();
        (store, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_get() {
        let (store, _temp) = create_temp_store().await;
        let content = b"hello world";

        let hash = store.store(content).await.unwrap();
        let retrieved = store.get(&hash).await.unwrap().unwrap();

        assert_eq!(content.as_slice(), retrieved.as_slice());
    }

    #[tokio::test]
    async fn test_deduplication() {
        let (store, _temp) = create_temp_store().await;
        let content = b"duplicate content";

        let hash1 = store.store(content).await.unwrap();
        let hash2 = store.store(content).await.unwrap();

        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_compression() {
        let (store, temp) = create_temp_store().await;
        let content = b"compressible content ".repeat(1000);

        let hash = store.store(&content).await.unwrap();

        // Check that stored file is smaller than original
        let path = store.hash_to_path(&hash);
        let stored_size = fs::metadata(&path).await.unwrap().len();

        assert!(stored_size < content.len() as u64);

        // But we can still get back the original
        let retrieved = store.get(&hash).await.unwrap().unwrap();
        assert_eq!(content.as_slice(), retrieved.as_slice());

        drop(temp);
    }

    #[tokio::test]
    async fn test_list_hashes() {
        let (store, _temp) = create_temp_store().await;

        store.store(b"content 1").await.unwrap();
        store.store(b"content 2").await.unwrap();
        store.store(b"content 3").await.unwrap();

        let hashes = store.list_hashes().await.unwrap();
        assert_eq!(hashes.len(), 3);
    }

    #[tokio::test]
    async fn test_delete() {
        let (store, _temp) = create_temp_store().await;
        let content = b"to delete";

        let hash = store.store(content).await.unwrap();
        assert!(store.exists(&hash).await.unwrap());

        let deleted = store.delete(&hash).await.unwrap();
        assert!(deleted);
        assert!(!store.exists(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let content = b"persistent content";
        let hash;

        // Store content
        {
            let store = FilesystemContentStore::new(temp_dir.path()).await.unwrap();
            hash = store.store(content).await.unwrap();
        }

        // Create new store instance and verify content persists
        {
            let store = FilesystemContentStore::new(temp_dir.path()).await.unwrap();
            let retrieved = store.get(&hash).await.unwrap().unwrap();
            assert_eq!(content.as_slice(), retrieved.as_slice());
        }
    }
}
