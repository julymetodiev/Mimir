//! Symbol table implementation.

use dashmap::DashMap;
use mimir_types::{FileId, Symbol, SymbolId};
use std::sync::Arc;

use crate::symbols::index::{FileIndex, NameIndex, QualifiedIndex};

/// The main symbol storage and indexing structure.
///
/// Thread-safe symbol table using DashMap for concurrent access.
/// Maintains indices for efficient lookups by name, file, and qualified path.
#[derive(Debug)]
pub struct SymbolTable {
    /// Primary storage: symbol ID → symbol data.
    symbols: DashMap<SymbolId, Arc<Symbol>>,

    /// Index by simple name.
    name_index: NameIndex,

    /// Index by file ID.
    file_index: FileIndex,

    /// Index by fully qualified name.
    qualified_index: QualifiedIndex,
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolTable {
    /// Create a new empty symbol table.
    pub fn new() -> Self {
        Self {
            symbols: DashMap::new(),
            name_index: NameIndex::new(),
            file_index: FileIndex::new(),
            qualified_index: QualifiedIndex::new(),
        }
    }

    /// Insert a symbol into the table.
    ///
    /// Returns the previous symbol with this ID if any.
    pub fn insert(&self, symbol: Symbol) -> Option<Arc<Symbol>> {
        let id = symbol.id;
        let name = symbol.name.clone();
        let file_id = symbol.file_id;
        let qualified = symbol.qualified_name.join("::");

        // Update indices
        self.name_index.insert(name, id);
        self.file_index.insert(file_id, id);
        self.qualified_index.insert(qualified, id);

        // Insert symbol
        self.symbols.insert(id, Arc::new(symbol))
    }

    /// Get a symbol by ID.
    pub fn get(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        self.symbols.get(&id).map(|r| Arc::clone(&*r))
    }

    /// Remove a symbol by ID.
    ///
    /// Returns the removed symbol if it existed.
    pub fn remove(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        if let Some((_, symbol)) = self.symbols.remove(&id) {
            // Remove from indices
            self.name_index.remove(&symbol.name, id);
            self.file_index.remove(symbol.file_id, id);
            self.qualified_index
                .remove(&symbol.qualified_name.join("::"));
            Some(symbol)
        } else {
            None
        }
    }

    /// Find symbols by simple name.
    pub fn find_by_name(&self, name: &str) -> Vec<Arc<Symbol>> {
        self.name_index
            .get(name)
            .into_iter()
            .filter_map(|id| self.get(id))
            .collect()
    }

    /// Find a symbol by fully qualified name.
    pub fn find_by_qualified_name(&self, qualified: &str) -> Option<Arc<Symbol>> {
        self.qualified_index
            .get(qualified)
            .and_then(|id| self.get(id))
    }

    /// Get all symbols in a file.
    pub fn symbols_in_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        self.file_index
            .get(file_id)
            .into_iter()
            .filter_map(|id| self.get(id))
            .collect()
    }

