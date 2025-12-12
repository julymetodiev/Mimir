//! FilesHandle for read/write access to file index.

use mimir_content::ContentStore;
use mimir_types::FileId;
use std::path::Path;
use std::sync::Arc;

use crate::entry::FileEntry;
use crate::index::FileIndex;

/// A handle for accessing the file index.
///
/// This follows the Handle pattern: cheap to clone, thread-safe.
pub struct FilesHandle<S: ContentStore> {
    index: Arc<FileIndex>,
    content_store: Arc<S>,
}

impl<S: ContentStore> FilesHandle<S> {
    /// Create a new files handle.
    pub fn new(index: Arc<FileIndex>, content_store: Arc<S>) -> Self {
        Self {
            index,
            content_store,
        }
    }

    /// Get a file entry by ID.
    pub fn get_by_id(&self, id: FileId) -> Option<FileEntry> {
        self.index.get_by_id(id)
    }

    /// Get a file entry by path.
    pub fn get_by_path(&self, path: &Path) -> Option<FileEntry> {
        self.index.get_by_path(path)
    }

    /// Check if a path is indexed.
    pub fn contains(&self, path: &Path) -> bool {
        self.index.contains_path(path)
    }

    /// Get the number of indexed files.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Get all file entries.
    pub fn all_entries(&self) -> Vec<FileEntry> {
        self.index.iter().collect()
    }

    /// Get the content store.
    pub fn content_store(&self) -> &Arc<S> {
        &self.content_store
    }

    /// Get the file index.
    pub fn index(&self) -> &Arc<FileIndex> {
        &self.index
    }
}

impl<S: ContentStore> Clone for FilesHandle<S> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            content_store: self.content_store.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mimir_content::MemoryContentStore;
    use mimir_types::{ContentHash, Language};
    use std::path::PathBuf;

    #[test]
    fn test_handle_clone() {
        let index = Arc::new(FileIndex::new());
        let store = Arc::new(MemoryContentStore::new());

        let handle1 = FilesHandle::new(index, store);
        let handle2 = handle1.clone();

        // Both handles should see the same data
        let entry = FileEntry::new(
            FileId::new(0),
            PathBuf::from("test.rs"),
            Language::Rust,
            ContentHash::compute(b"test"),
            100,
            Utc::now(),
        );

        handle1.index.insert(entry);

        assert_eq!(handle1.len(), 1);
        assert_eq!(handle2.len(), 1);
    }
}
