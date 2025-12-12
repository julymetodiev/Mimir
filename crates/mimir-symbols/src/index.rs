//! Index structures for efficient symbol lookups.

use dashmap::DashMap;
use mimir_types::{FileId, SymbolId};

/// Index from symbol names to symbol IDs.
///
/// Allows multiple symbols with the same name (common in different modules).
#[derive(Debug, Default)]
pub struct NameIndex {
    /// Maps symbol name → list of symbol IDs with that name.
    index: DashMap<String, Vec<SymbolId>>,
}

impl NameIndex {
    /// Create a new empty name index.
    pub fn new() -> Self {
        Self {
            index: DashMap::new(),
        }
    }

    /// Add a symbol to the index.
    pub fn insert(&self, name: String, symbol_id: SymbolId) {
        self.index
            .entry(name)
            .or_default()
            .push(symbol_id);
    }

    /// Look up symbols by name.
    pub fn get(&self, name: &str) -> Vec<SymbolId> {
        self.index
            .get(name)
            .map(|ids| ids.clone())
            .unwrap_or_default()
    }

    /// Remove a symbol from the index.
    pub fn remove(&self, name: &str, symbol_id: SymbolId) {
        if let Some(mut ids) = self.index.get_mut(name) {
            ids.retain(|id| *id != symbol_id);
            if ids.is_empty() {
                drop(ids);
                self.index.remove(name);
            }
        }
    }

    /// Get all names in the index.
    #[allow(dead_code)]
    pub fn names(&self) -> Vec<String> {
        self.index.iter().map(|r| r.key().clone()).collect()
    }

    /// Get the number of unique names.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Check if the index is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.index.clear();
    }
}

/// Index from file IDs to symbol IDs.
///
/// Allows efficient lookup of all symbols in a file.
#[derive(Debug, Default)]
pub struct FileIndex {
    /// Maps file ID → list of symbol IDs in that file.
    index: DashMap<FileId, Vec<SymbolId>>,
}

impl FileIndex {
    /// Create a new empty file index.
    pub fn new() -> Self {
        Self {
            index: DashMap::new(),
        }
    }

    /// Add a symbol to the index.
    pub fn insert(&self, file_id: FileId, symbol_id: SymbolId) {
        self.index
            .entry(file_id)
            .or_default()
            .push(symbol_id);
    }

    /// Get all symbols in a file.
    pub fn get(&self, file_id: FileId) -> Vec<SymbolId> {
        self.index
            .get(&file_id)
            .map(|ids| ids.clone())
            .unwrap_or_default()
    }

    /// Remove a symbol from the index.
    pub fn remove(&self, file_id: FileId, symbol_id: SymbolId) {
        if let Some(mut ids) = self.index.get_mut(&file_id) {
            ids.retain(|id| *id != symbol_id);
            if ids.is_empty() {
                drop(ids);
                self.index.remove(&file_id);
            }
        }
    }

    /// Remove all symbols for a file.
    pub fn remove_file(&self, file_id: FileId) -> Vec<SymbolId> {
        self.index
            .remove(&file_id)
            .map(|(_, ids)| ids)
            .unwrap_or_default()
    }

    /// Get all file IDs with symbols.
    #[allow(dead_code)]
    pub fn files(&self) -> Vec<FileId> {
        self.index.iter().map(|r| *r.key()).collect()
    }

    /// Get the number of files with symbols.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Check if the index is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.index.clear();
    }
}

/// Index from qualified names to symbol IDs.
///
/// Uses the fully qualified path for unique lookups.
#[derive(Debug, Default)]
pub struct QualifiedIndex {
    /// Maps qualified name string → symbol ID.
    index: DashMap<String, SymbolId>,
}

impl QualifiedIndex {
    /// Create a new empty qualified index.
    pub fn new() -> Self {
        Self {
            index: DashMap::new(),
        }
    }

    /// Insert a qualified name mapping.
    pub fn insert(&self, qualified_name: String, symbol_id: SymbolId) {
        self.index.insert(qualified_name, symbol_id);
    }

    /// Look up a symbol by qualified name.
    pub fn get(&self, qualified_name: &str) -> Option<SymbolId> {
        self.index.get(qualified_name).map(|r| *r)
    }

    /// Remove a qualified name mapping.
    pub fn remove(&self, qualified_name: &str) -> Option<SymbolId> {
        self.index.remove(qualified_name).map(|(_, id)| id)
    }

    /// Get the number of qualified names.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Check if the index is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.index.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::IdGenerator;

    fn ids() -> IdGenerator {
        IdGenerator::new()
    }

    #[test]
    fn test_name_index_basic() {
        let ids = ids();
        let index = NameIndex::new();
        let id1: SymbolId = ids.next();
        let id2: SymbolId = ids.next();

        index.insert("foo".to_string(), id1);
        index.insert("foo".to_string(), id2);
        index.insert("bar".to_string(), id1);

        let foo_ids = index.get("foo");
        assert_eq!(foo_ids.len(), 2);
        assert!(foo_ids.contains(&id1));
        assert!(foo_ids.contains(&id2));

        let bar_ids = index.get("bar");
        assert_eq!(bar_ids.len(), 1);
        assert!(bar_ids.contains(&id1));

        assert!(index.get("baz").is_empty());
    }

    #[test]
    fn test_name_index_remove() {
        let ids = ids();
        let index = NameIndex::new();
        let id1: SymbolId = ids.next();
        let id2: SymbolId = ids.next();

        index.insert("foo".to_string(), id1);
        index.insert("foo".to_string(), id2);

        index.remove("foo", id1);
        let result = index.get("foo");
        assert_eq!(result.len(), 1);
        assert!(result.contains(&id2));

        index.remove("foo", id2);
        assert!(index.get("foo").is_empty());
        assert!(index.is_empty());
    }

    #[test]
    fn test_file_index_basic() {
        let ids = ids();
        let index = FileIndex::new();
        let file1: FileId = ids.next();
        let file2: FileId = ids.next();
        let sym1: SymbolId = ids.next();
        let sym2: SymbolId = ids.next();
        let sym3: SymbolId = ids.next();

        index.insert(file1, sym1);
        index.insert(file1, sym2);
        index.insert(file2, sym3);

        let file1_syms = index.get(file1);
        assert_eq!(file1_syms.len(), 2);
        assert!(file1_syms.contains(&sym1));
        assert!(file1_syms.contains(&sym2));

        let file2_syms = index.get(file2);
        assert_eq!(file2_syms.len(), 1);
        assert!(file2_syms.contains(&sym3));
    }

    #[test]
    fn test_file_index_remove_file() {
        let ids = ids();
        let index = FileIndex::new();
        let file: FileId = ids.next();
        let sym1: SymbolId = ids.next();
        let sym2: SymbolId = ids.next();

        index.insert(file, sym1);
        index.insert(file, sym2);

        let removed = index.remove_file(file);
        assert_eq!(removed.len(), 2);
        assert!(index.get(file).is_empty());
    }

    #[test]
    fn test_qualified_index() {
        let ids = ids();
        let index = QualifiedIndex::new();
        let id1: SymbolId = ids.next();
        let id2: SymbolId = ids.next();

        index.insert("crate::module::foo".to_string(), id1);
        index.insert("crate::module::bar".to_string(), id2);

        assert_eq!(index.get("crate::module::foo"), Some(id1));
        assert_eq!(index.get("crate::module::bar"), Some(id2));
        assert_eq!(index.get("crate::module::baz"), None);

        index.remove("crate::module::foo");
        assert_eq!(index.get("crate::module::foo"), None);
    }
}
