//! Edge types for the relation graph.

use mimir_types::{Confidence, ReferenceKind, Span};
use serde::{Deserialize, Serialize};

/// The kind of edge in the relation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Function/method call.
    Calls,
    /// Names a function without calling it, as in `.map(name)`. It may run later, or never.
    UsesAsValue,
    /// Import relationship.
    Imports,
    /// Type usage.
    UsesType,
    /// Instantiation.
    Instantiates,
    /// Field access.
    AccessesField,
    /// Inheritance (extends).
    Extends,
    /// Implements trait/interface.
    Implements,
    /// Contains (parent-child relationship).
    Contains,
    /// Reads variable.
    Reads,
    /// Writes variable.
    Writes,
    /// Borrows reference.
    Borrows,
    /// Mutably borrows reference.
    MutablyBorrows,
}

impl From<ReferenceKind> for EdgeKind {
    fn from(kind: ReferenceKind) -> Self {
        match kind {
            ReferenceKind::Call => EdgeKind::Calls,
            ReferenceKind::FunctionValue => EdgeKind::UsesAsValue,
            ReferenceKind::Import => EdgeKind::Imports,
            ReferenceKind::TypeUse => EdgeKind::UsesType,
            ReferenceKind::Instantiation => EdgeKind::Instantiates,
            ReferenceKind::FieldAccess => EdgeKind::AccessesField,
            ReferenceKind::Inheritance => EdgeKind::Extends,
            ReferenceKind::Implementation => EdgeKind::Implements,
            ReferenceKind::Read | ReferenceKind::LocalRead => EdgeKind::Reads,
            ReferenceKind::Write => EdgeKind::Writes,
            ReferenceKind::Borrow => EdgeKind::Borrows,
            ReferenceKind::MutableBorrow => EdgeKind::MutablyBorrows,
        }
    }
}

/// Data associated with an edge in the relation graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeData {
    /// Kind of relationship.
    pub kind: EdgeKind,

    /// Byte span of the reference in source code.
    pub span: Span,

    /// Confidence level of this relationship.
    pub confidence: Confidence,
}

impl EdgeData {
    /// Create new edge data.
    pub fn new(kind: EdgeKind, span: Span, confidence: Confidence) -> Self {
        Self {
            kind,
            span,
            confidence,
        }
    }

    /// Create edge data from a reference kind.
    pub fn from_reference(kind: ReferenceKind, span: Span, confidence: Confidence) -> Self {
        Self {
            kind: EdgeKind::from(kind),
            span,
            confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_kind_from_reference_kind() {
        assert_eq!(EdgeKind::from(ReferenceKind::Call), EdgeKind::Calls);
        assert_eq!(EdgeKind::from(ReferenceKind::Import), EdgeKind::Imports);
        assert_eq!(
            EdgeKind::from(ReferenceKind::Implementation),
            EdgeKind::Implements
        );
    }

    #[test]
    fn test_edge_data_creation() {
        let edge = EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High);
        assert_eq!(edge.kind, EdgeKind::Calls);
        assert_eq!(edge.span.start, 10);
        assert_eq!(edge.confidence, Confidence::High);
    }
}