    /// Remove all symbols for a file.
    ///
    /// Returns the removed symbols.
    pub fn remove_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        let symbol_ids = self.file_index.remove_file(file_id);
        symbol_ids
            .into_iter()
            .filter_map(|id| {
                if let Some((_, symbol)) = self.symbols.remove(&id) {
                    self.name_index.remove(&symbol.name, id);
                    self.qualified_index
                        .remove(&symbol.qualified_name.join("::"));
                    Some(symbol)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all symbols.
    pub fn all_symbols(&self) -> Vec<Arc<Symbol>> {
        self.symbols.iter().map(|r| Arc::clone(&*r)).collect()
    }

    /// Get the number of symbols.
    pub fn count(&self) -> usize {
        self.symbols.len()
    }

    /// Get statistics about the table.
    pub fn stats(&self) -> SymbolTableStats {
        SymbolTableStats {
            total_symbols: self.symbols.len(),
            unique_names: self.name_index.len(),
            files_with_symbols: self.file_index.len(),
        }
    }
}

/// Statistics about the symbol table.
#[derive(Debug, Clone)]
pub struct SymbolTableStats {
    /// Total number of symbols.
    pub total_symbols: usize,
    /// Number of unique symbol names.
    pub unique_names: usize,
    /// Number of files with symbols.
    pub files_with_symbols: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::{IdGenerator, Language, QualifiedName, Range, Span, SymbolKind, Visibility};

    fn ids() -> IdGenerator {
        IdGenerator::new()
    }

    fn make_symbol(name: &str, file_id: FileId, ids: &IdGenerator) -> Symbol {
        Symbol {
            id: ids.next(),
            name: name.to_string(),
            qualified_name: QualifiedName::simple(name),
            kind: SymbolKind::Function,
            language: Language::Rust,
            file_id,
            range: Range::new(1, 0, 10, 1),
            span: Span::new(0, 100),
            signature: None,
            visibility: Visibility::Public,
            doc: None,
        }
    }

    fn make_qualified_symbol(segments: &[&str], file_id: FileId, ids: &IdGenerator) -> Symbol {
        let name = segments.last().unwrap().to_string();
        Symbol {
            id: ids.next(),
            name: name.clone(),
            qualified_name: QualifiedName::new(segments.iter().map(|s| s.to_string()).collect()),
            kind: SymbolKind::Function,
            language: Language::Rust,
            file_id,
            range: Range::new(1, 0, 10, 1),
            span: Span::new(0, 100),
            signature: None,
            visibility: Visibility::Public,
            doc: None,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let ids = ids();
        let table = SymbolTable::new();
        let file_id: FileId = ids.next();
        let symbol = make_symbol("foo", file_id, &ids);
        let id = symbol.id;

        table.insert(symbol);

        let retrieved = table.get(id).unwrap();
        assert_eq!(retrieved.name, "foo");
        assert_eq!(retrieved.file_id, file_id);
    }

    #[test]
    fn test_find_by_name() {
        let ids = ids();
        let table = SymbolTable::new();
        let file1: FileId = ids.next();
        let file2: FileId = ids.next();

        table.insert(make_symbol("foo", file1, &ids));
        table.insert(make_symbol("foo", file2, &ids));
        table.insert(make_symbol("bar", file1, &ids));

        let foos = table.find_by_name("foo");
        assert_eq!(foos.len(), 2);

        let bars = table.find_by_name("bar");
        assert_eq!(bars.len(), 1);

        let bazs = table.find_by_name("baz");
        assert!(bazs.is_empty());
    }

    #[test]
    fn test_find_by_qualified_name() {
        let ids = ids();
        let table = SymbolTable::new();
        let file_id: FileId = ids.next();

        table.insert(make_qualified_symbol(
            &["crate", "module", "foo"],
            file_id,
            &ids,
        ));
        table.insert(make_qualified_symbol(
            &["crate", "other", "foo"],
            file_id,
            &ids,
        ));

        let sym1 = table.find_by_qualified_name("crate::module::foo");
        assert!(sym1.is_some());
        assert_eq!(sym1.unwrap().name, "foo");

        let sym2 = table.find_by_qualified_name("crate::other::foo");
        assert!(sym2.is_some());

        let sym3 = table.find_by_qualified_name("crate::module::bar");
        assert!(sym3.is_none());
    }

    #[test]
    fn test_symbols_in_file() {
        let ids = ids();
        let table = SymbolTable::new();
        let file1: FileId = ids.next();
        let file2: FileId = ids.next();

        table.insert(make_symbol("foo", file1, &ids));
        table.insert(make_symbol("bar", file1, &ids));
        table.insert(make_symbol("baz", file2, &ids));

        let file1_syms = table.symbols_in_file(file1);
        assert_eq!(file1_syms.len(), 2);

        let file2_syms = table.symbols_in_file(file2);
        assert_eq!(file2_syms.len(), 1);
    }

    #[test]
    fn test_remove() {
        let ids = ids();
        let table = SymbolTable::new();
        let file_id: FileId = ids.next();
        let symbol = make_symbol("foo", file_id, &ids);
        let id = symbol.id;

        table.insert(symbol);
        assert!(table.get(id).is_some());

        let removed = table.remove(id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "foo");

        assert!(table.get(id).is_none());
        assert!(table.find_by_name("foo").is_empty());
    }

    #[test]
    fn test_remove_file() {
        let ids = ids();
        let table = SymbolTable::new();
        let file1: FileId = ids.next();
        let file2: FileId = ids.next();

        table.insert(make_symbol("foo", file1, &ids));
        table.insert(make_symbol("bar", file1, &ids));
        table.insert(make_symbol("baz", file2, &ids));

        let removed = table.remove_file(file1);
        assert_eq!(removed.len(), 2);

        assert!(table.symbols_in_file(file1).is_empty());
        assert_eq!(table.symbols_in_file(file2).len(), 1);
        assert_eq!(table.count(), 1);
    }

    #[test]
    fn test_stats() {
        let ids = ids();
        let table = SymbolTable::new();
        let file1: FileId = ids.next();
        let file2: FileId = ids.next();

        table.insert(make_symbol("foo", file1, &ids));
        table.insert(make_symbol("foo", file2, &ids)); // same name, different file
        table.insert(make_symbol("bar", file1, &ids));

        let stats = table.stats();
        assert_eq!(stats.total_symbols, 3);
        assert_eq!(stats.unique_names, 2); // foo, bar
        assert_eq!(stats.files_with_symbols, 2); // file1, file2
    }
}
