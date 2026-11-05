//! Types for extracted symbols and references from parsing.

pub use mimir_types::{CallForm, ExtractedReference};
use mimir_types::{Language, QualifiedName, Range, Signature, Span, SymbolKind, Visibility};
use serde::{Deserialize, Serialize};

/// A symbol extracted from source code.
///
/// This is an intermediate representation before being stored in the symbol table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedSymbol {
    /// Simple name.
    pub name: String,

    /// Fully qualified name.
    pub qualified_name: QualifiedName,

    /// Kind of symbol.
    pub kind: SymbolKind,

    /// Programming language.
    pub language: Language,

    /// Line/column range.
    pub range: Range,

    /// Byte span.
    pub span: Span,

    /// Function signature if applicable.
    pub signature: Option<Signature>,

    /// Visibility.
    pub visibility: Visibility,

    /// Documentation comment.
    pub doc: Option<String>,

    /// Parent symbol name (for nested symbols).
    pub parent: Option<String>,
}

impl ExtractedSymbol {
    /// Create a new extracted symbol with minimal required fields.
    pub fn new(
        name: String,
        kind: SymbolKind,
        language: Language,
        range: Range,
        span: Span,
    ) -> Self {
        Self {
            qualified_name: QualifiedName::simple(&name),
            name,
            kind,
            language,
            range,
            span,
            signature: None,
            visibility: Visibility::default(),
            doc: None,
            parent: None,
        }
    }

    /// Set the qualified name.
    pub fn with_qualified_name(mut self, qualified_name: QualifiedName) -> Self {
        self.qualified_name = qualified_name;
        self
    }

    /// Set the visibility.
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Set the parent symbol name.
    pub fn with_parent(mut self, parent: String) -> Self {
        self.parent = Some(parent);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::{Confidence, ReferenceKind};

    #[test]
    fn test_extracted_symbol() {
        let sym = ExtractedSymbol::new(
            "foo".to_string(),
            SymbolKind::Function,
            Language::Rust,
            Range::new(1, 0, 5, 1),
            Span::new(0, 50),
        )
        .with_visibility(Visibility::Public);

        assert_eq!(sym.name, "foo");
        assert_eq!(sym.kind, SymbolKind::Function);
        assert_eq!(sym.visibility, Visibility::Public);
    }

    #[test]
    fn test_extracted_reference() {
        let reference = ExtractedReference::new(
            "bar".to_string(),
            ReferenceKind::Call,
            Range::new(10, 4, 10, 12),
            Span::new(100, 108),
        )
        .with_confidence(Confidence::High)
        .with_container("foo".to_string());

        assert_eq!(reference.name, "bar");
        assert_eq!(reference.kind, ReferenceKind::Call);
        assert_eq!(reference.confidence, Confidence::High);
        assert_eq!(reference.container, Some("foo".to_string()));
    }
}
