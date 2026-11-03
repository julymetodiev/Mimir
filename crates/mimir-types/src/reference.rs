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

    // Data references
    /// Variable read.
    Read,

    /// A constant named inside a format string, as in `format!("{NAME}")`, which no index reports.
    FormatRead,

    /// A name bound in the function, read where a function could also be named.
    LocalRead,

    /// A type, trait or variant named in code. What it is, a semantic source can say.
    TypeUse,

    /// A field read, written or named in a pattern.
    FieldUse,
}

impl ReferenceKind {}

impl fmt::Display for ReferenceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Call => "call",
            Self::FunctionValue => "fn_value",
            Self::Import => "import",
            Self::Read => "read",
            Self::FormatRead => "format_read",
            Self::LocalRead => "local_read",
            Self::TypeUse => "type_use",
            Self::FieldUse => "field_use",
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

impl Confidence {}

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
