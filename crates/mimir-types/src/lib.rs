//! Core types for the Mimir semantic code intelligence engine.
//!
//! This crate provides foundational types used throughout the Mimir system:
//! - ID types (SymbolId, FileId, etc.)
//! - ContentHash for content-addressed storage
//! - Location and Span for source code positions
//! - Language enum for supported programming languages

mod content_hash;
mod id;
mod language;
mod location;

pub use content_hash::ContentHash;
pub use id::{CheckpointId, FileId, SnapshotId, StepId, SymbolId, TaskId};
pub use language::Language;
pub use location::{Location, Range, Span};
