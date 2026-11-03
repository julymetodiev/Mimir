//! Edit operations.

use mimir_types::{ContentHash, Span};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// An edit operation on a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditOp {
    /// Replace a span of bytes with new content.
    Replace {
        /// Path to the file.
        file_path: PathBuf,
        /// Byte span to replace.
        span: Span,
        /// New content to insert.
        new_content: String,
        /// Expected content hash for optimistic locking.
        /// If None, no lock check is performed.
        expected_hash: Option<ContentHash>,
    },

    /// Insert content at a position.
    Insert {
        /// Path to the file.
        file_path: PathBuf,
        /// Byte position to insert at.
        position: u32,
        /// Content to insert.
        content: String,
        /// Expected content hash for optimistic locking.
        expected_hash: Option<ContentHash>,
    },

    /// Delete a span of bytes.
    Delete {
        /// Path to the file.
        file_path: PathBuf,
        /// Byte span to delete.
        span: Span,
        /// Expected content hash for optimistic locking.
        expected_hash: Option<ContentHash>,
    },

    /// Replace an entire line range.
    ReplaceLines {
        /// Path to the file.
        file_path: PathBuf,
        /// Starting line (1-indexed).
        start_line: u32,
        /// Ending line (1-indexed, inclusive).
        end_line: u32,
        /// New content (should include newlines).
        new_content: String,
        /// Expected content hash for optimistic locking.
        expected_hash: Option<ContentHash>,
    },

    /// Replace text found by what it says.
    ReplaceText {
        /// Path to the file.
        file_path: PathBuf,
        /// The text to find, which must occur once unless `all` is set.
        old: String,
        /// What to put in its place.
        new: String,
        /// Replace every place it occurs.
        all: bool,
        /// Expected content hash for optimistic locking.
        expected_hash: Option<ContentHash>,
    },
}

impl EditOp {
    /// Create a replace operation.
    pub fn replace(
        file_path: impl Into<PathBuf>,
        span: Span,
        new_content: impl Into<String>,
    ) -> Self {
        Self::Replace {
            file_path: file_path.into(),
            span,
            new_content: new_content.into(),
            expected_hash: None,
        }
    }

    /// Create an insert operation.
    pub fn insert(
        file_path: impl Into<PathBuf>,
        position: u32,
        content: impl Into<String>,
    ) -> Self {
        Self::Insert {
            file_path: file_path.into(),
            position,
            content: content.into(),
            expected_hash: None,
        }
    }

    /// Create a delete operation.
    pub fn delete(file_path: impl Into<PathBuf>, span: Span) -> Self {
        Self::Delete {
            file_path: file_path.into(),
            span,
            expected_hash: None,
        }
    }

    /// Create a replace lines operation.
    pub fn replace_lines(
        file_path: impl Into<PathBuf>,
        start_line: u32,
        end_line: u32,
        new_content: impl Into<String>,
    ) -> Self {
        Self::ReplaceLines {
            file_path: file_path.into(),
            start_line,
            end_line,
            new_content: new_content.into(),
            expected_hash: None,
        }
    }

    /// Create a replace text operation.
    pub fn replace_text(
        file_path: impl Into<PathBuf>,
        old: impl Into<String>,
        new: impl Into<String>,
        all: bool,
    ) -> Self {
        Self::ReplaceText {
            file_path: file_path.into(),
            old: old.into(),
            new: new.into(),
            all,
            expected_hash: None,
        }
    }

    /// Set the expected hash for optimistic locking.
    pub fn with_expected_hash(mut self, hash: ContentHash) -> Self {
        match &mut self {
            Self::Replace { expected_hash, .. } => *expected_hash = Some(hash),
            Self::Insert { expected_hash, .. } => *expected_hash = Some(hash),
            Self::Delete { expected_hash, .. } => *expected_hash = Some(hash),
            Self::ReplaceLines { expected_hash, .. } => *expected_hash = Some(hash),
            Self::ReplaceText { expected_hash, .. } => *expected_hash = Some(hash),
        }
        self
    }

    /// Get the file path for this operation.
    pub fn file_path(&self) -> &PathBuf {
        match self {
            Self::Replace { file_path, .. } => file_path,
            Self::Insert { file_path, .. } => file_path,
            Self::Delete { file_path, .. } => file_path,
            Self::ReplaceLines { file_path, .. } => file_path,
            Self::ReplaceText { file_path, .. } => file_path,
        }
    }

    /// Get the expected hash if set.
    pub fn expected_hash(&self) -> Option<&ContentHash> {
        match self {
            Self::Replace { expected_hash, .. } => expected_hash.as_ref(),
            Self::Insert { expected_hash, .. } => expected_hash.as_ref(),
            Self::Delete { expected_hash, .. } => expected_hash.as_ref(),
            Self::ReplaceLines { expected_hash, .. } => expected_hash.as_ref(),
            Self::ReplaceText { expected_hash, .. } => expected_hash.as_ref(),
        }
    }
}

/// Result of an edit operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditResult {
    /// Path to the edited file.
    pub file_path: PathBuf,
    /// Hash before the edit.
    pub old_hash: ContentHash,
    /// Hash after the edit.
    pub new_hash: ContentHash,
    /// Number of bytes changed.
    /// Number of bytes changed (delta).
    pub bytes_changed: i64,
    /// Number of lines changed (delta).
    pub lines_changed: i64,
    /// Number of lines modified/touched.
    pub lines_modified: usize,
}

/// A diff showing what would change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    /// Path to the file.
    pub file_path: PathBuf,
    /// Original content (the affected region).
    pub old_content: String,
    /// New content (what it would become).
    pub new_content: String,
    /// Start line of the change.
    pub start_line: u32,
    /// End line of the change.
    pub end_line: u32,
    /// Unified diff format.
    pub unified: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_op_builders() {
        let op = EditOp::replace("test.rs", Span::new(0, 10), "new content");
        assert_eq!(op.file_path().to_str().unwrap(), "test.rs");
        assert!(op.expected_hash().is_none());

        let hash = ContentHash::compute(b"test");
        let op = op.with_expected_hash(hash);
        assert!(op.expected_hash().is_some());
    }

    #[test]
    fn test_insert_op() {
        let op = EditOp::insert("test.rs", 50, "inserted text");
        if let EditOp::Insert {
            position, content, ..
        } = op
        {
            assert_eq!(position, 50);
            assert_eq!(content, "inserted text");
        } else {
            panic!("Expected Insert variant");
        }
    }

    #[test]
    fn test_delete_op() {
        let op = EditOp::delete("test.rs", Span::new(10, 20));
        if let EditOp::Delete { span, .. } = op {
            assert_eq!(span.start, 10);
            assert_eq!(span.end, 20);
        } else {
            panic!("Expected Delete variant");
        }
    }

    #[test]
    fn test_replace_lines_op() {
        let op = EditOp::replace_lines("test.rs", 5, 10, "new lines\n");
        if let EditOp::ReplaceLines {
            start_line,
            end_line,
            ..
        } = op
        {
            assert_eq!(start_line, 5);
            assert_eq!(end_line, 10);
        } else {
            panic!("Expected ReplaceLines variant");
        }
    }
}
