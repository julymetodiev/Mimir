//! Query types and options.

use mimir_types::{Language, SymbolKind};
use serde::{Deserialize, Serialize};

/// Options for querying the code graph.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryOptions {
    /// Filter by language.
    pub language: Option<Language>,

    /// Filter by symbol kind.
    pub kind: Option<SymbolKind>,

    /// Maximum number of results.
    pub limit: Option<usize>,

    /// Include only symbols with matching visibility.
    pub public_only: bool,
}

impl QueryOptions {
    /// Create new query options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by language.
    pub fn with_language(mut self, language: Language) -> Self {
        self.language = Some(language);
        self
    }

    /// Filter by symbol kind.
    pub fn with_kind(mut self, kind: SymbolKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Limit results.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Only include public symbols.
    pub fn public_only(mut self) -> Self {
        self.public_only = true;
        self
    }
}

/// Result of a symbol query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult<T> {
    /// The results.
    pub items: Vec<T>,

    /// Total count (if pagination is used).
    pub total: usize,

    /// Whether more results are available.
    pub has_more: bool,
}

impl<T> QueryResult<T> {
    /// Create a new query result.
    pub fn new(items: Vec<T>) -> Self {
        let total = items.len();
        Self {
            items,
            total,
            has_more: false,
        }
    }

    /// Create an empty result.
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            total: 0,
            has_more: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_options() {
        let opts = QueryOptions::new()
            .with_language(Language::Rust)
            .with_kind(SymbolKind::Function)
            .with_limit(10)
            .public_only();

        assert_eq!(opts.language, Some(Language::Rust));
        assert_eq!(opts.kind, Some(SymbolKind::Function));
        assert_eq!(opts.limit, Some(10));
        assert!(opts.public_only);
    }

    #[test]
    fn test_query_result() {
        let result = QueryResult::new(vec![1, 2, 3]);
        assert_eq!(result.items.len(), 3);
        assert_eq!(result.total, 3);
        assert!(!result.has_more);
    }
}
