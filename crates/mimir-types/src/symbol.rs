//! Symbol types for code representation.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{FileId, Language, Range, Span, SymbolId};

/// A symbol in the code graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Unique identifier.
    pub id: SymbolId,

    /// Simple name (e.g., "process_data").
    pub name: String,

    /// Fully qualified name (e.g., "crate::utils::process_data").
    pub qualified_name: QualifiedName,

    /// Kind of symbol.
    pub kind: SymbolKind,

    /// Programming language.
    pub language: Language,

    /// File containing this symbol.
    pub file_id: FileId,

    /// Line/column range in the file.
    pub range: Range,

    /// Byte span in the file.
    pub span: Span,

    /// Function/method signature if applicable.
    pub signature: Option<Signature>,

    /// Visibility level.
    pub visibility: Visibility,

    /// Documentation comment.
    pub doc: Option<String>,
}

/// A fully qualified name with path segments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualifiedName {
    segments: Vec<String>,
}

impl QualifiedName {
    /// Create a new qualified name from segments.
    pub fn new(segments: Vec<String>) -> Self {
        debug_assert!(!segments.is_empty(), "QualifiedName cannot be empty");
        Self { segments }
    }

    /// Create from a single name (no path).
    pub fn simple(name: impl Into<String>) -> Self {
        Self {
            segments: vec![name.into()],
        }
    }

    /// Parse a qualified name from a string with separator.
    pub fn from_str(s: &str, separator: &str) -> Self {
        Self::new(s.split(separator).map(String::from).collect())
    }

    /// Get the simple name (last segment).
    pub fn name(&self) -> &str {
        self.segments.last().expect("QualifiedName is never empty")
    }

    /// Get the parent path (all segments except the last).
    pub fn parent(&self) -> Option<Self> {
        if self.segments.len() > 1 {
            Some(Self::new(self.segments[..self.segments.len() - 1].to_vec()))
        } else {
            None
        }
    }

    /// Get all segments.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Join segments with a separator.
    pub fn join(&self, separator: &str) -> String {
        self.segments.join(separator)
    }

    /// Add a segment to the path.
    pub fn push(&mut self, segment: impl Into<String>) {
        self.segments.push(segment.into());
    }

    /// Check if this name starts with another path.
    pub fn starts_with(&self, other: &Self) -> bool {
        self.segments.starts_with(&other.segments)
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.join("::"))
    }
}

/// The kind of symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    // Containers
    Module,
    Class,
    Interface,
    Trait,
    Namespace,
    Package,

    // Callables
    Function,
    Method,
    Constructor,
    Closure,
    Macro,

    // Data structures
    Struct,
    Enum,
    EnumVariant,
    Union,
    TypeAlias,

    // Members
    Field,
    Property,
    Constant,
    Variable,
    Parameter,

    // Types
    TypeParameter,

    // Special
    Impl,
    Endpoint,
    Test,
}

impl SymbolKind {
    /// Check if this is a container kind (can contain other symbols).
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            Self::Module
                | Self::Class
                | Self::Interface
                | Self::Trait
                | Self::Namespace
                | Self::Package
        )
    }

    /// Check if this is a callable kind.
    pub fn is_callable(&self) -> bool {
        matches!(
            self,
            Self::Function | Self::Method | Self::Constructor | Self::Closure | Self::Macro
        )
    }

    /// Check if this is a type kind.
    pub fn is_type(&self) -> bool {
        matches!(
            self,
            Self::Struct
                | Self::Enum
                | Self::Union
                | Self::TypeAlias
                | Self::Interface
                | Self::Trait
                | Self::Class
        )
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Module => "module",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Namespace => "namespace",
            Self::Package => "package",
            Self::Function => "function",
            Self::Method => "method",
            Self::Constructor => "constructor",
            Self::Closure => "closure",
            Self::Macro => "macro",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::EnumVariant => "variant",
            Self::Union => "union",
            Self::TypeAlias => "type",
            Self::Field => "field",
            Self::Property => "property",
            Self::Constant => "const",
            Self::Variable => "var",
            Self::Parameter => "param",
            Self::TypeParameter => "type_param",
            Self::Impl => "impl",
            Self::Endpoint => "endpoint",
            Self::Test => "test",
        };
        write!(f, "{}", s)
    }
}

/// Function/method signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// Parameters.
    pub parameters: Vec<Parameter>,

    /// Return type if any.
    pub return_type: Option<String>,

    /// Generic type parameters.
    pub type_parameters: Vec<String>,

    /// Raw signature text.
    pub raw: String,
}

/// A function parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name.
    pub name: String,

    /// Type annotation if present.
    pub type_annotation: Option<String>,

    /// Default value if present.
    pub default_value: Option<String>,
}

/// Visibility level of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Public (visible everywhere).
    Public,

    /// Private (visible only in same scope).
    Private,

    /// Protected (visible in subclasses).
    Protected,

    /// Internal (pub(crate) in Rust).
    Internal,

    /// Inherited from parent scope.
    #[default]
    Inherited,
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Public => "pub",
            Self::Private => "priv",
            Self::Protected => "prot",
            Self::Internal => "pub(crate)",
            Self::Inherited => "",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qualified_name() {
        let qn = QualifiedName::from_str("crate::module::func", "::");
        assert_eq!(qn.name(), "func");
        assert_eq!(qn.join("::"), "crate::module::func");

        let parent = qn.parent().unwrap();
        assert_eq!(parent.join("::"), "crate::module");
    }

    #[test]
    fn test_qualified_name_simple() {
        let qn = QualifiedName::simple("main");
        assert_eq!(qn.name(), "main");
        assert!(qn.parent().is_none());
    }

    #[test]
    fn test_symbol_kind_predicates() {
        assert!(SymbolKind::Module.is_container());
        assert!(SymbolKind::Function.is_callable());
        assert!(SymbolKind::Struct.is_type());

        assert!(!SymbolKind::Variable.is_container());
        assert!(!SymbolKind::Struct.is_callable());
        assert!(!SymbolKind::Function.is_type());
    }

    #[test]
    fn test_visibility_display() {
        assert_eq!(format!("{}", Visibility::Public), "pub");
        assert_eq!(format!("{}", Visibility::Internal), "pub(crate)");
    }
}
