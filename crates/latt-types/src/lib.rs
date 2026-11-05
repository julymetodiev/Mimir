//! Core types for the Lantern Lattice semantic code intelligence engine.
//!
//! This crate provides foundational types used throughout the Lantern Lattice system:
//! - ID types (SymbolId, FileId, etc.)
//! - ContentHash for content-addressed storage
//! - Location and Span for source code positions
//! - Language enum for supported programming languages
//! - Symbol and Reference types for code graph

mod content_hash;
mod id;
mod language;
mod location;
mod reference;
mod symbol;

pub use content_hash::ContentHash;
pub use id::{CheckpointId, FileId, IdGenerator, SnapshotId, StepId, SymbolId, TaskId};
pub use language::Language;
pub use location::{Location, Range, Span};
pub use reference::{CallForm, Confidence, ExtractedReference, Reference, ReferenceKind};
pub use symbol::{Parameter, QualifiedName, Signature, Symbol, SymbolKind, Visibility};
