//! In-memory content store for testing.

use async_trait::async_trait;
use dashmap::DashMap;
use mimir_error::StorageError;
use mimir_types::ContentHash;

use crate::traits::ContentStore;

/// An in-memory content store, primarily for testing.
/// Uses DashMap for lock-free concurrent access.
pub struct MemoryContentStore {
    data: DashMap<ContentHash, Vec<u8>>,
}

impl MemoryContentStore {
    /// Create a new empty memory store.
    pub fn new() -> Self {
        Self {
            data: DashMap::new(),
        }
    }
}

impl Default for MemoryContentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContentStore for MemoryContentStore {
    async fn store(&self, content: &[u8]) -> Result<ContentHash, StorageError> {
        let hash = ContentHash::compute(content);
        self.data.entry(hash).or_insert_with(|| content.to_vec());
        Ok(hash)
    }

    async fn get(&self, hash: &ContentHash) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.data.get(hash).map(|v| v.value().clone()))
    }

    async fn exists(&self, hash: &ContentHash) -> Result<bool, StorageError> {
        Ok(self.data.contains_key(hash))
    }

    async fn delete(&self, hash: &ContentHash) -> Result<bool, StorageError> {
        Ok(self.data.remove(hash).is_some())
    }

    async fn list_hashes(&self) -> Result<Vec<ContentHash>, StorageError> {
        Ok(self.data.iter().map(|entry| *entry.key()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_and_get() {
        let store = MemoryContentStore::new();
        let content = b"hello world";

        let hash = store.store(content).await.unwrap();
        let retrieved = store.get(&hash).await.unwrap().unwrap();

        assert_eq!(content.as_slice(), retrieved.as_slice());
    }

    #[tokio::test]
    async fn test_deduplication() {
        let store = MemoryContentStore::new();
        let content = b"duplicate content";

        let hash1 = store.store(content).await.unwrap();
        let hash2 = store.store(content).await.unwrap();

        assert_eq!(hash1, hash2);
        assert_eq!(store.list_hashes().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_exists() {
        let store = MemoryContentStore::new();
        let content = b"test";

        let hash = store.store(content).await.unwrap();

        assert!(store.exists(&hash).await.unwrap());

        let fake_hash = ContentHash::compute(b"nonexistent");
        assert!(!store.exists(&fake_hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let store = MemoryContentStore::new();
        let content = b"to be deleted";

        let hash = store.store(content).await.unwrap();
        assert!(store.exists(&hash).await.unwrap());

        let deleted = store.delete(&hash).await.unwrap();
        assert!(deleted);
        assert!(!store.exists(&hash).await.unwrap());

        // Deleting again should return false
        let deleted_again = store.delete(&hash).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        use std::sync::Arc;

        let store = Arc::new(MemoryContentStore::new());
        let mut handles = vec![];

        // Spawn multiple tasks writing concurrently
        for i in 0..10 {
            let store = Arc::clone(&store);
            handles.push(tokio::spawn(async move {
                let content = format!("content {}", i);
                store.store(content.as_bytes()).await.unwrap()
            }));
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }

        // Should have 10 unique entries
        assert_eq!(store.list_hashes().await.unwrap().len(), 10);
    }
}
