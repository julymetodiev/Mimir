//! Handle pattern for thread-safe symbol table access.

use mimir_types::{FileId, Symbol, SymbolId};
use std::sync::Arc;

use crate::symbols::table::{SymbolTable, SymbolTableStats};

/// A cheap, cloneable handle to the symbol table.
///
/// This handle wraps an `Arc<SymbolTable>` providing convenient access
/// to the underlying table. It can be freely cloned and shared across threads.
#[derive(Debug, Clone)]
pub struct SymbolsHandle {
    inner: Arc<SymbolTable>,
}

impl Default for SymbolsHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolsHandle {
    /// Create a new symbols handle with an empty table.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SymbolTable::new()),
        }
    }

    /// Create a handle from an existing table.
    pub fn from_table(table: SymbolTable) -> Self {
        Self {
            inner: Arc::new(table),
        }
    }

    /// Insert a symbol.
    pub fn insert(&self, symbol: Symbol) -> Option<Arc<Symbol>> {
        self.inner.insert(symbol)
    }

    /// Get a symbol by ID.
    pub fn get(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        self.inner.get(id)
    }

    /// Remove a symbol by ID.
    pub fn remove(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        self.inner.remove(id)
    }

    /// Find symbols by simple name.
    pub fn find_by_name(&self, name: &str) -> Vec<Arc<Symbol>> {
        self.inner.find_by_name(name)
    }

    /// Find a symbol by fully qualified name.
    pub fn find_by_qualified_name(&self, qualified: &str) -> Option<Arc<Symbol>> {
        self.inner.find_by_qualified_name(qualified)
    }

    /// Get all symbols in a file.
    pub fn symbols_in_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        self.inner.symbols_in_file(file_id)
    }

    /// Remove all symbols for a file.
    pub fn remove_file(&self, file_id: FileId) -> Vec<Arc<Symbol>> {
        self.inner.remove_file(file_id)
    }

    /// Get all symbol IDs.
    pub fn symbol_ids(&self) -> Vec<SymbolId> {
        self.inner.symbol_ids()
    }

    /// Get all symbols.
    pub fn all_symbols(&self) -> Vec<Arc<Symbol>> {
        self.inner.all_symbols()
    }

    /// Get the number of symbols.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Clear all symbols.
    pub fn clear(&self) {
        self.inner.clear()
    }

    /// Get statistics.
    pub fn stats(&self) -> SymbolTableStats {
        self.inner.stats()
    }

    /// Get the number of strong references to the underlying table.
    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }
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

    #[test]
    fn test_handle_clone_shares_data() {
        let ids = ids();
        let handle1 = SymbolsHandle::new();
        let handle2 = handle1.clone();
        let file_id: FileId = ids.next();

        // Insert through handle1
        let symbol = make_symbol("foo", file_id, &ids);
        let id = symbol.id;
        handle1.insert(symbol);

        // Should be visible through handle2
        assert!(handle2.get(id).is_some());
        assert_eq!(handle2.len(), 1);
    }

    #[test]
    fn test_handle_strong_count() {
        let handle1 = SymbolsHandle::new();
        assert_eq!(handle1.strong_count(), 1);

        let handle2 = handle1.clone();
        assert_eq!(handle1.strong_count(), 2);
        assert_eq!(handle2.strong_count(), 2);

        drop(handle2);
        assert_eq!(handle1.strong_count(), 1);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::thread;

        let handle = SymbolsHandle::new();
        // Use a shared atomic counter for unique IDs across threads
        let counter = std::sync::Arc::new(AtomicU64::new(1));
        let file_id = FileId::new(1);

        let handles: Vec<_> = (0..4).map(|_| (handle.clone(), counter.clone())).collect();

        let threads: Vec<_> = handles
            .into_iter()
            .enumerate()
            .map(|(i, (h, cnt))| {
                thread::spawn(move || {
                    for j in 0..100 {
                        let name = format!("sym_{}_{}", i, j);
                        let sym_id = SymbolId::new(cnt.fetch_add(1, Ordering::SeqCst));
                        let symbol = Symbol {
                            id: sym_id,
                            name: name.clone(),
                            qualified_name: QualifiedName::simple(&name),
                            kind: SymbolKind::Function,
                            language: Language::Rust,
                            file_id,
                            range: Range::new(1, 0, 10, 1),
                            span: Span::new(0, 100),
                            signature: None,
                            visibility: Visibility::Public,
                            doc: None,
                        };
                        h.insert(symbol);
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(handle.len(), 400);
    }
}
