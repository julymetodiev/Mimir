//! Reference types for code relationships.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{Location, Span, SymbolId};

/// A reference from one symbol to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    /// Symbol making the reference.
    pub from_symbol: SymbolId,

    /// Symbol being referenced.
    pub to_symbol: SymbolId,

    /// Kind of reference.
    pub kind: ReferenceKind,

    /// Location of the reference.
    pub location: Location,

    /// Byte span of the reference.
    pub span: Span,

    /// Confidence level of this reference.
    pub confidence: Confidence,
}

/// The kind of reference between symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    // Code references
    /// Function or method call.
    Call,

    /// A function named without being called, as in `.map(name)`.
    FunctionValue,

    /// Import statement.
    Import,

    /// Type annotation usage.
    TypeUse,

    /// Instantiation (new Foo() or Foo::new()).
    Instantiation,

    /// Field access (foo.bar).
    FieldAccess,

    /// Inheritance (extends, implements).
    Inheritance,

    /// Trait/interface implementation.
    Implementation,

    // Data references
    /// Variable read.
    Read,

    /// Variable write/assignment.
    Write,

    /// Borrow (&foo).
    Borrow,

    /// Mutable borrow (&mut foo).
    MutableBorrow,
}

impl ReferenceKind {
    /// Check if this is a call-like reference.
    pub fn is_call(&self) -> bool {
        matches!(self, Self::Call | Self::Instantiation)
    }

    /// Check if this is a data access reference.
    pub fn is_data_access(&self) -> bool {
        matches!(
            self,
            Self::Read | Self::Write | Self::FieldAccess | Self::Borrow | Self::MutableBorrow
        )
    }

    /// Check if this is a type relationship.
    pub fn is_type_relation(&self) -> bool {
        matches!(
            self,
            Self::TypeUse | Self::Inheritance | Self::Implementation
        )
    }
}

impl fmt::Display for ReferenceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Call => "call",
            Self::FunctionValue => "fn_value",
            Self::Import => "import",
            Self::TypeUse => "type_use",
            Self::Instantiation => "new",
            Self::FieldAccess => "field",
            Self::Inheritance => "extends",
            Self::Implementation => "impl",
            Self::Read => "read",
            Self::Write => "write",
            Self::Borrow => "borrow",
            Self::MutableBorrow => "mut_borrow",
        };
        write!(f, "{}", s)
    }
}

/// Confidence level of a reference resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// 100% certain (same file, unique name, or explicit path).
    Certain,

    /// Very likely correct (import resolved, type checked).
    #[default]
    High,

    /// Probably correct (name match with context).
    Medium,

    /// Best guess (name match only).
    Low,
}

impl Confidence {
    /// Convert to a numeric value (0.0 to 1.0).
    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Certain => 1.0,
            Self::High => 0.9,
            Self::Medium => 0.7,
            Self::Low => 0.5,
        }
    }

    /// Check if this confidence level is acceptable.
    pub fn is_acceptable(&self) -> bool {
        matches!(self, Self::Certain | Self::High | Self::Medium)
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Certain => "certain",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_kind_predicates() {
        assert!(ReferenceKind::Call.is_call());
        assert!(ReferenceKind::Instantiation.is_call());
        assert!(!ReferenceKind::Import.is_call());

        assert!(ReferenceKind::Read.is_data_access());
        assert!(ReferenceKind::FieldAccess.is_data_access());
        assert!(!ReferenceKind::Call.is_data_access());

        assert!(ReferenceKind::TypeUse.is_type_relation());
        assert!(ReferenceKind::Inheritance.is_type_relation());
        assert!(!ReferenceKind::Call.is_type_relation());
    }

    #[test]
    fn test_confidence_values() {
        assert_eq!(Confidence::Certain.as_f32(), 1.0);
        assert_eq!(Confidence::High.as_f32(), 0.9);
        assert_eq!(Confidence::Medium.as_f32(), 0.7);
        assert_eq!(Confidence::Low.as_f32(), 0.5);
    }

    #[test]
    fn test_confidence_acceptable() {
        assert!(Confidence::Certain.is_acceptable());
        assert!(Confidence::High.is_acceptable());
        assert!(Confidence::Medium.is_acceptable());
        assert!(!Confidence::Low.is_acceptable());
    }
}
