//! File editing operations with conflict detection.
//!
//! This crate provides tools for safely editing files with optimistic locking
//! to detect and prevent concurrent modification conflicts.
//!
//! # Example
//!
//! ```no_run
//! use mimir_edit::{Editor, EditOp};
//! use mimir_types::Span;
//!
//! # async fn example() -> Result<(), mimir_edit::EditError> {
//! let editor = Editor::new();
//!
//! // Replace text at a specific byte range
//! let op = EditOp::replace("src/main.rs", Span::new(10, 20), "new_function");
//! let result = editor.apply(&op).await?;
//!
//! println!("Changed {} bytes", result.bytes_changed);
//! # Ok(())
//! # }
//! ```
//!
//! # Optimistic Locking
//!
//! Use content hashes to detect concurrent modifications:
//!
//! ```no_run
//! use mimir_edit::{Editor, EditOp};
//! use mimir_types::{ContentHash, Span};
//!
//! # async fn example() -> Result<(), mimir_edit::EditError> {
//! let content = std::fs::read("src/main.rs")?;
//! let hash = ContentHash::compute(&content);
//!
//! let op = EditOp::replace("src/main.rs", Span::new(10, 20), "new_function")
//!     .with_expected_hash(hash);
//!
//! let editor = Editor::new();
//! // This will fail if the file was modified since we computed the hash
//! let result = editor.apply(&op).await?;
//! # Ok(())
//! # }
//! ```

mod editor;
mod error;
mod op;

pub use error::EditError;

/// Result type for edit operations.
pub type EditResult<T> = Result<T, EditError>;

pub use editor::Editor;
pub use op::{Diff, EditOp, EditResult as OpResult};
