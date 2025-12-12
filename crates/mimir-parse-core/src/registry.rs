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

    /// Get an adapter by file extension.
    pub fn get_by_extension(&self, extension: &str) -> Option<Arc<dyn LanguageAdapter>> {
        // Remove leading dot if present
        let ext = extension.strip_prefix('.').unwrap_or(extension);
        self.by_extension.get(ext).map(|r| r.clone())
    }

    /// Get all registered languages.
    pub fn languages(&self) -> Vec<Language> {
        self.by_language.iter().map(|r| *r.key()).collect()
    }

    /// Get all registered extensions.
    pub fn extensions(&self) -> Vec<String> {
        self.by_extension.iter().map(|r| r.key().clone()).collect()
    }

    /// Check if a language is registered.
    pub fn has_language(&self, language: Language) -> bool {
        self.by_language.contains_key(&language)
    }

    /// Check if an extension is registered.
    pub fn has_extension(&self, extension: &str) -> bool {
        let ext = extension.strip_prefix('.').unwrap_or(extension);
        self.by_extension.contains_key(ext)
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

        assert!(registry.has_language(Language::Rust));
        assert!(registry.has_extension("rs"));
        assert!(registry.has_extension(".rs"));

        assert!(registry.get_by_language(Language::Rust).is_some());
        assert!(registry.get_by_extension("rs").is_some());
    }

    #[test]
    fn test_missing_adapter() {
        let registry = AdapterRegistry::new();

        assert!(!registry.has_language(Language::TypeScript));
        assert!(!registry.has_extension("ts"));

        assert!(registry.get_by_language(Language::TypeScript).is_none());
        assert!(registry.get_by_extension("ts").is_none());
    }
}
