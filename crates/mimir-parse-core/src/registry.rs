//! Registry of language adapters.

use dashmap::DashMap;
use mimir_types::Language;
use std::sync::Arc;

use crate::adapter::LanguageAdapter;

/// Registry for language adapters.
///
/// Provides lookup by language or file extension.
#[derive(Default)]
pub struct AdapterRegistry {
    /// Adapters by language.
    by_language: DashMap<Language, Arc<dyn LanguageAdapter>>,
    /// Adapters by file extension.
    by_extension: DashMap<String, Arc<dyn LanguageAdapter>>,
}

impl std::fmt::Debug for AdapterRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdapterRegistry")
            .field("languages", &self.languages())
            .field("extensions", &self.extensions())
            .finish()
    }
}

impl AdapterRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            by_language: DashMap::new(),
            by_extension: DashMap::new(),
        }
    }

    /// Register a language adapter.
    pub fn register(&self, adapter: Arc<dyn LanguageAdapter>) {
        let language = adapter.language();

        // Register by language
        self.by_language.insert(language, adapter.clone());

        // Register by extension
        for ext in adapter.extensions() {
            self.by_extension.insert(ext.to_string(), adapter.clone());
        }
    }

    /// Get an adapter by language.
    pub fn get_by_language(&self, language: Language) -> Option<Arc<dyn LanguageAdapter>> {
        self.by_language.get(&language).map(|r| r.clone())
    }

    /// Get all registered languages.
    pub fn languages(&self) -> Vec<Language> {
        self.by_language.iter().map(|r| *r.key()).collect()
    }

    /// Get all registered extensions.
    pub fn extensions(&self) -> Vec<String> {
        self.by_extension.iter().map(|r| r.key().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extracted::{ExtractedReference, ExtractedSymbol};
    use crate::tree::ParsedTree;
    use mimir_types::FileId;

    // Mock adapter for testing
    struct MockAdapter;

    impl LanguageAdapter for MockAdapter {
        fn language(&self) -> Language {
            Language::Rust
        }

        fn parse(&self, _source: &str) -> Option<ParsedTree> {
            None
        }

        fn extract_symbols(&self, _tree: &ParsedTree, _file_id: FileId) -> Vec<ExtractedSymbol> {
            vec![]
        }

        fn extract_references(
            &self,
            _tree: &ParsedTree,
            _file_id: FileId,
        ) -> Vec<ExtractedReference> {
            vec![]
        }

        fn extensions(&self) -> &[&str] {
            &["rs"]
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(MockAdapter));

        assert!(registry.get_by_language(Language::Rust).is_some());
    }

    #[test]
    fn test_missing_adapter() {
        let registry = AdapterRegistry::new();

        assert!(registry.get_by_language(Language::TypeScript).is_none());
    }
}
