//! File index with lock-free reads.

use dashmap::DashMap;
use mimir_types::FileId;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::entry::FileEntry;

/// A lock-free file index.
pub struct FileIndex {
    /// Map from FileId to FileEntry.
    by_id: DashMap<FileId, FileEntry>,

    /// Map from path to FileId.
    by_path: DashMap<PathBuf, FileId>,

    /// Next ID to assign.
    next_id: AtomicU64,
}

impl FileIndex {
    /// Create a new empty file index.
    pub fn new() -> Self {
        Self {
            by_id: DashMap::new(),
            by_path: DashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Insert a file entry, returning the assigned ID.
    pub fn insert(&self, mut entry: FileEntry) -> FileId {
        // Check if path already exists
        if let Some(existing_id) = self.by_path.get(&entry.path) {
            let id = *existing_id;
            entry.id = id;
            self.by_id.insert(id, entry);
            return id;
        }

        // Assign new ID
        let id = FileId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        entry.id = id;

        self.by_path.insert(entry.path.clone(), id);
        self.by_id.insert(id, entry);

        id
    }

    /// Get a file entry by ID.
    pub fn get_by_id(&self, id: FileId) -> Option<FileEntry> {
        self.by_id.get(&id).map(|entry| entry.clone())
    }

    /// Get a file entry by path.
    pub fn get_by_path(&self, path: &Path) -> Option<FileEntry> {
        let id = self.by_path.get(path)?;
        self.by_id.get(&*id).map(|entry| entry.clone())
    }

    /// Remove a file by path.
    pub fn remove(&self, path: &Path) -> Option<FileEntry> {
        let (_, id) = self.by_path.remove(path)?;
        let (_, entry) = self.by_id.remove(&id)?;
        Some(entry)
    }

    /// Remove a file by ID.
    pub fn remove_by_id(&self, id: FileId) -> Option<FileEntry> {
        let (_, entry) = self.by_id.remove(&id)?;
        self.by_path.remove(&entry.path);
        Some(entry)
    }

    /// Check if a path is in the index.
    pub fn contains_path(&self, path: &Path) -> bool {
        self.by_path.contains_key(path)
    }

    /// Get the number of files in the index.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Iterate over all file entries.
    pub fn iter(&self) -> impl Iterator<Item = FileEntry> + '_ {
        self.by_id.iter().map(|entry| entry.value().clone())
    }

    /// Get all paths in the index.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.by_path.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Clear the index.
    pub fn clear(&self) {
        self.by_id.clear();
        self.by_path.clear();
    }
}

impl Default for FileIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mimir_types::{ContentHash, Language};

    fn make_entry(path: &str) -> FileEntry {
        FileEntry::new(
            FileId::new(0), // Will be overwritten
            PathBuf::from(path),
            Language::Rust,
            ContentHash::compute(path.as_bytes()),
            100,
            Utc::now(),
        )
    }

    #[test]
    fn test_insert_and_get() {
        let index = FileIndex::new();
        let entry = make_entry("src/main.rs");

        let id = index.insert(entry.clone());

        let by_id = index.get_by_id(id).unwrap();
        assert_eq!(by_id.path, PathBuf::from("src/main.rs"));

        let by_path = index.get_by_path(Path::new("src/main.rs")).unwrap();
        assert_eq!(by_path.id, id);
    }

    #[test]
    fn test_update_existing() {
        let index = FileIndex::new();

        let entry1 = make_entry("src/main.rs");
        let id1 = index.insert(entry1);

        let entry2 = make_entry("src/main.rs");
        let id2 = index.insert(entry2);

        // Should reuse same ID
        assert_eq!(id1, id2);
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_remove() {
        let index = FileIndex::new();
        let entry = make_entry("src/main.rs");

        index.insert(entry);
        assert!(index.contains_path(Path::new("src/main.rs")));

        let removed = index.remove(Path::new("src/main.rs")).unwrap();
        assert_eq!(removed.path, PathBuf::from("src/main.rs"));
        assert!(!index.contains_path(Path::new("src/main.rs")));
    }

    #[test]
    fn test_iter() {
        let index = FileIndex::new();
        index.insert(make_entry("a.rs"));
        index.insert(make_entry("b.rs"));
        index.insert(make_entry("c.rs"));

        let entries: Vec<_> = index.iter().collect();
        assert_eq!(entries.len(), 3);
    }
}
