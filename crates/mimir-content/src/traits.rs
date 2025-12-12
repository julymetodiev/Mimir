//! ContentStore trait definition.

use async_trait::async_trait;
use mimir_error::StorageError;
use mimir_types::ContentHash;

/// Content-addressed storage trait.
///
/// Stores arbitrary bytes indexed by their Blake3 hash.
/// Same content = same hash = automatic deduplication.
#[async_trait]
pub trait ContentStore: Send + Sync {
    /// Store content, returning its hash.
    ///
    /// If content already exists, returns existing hash without re-storing.
    async fn store(&self, content: &[u8]) -> Result<ContentHash, StorageError>;

    /// Retrieve content by hash.
    async fn get(&self, hash: &ContentHash) -> Result<Option<Vec<u8>>, StorageError>;

    /// Check if content exists.
    async fn exists(&self, hash: &ContentHash) -> Result<bool, StorageError>;

    /// Delete content (for garbage collection).
    async fn delete(&self, hash: &ContentHash) -> Result<bool, StorageError>;

    /// List all stored hashes.
    async fn list_hashes(&self) -> Result<Vec<ContentHash>, StorageError>;

    /// Get the total number of stored items.
    async fn count(&self) -> Result<usize, StorageError> {
        Ok(self.list_hashes().await?.len())
    }
}

/// Extension methods for ContentStore.
#[allow(dead_code)]
#[async_trait]
pub trait ContentStoreExt: ContentStore {
    /// Store a string as content.
    async fn store_string(&self, content: &str) -> Result<ContentHash, StorageError> {
        self.store(content.as_bytes()).await
    }

    /// Get content as a string.
    async fn get_string(&self, hash: &ContentHash) -> Result<Option<String>, StorageError> {
        match self.get(hash).await? {
            Some(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            None => Ok(None),
        }
    }
}

// Blanket implementation
impl<T: ContentStore> ContentStoreExt for T {}
