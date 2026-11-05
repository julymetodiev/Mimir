//! Reference types for code relationships.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{Location, Range, Span, SymbolId};

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

    /// A field written, where a parser can tell: a setter runs, not a getter.
    FieldWrite,

    /// A method named where it is declared, for a language where a server says what implements what.
    MethodDeclaration,
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
            Self::FieldWrite => "field_write",
            Self::MethodDeclaration => "method_declaration",
        };
        write!(f, "{}", s)
    }
}

/// A reference found in source code, before it is resolved to a symbol.
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
