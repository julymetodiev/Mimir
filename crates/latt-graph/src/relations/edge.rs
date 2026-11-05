//! Edge types for the relation graph.

use latt_types::{Confidence, Span};
use serde::{Deserialize, Serialize};

/// The kind of edge in the relation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Function/method call.
    Calls,
    /// Names a function without calling it, as in `.map(name)`. It may run later, or never.
    UsesAsValue,
    /// Reads a constant or static that holds no function. `trace` of the constant shows it, and
    /// `trace` of the reader does not: `MAX_SIZE` under every function would hide the rest.
    UsesConstant,
    /// Names a type, trait, alias or variant. Like a read of a constant, only `trace` of what is
    /// used shows it.
    UsesType,
    /// Reads, writes or destructures a field. An index does not tell these apart. Only `trace` of
    /// the field shows it.
    UsesField,
    /// Import relationship.
    Imports,
    /// Implements trait/interface.
    Implements,
}

impl EdgeKind {
    /// The kind's name in a saved graph.
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "Calls",
            EdgeKind::UsesAsValue => "UsesAsValue",
            EdgeKind::UsesConstant => "UsesConstant",
            EdgeKind::UsesType => "UsesType",
            EdgeKind::UsesField => "UsesField",
            EdgeKind::Imports => "Imports",
            EdgeKind::Implements => "Implements",
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_data_creation() {
        let edge = EdgeData::new(EdgeKind::Calls, Span::new(10, 20), Confidence::High);
        assert_eq!(edge.kind, EdgeKind::Calls);
        assert_eq!(edge.span.start, 10);
        assert_eq!(edge.confidence, Confidence::High);
    }
}
