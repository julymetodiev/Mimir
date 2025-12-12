//! Language adapter trait for parsing.

use mimir_types::{FileId, Language};

use crate::extracted::{ExtractedReference, ExtractedSymbol};
use crate::tree::ParsedTree;

/// Trait for language-specific parsing adapters.
///
/// Each supported language implements this trait to provide:
/// - Tree-sitter parsing
/// - Symbol extraction (functions, classes, etc.)
/// - Reference extraction (calls, imports, etc.)
pub trait LanguageAdapter: Send + Sync {
    /// Get the language this adapter handles.
    fn language(&self) -> Language;

    /// Parse source code into a parse tree.
    fn parse(&self, source: &str) -> Option<ParsedTree>;

    /// Extract symbols from a parse tree.
    fn extract_symbols(&self, tree: &ParsedTree, file_id: FileId) -> Vec<ExtractedSymbol>;

    /// Extract references from a parse tree.
    fn extract_references(&self, tree: &ParsedTree, file_id: FileId) -> Vec<ExtractedReference>;

    /// Get file extensions this adapter handles.
    fn extensions(&self) -> &[&str];
}

#[cfg(test)]
mod tests {
    use super::*;

    // We can't easily test the trait without an implementation,
    // but we can verify the trait is object-safe
    #[test]
    fn test_trait_object_safety() {
        fn _takes_adapter(_: &dyn LanguageAdapter) {}
    }
}
