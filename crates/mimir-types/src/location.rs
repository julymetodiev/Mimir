//! Source code location types.

use crate::FileId;
use serde::{Deserialize, Serialize};

/// A specific position in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    /// The file containing this location.
    pub file_id: FileId,
    /// Line number (1-indexed).
    pub line: u32,
    /// Column number (1-indexed).
    pub column: u32,
}

impl Location {
    /// Create a new location.
    pub const fn new(file_id: FileId, line: u32, column: u32) -> Self {
        Self {
            file_id,
            line,
            column,
        }
    }
}

/// A byte range within a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// Start byte offset (inclusive).
    pub start: u32,
    /// End byte offset (exclusive).
    pub end: u32,
}

impl Span {
    /// Create a new span.
    pub const fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    /// Get the length of the span in bytes.
    #[inline]
    pub const fn len(&self) -> u32 {
        self.end - self.start
    }

    /// Check if the span is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Check if this span contains a byte offset.
    #[inline]
    pub const fn contains(&self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Check if this span overlaps with another.
    #[inline]
    pub const fn overlaps(&self, other: &Span) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Extend this span to include another span.
    pub fn extend(&mut self, other: &Span) {
        self.start = self.start.min(other.start);
        self.end = self.end.max(other.end);
    }
}

/// A range of lines and columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// Start line (1-indexed).
    pub start_line: u32,
    /// Start column (1-indexed).
    pub start_col: u32,
    /// End line (1-indexed).
    pub end_line: u32,
    /// End column (1-indexed).
    pub end_col: u32,
}

impl Range {
    /// Create a new range.
    pub const fn new(start_line: u32, start_col: u32, end_line: u32, end_col: u32) -> Self {
        Self {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    /// Create a range for a single line.
    pub const fn single_line(line: u32, start_col: u32, end_col: u32) -> Self {
        Self {
            start_line: line,
            start_col,
            end_line: line,
            end_col,
        }
    }

    /// Check if this range spans multiple lines.
    #[inline]
    pub const fn is_multiline(&self) -> bool {
        self.start_line != self.end_line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_location() {
        let loc = Location::new(FileId::new(1), 10, 5);
        assert_eq!(loc.line, 10);
        assert_eq!(loc.column, 5);
    }

    #[test]
    fn test_span() {
        let span = Span::new(10, 20);
        assert_eq!(span.len(), 10);
        assert!(!span.is_empty());
        assert!(span.contains(15));
        assert!(!span.contains(5));
        assert!(!span.contains(20));
    }

    #[test]
    fn test_span_overlap() {
        let span1 = Span::new(10, 20);
        let span2 = Span::new(15, 25);
        let span3 = Span::new(25, 30);

        assert!(span1.overlaps(&span2));
        assert!(!span1.overlaps(&span3));
    }

    #[test]
    fn test_range() {
        let range = Range::new(1, 1, 5, 10);
        assert!(range.is_multiline());

        let single = Range::single_line(3, 5, 15);
        assert!(!single.is_multiline());
    }
}
