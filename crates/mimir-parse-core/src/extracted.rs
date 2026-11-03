//! Types for extracted symbols and references from parsing.

use mimir_types::{
    Confidence, Language, QualifiedName, Range, ReferenceKind, Signature, Span, SymbolKind,
    Visibility,
};
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

/// A reference extracted from source code.
///
/// This is an intermediate representation before being stored in the relation graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedReference {
    /// Name of the symbol being referenced.
    pub name: String,

    /// Kind of reference.
    pub kind: ReferenceKind,

    /// Line/column range.
    pub range: Range,

    /// Byte span.
    pub span: Span,

    /// Confidence level.
    pub confidence: Confidence,

    /// Context for resolution (e.g., import path, type annotation).
    pub context: Option<String>,

    /// Name of the containing symbol (the "from" side).
    pub container: Option<String>,

    /// How a call or function value is written; `context` holds its receiver or path. On an import,
    /// [`CallForm::Bound`] says that the name here is not the name in the other file. `None` for
    /// anything else.
    #[serde(default)]
    pub call_form: Option<CallForm>,

    /// Where a semantic source can be asked what a receiver's type is: its last name, as a 1-based
    /// line and a column. `None` if it has no such name, as in `make().run()`, or is `this`.
    #[serde(default)]
    pub receiver_at: Option<(u32, u32)>,
}

/// How a call is written, which decides what its name can refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallForm {
    /// `name()`
    Plain,
    /// `receiver.name()`
    Method,
    /// `path::name()`
    Scoped,
    /// `Struct { name }`, which only a value can be.
    Shorthand,
    /// `name()` where the function around it also binds `name`, somewhere. Only a semantic source
    /// can say which one it is.
    Bound,
}

impl ExtractedReference {
    /// Create a new extracted reference with minimal required fields.
    pub fn new(name: String, kind: ReferenceKind, range: Range, span: Span) -> Self {
        Self {
            name,
            kind,
            range,
            span,
            confidence: Confidence::default(),
            context: None,
            container: None,
            call_form: None,
            receiver_at: None,
        }
    }

    /// Set the form of a call.
    pub fn with_call_form(mut self, form: CallForm) -> Self {
        self.call_form = Some(form);
        self
    }

    /// Set where a receiver's type can be asked for.
    pub fn with_receiver_at(mut self, at: Option<(u32, u32)>) -> Self {
        self.receiver_at = at;
        self
    }

    /// Set the confidence level.
    pub fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }

    /// Set the context.
    pub fn with_context(mut self, context: String) -> Self {
        self.context = Some(context);
        self
    }

    /// Set the container.
    pub fn with_container(mut self, container: String) -> Self {
        self.container = Some(container);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
