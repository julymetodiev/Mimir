//! Editor for applying file operations.

use crate::op::{Diff, EditOp, EditResult as OpResult};
use crate::{EditError, EditResult};
use mimir_types::ContentHash;
use std::path::Path;
use tokio::fs;

/// Editor for applying file operations with conflict detection.
#[derive(Debug, Default)]
pub struct Editor {
    /// Whether to create backup files before editing.
    create_backups: bool,
}

impl Editor {
    /// Create a new editor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable backup file creation.
    pub fn with_backups(mut self) -> Self {
        self.create_backups = true;
        self
    }

    /// Apply a single edit operation.
    pub async fn apply(&self, op: &EditOp) -> EditResult<OpResult> {
        let path = op.file_path();

        // Read current content
        let content = self.read_file(path).await?;
        let old_hash = ContentHash::compute(content.as_bytes());

        // Check optimistic lock
        if let Some(expected) = op.expected_hash() {
            if &old_hash != expected {
                return Err(EditError::HashMismatch {
                    path: path.clone(),
                    expected: expected.clone(),
                    actual: old_hash,
                });
            }
        }

        // Create backup if enabled
        if self.create_backups {
            self.create_backup(path, &content).await?;
        }

        // Apply the operation
        let new_content = self.apply_to_content(&content, op)?;
        let new_hash = ContentHash::compute(new_content.as_bytes());

        // Calculate stats
        let old_lines = content.lines().count() as i64;
        let new_lines = new_content.lines().count() as i64;
        let old_bytes = content.len() as i64;
        let new_bytes = new_content.len() as i64;

        // Write the new content
        fs::write(path, &new_content).await?;

        Ok(OpResult {
            file_path: path.clone(),
            old_hash,
            new_hash,
            bytes_changed: new_bytes - old_bytes,
            lines_changed: new_lines - old_lines,
            lines_modified: old_lines.max(new_lines) as usize,
        })
    }

    /// Preview an edit operation without applying it.
    pub async fn preview(&self, op: &EditOp) -> EditResult<Diff> {
        let path = op.file_path();
        let content = self.read_file(path).await?;

        // Check optimistic lock if set
        if let Some(expected) = op.expected_hash() {
            let actual = ContentHash::compute(content.as_bytes());
            if &actual != expected {
                return Err(EditError::HashMismatch {
                    path: path.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }

        let new_content = self.apply_to_content(&content, op)?;
        let (start_line, end_line, old_region, new_region) =
            self.extract_change_region(&content, &new_content, op);

        // Generate unified diff
        let unified = self.generate_unified_diff(path, &old_region, &new_region, start_line);

        Ok(Diff {
            file_path: path.clone(),
            old_content: old_region,
            new_content: new_region,
            start_line,
            end_line,
            unified,
        })
    }

    /// Apply multiple edit operations as a batch.
    /// Operations are sorted and applied in reverse order to maintain correct offsets.
    pub async fn apply_batch(&self, ops: &[EditOp]) -> EditResult<Vec<OpResult>> {
        // Group by file
        let mut by_file: std::collections::HashMap<_, Vec<_>> = std::collections::HashMap::new();
        for op in ops {
            by_file.entry(op.file_path().clone()).or_default().push(op);
        }

        let mut results = Vec::new();

        for (path, file_ops) in by_file {
            // Check for overlapping spans
            self.check_overlaps(&file_ops)?;

            // Read content once
            let content = self.read_file(&path).await?;
            let old_hash = ContentHash::compute(content.as_bytes());

            // Check all expected hashes
            for op in &file_ops {
                if let Some(expected) = op.expected_hash() {
                    if &old_hash != expected {
                        return Err(EditError::HashMismatch {
                            path: path.clone(),
                            expected: expected.clone(),
                            actual: old_hash.clone(),
                        });
                    }
                }
            }

            // Create backup if enabled
            if self.create_backups {
                self.create_backup(&path, &content).await?;
            }

            // Sort operations by start position (descending) to apply from end to start
            let mut sorted_ops: Vec<_> = file_ops.into_iter().collect();
            sorted_ops.sort_by(|a, b| {
                let pos_a = self.get_start_position(a);
                let pos_b = self.get_start_position(b);
                pos_b.cmp(&pos_a)
            });

            // Apply all operations
            let mut current_content = content.clone();
            for op in sorted_ops {
                current_content = self.apply_to_content(&current_content, op)?;
            }

            let new_hash = ContentHash::compute(current_content.as_bytes());
            let old_lines = content.lines().count() as i64;
            let new_lines = current_content.lines().count() as i64;
            let old_bytes = content.len() as i64;
            let new_bytes = current_content.len() as i64;

            // Write once
            fs::write(&path, &current_content).await?;

            results.push(OpResult {
                file_path: path,
                old_hash,
                new_hash,
                bytes_changed: new_bytes - old_bytes,
                lines_changed: new_lines - old_lines,
                lines_modified: old_lines.max(new_lines) as usize,
            });
        }

        Ok(results)
    }

    /// Read file content, returning an error if not found.
    async fn read_file(&self, path: &Path) -> EditResult<String> {
        fs::read_to_string(path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                EditError::FileNotFound {
                    path: path.to_path_buf(),
                }
            } else {
                EditError::Io(e)
            }
        })
    }

    /// Create a backup file.
    async fn create_backup(&self, path: &Path, content: &str) -> EditResult<()> {
        let backup_path = path.with_extension("bak");
        fs::write(&backup_path, content).await?;
        Ok(())
    }

    /// Apply an operation to content string.
    fn apply_to_content(&self, content: &str, op: &EditOp) -> EditResult<String> {
        match op {
            EditOp::Replace {
                file_path,
                span,
                new_content,
                ..
            } => {
                let bytes = content.as_bytes();
                if span.end as usize > bytes.len() {
                    return Err(EditError::InvalidSpan {
                        path: file_path.clone(),
                        start: span.start,
                        end: span.end,
                        size: bytes.len() as u32,
                    });
                }

                let mut result = String::new();
                result.push_str(&content[..span.start as usize]);
                result.push_str(new_content);
                result.push_str(&content[span.end as usize..]);
                Ok(result)
            }

            EditOp::Insert {
                file_path,
                position,
                content: insert_content,
                ..
            } => {
                let bytes = content.as_bytes();
                if *position as usize > bytes.len() {
                    return Err(EditError::InvalidSpan {
                        path: file_path.clone(),
                        start: *position,
                        end: *position,
                        size: bytes.len() as u32,
                    });
                }

                let mut result = String::new();
                result.push_str(&content[..*position as usize]);
                result.push_str(insert_content);
                result.push_str(&content[*position as usize..]);
                Ok(result)
            }

            EditOp::Delete {
                file_path, span, ..
            } => {
                let bytes = content.as_bytes();
                if span.end as usize > bytes.len() {
                    return Err(EditError::InvalidSpan {
                        path: file_path.clone(),
                        start: span.start,
                        end: span.end,
                        size: bytes.len() as u32,
                    });
                }

                let mut result = String::new();
                result.push_str(&content[..span.start as usize]);
                result.push_str(&content[span.end as usize..]);
                Ok(result)
            }

            EditOp::ReplaceLines {
                file_path,
                start_line,
                end_line,
                new_content,
                ..
            } => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len() as u32;

                // Lines are 1-indexed
                if *start_line == 0
                    || *end_line == 0
                    || *start_line > *end_line
                    || *end_line > total_lines
                {
                    return Err(EditError::InvalidLineRange {
                        path: file_path.clone(),
                        start_line: *start_line,
                        end_line: *end_line,
                        total_lines,
                    });
                }

                let mut result = String::new();

                // Add lines before the range
                for line in lines.iter().take(*start_line as usize - 1) {
                    result.push_str(line);
                    result.push('\n');
                }

                // Add new content (ensure it ends with newline)
                result.push_str(new_content);
                if !new_content.ends_with('\n') {
                    result.push('\n');
                }

                // Add lines after the range
                for line in lines.iter().skip(*end_line as usize) {
                    result.push_str(line);
                    result.push('\n');
                }

                // Remove trailing newline if original didn't have one
                if !content.ends_with('\n') && result.ends_with('\n') {
                    result.pop();
                }

                Ok(result)
            }
        }
    }

    /// Get the start position of an operation for sorting.
    fn get_start_position(&self, op: &EditOp) -> u32 {
        match op {
            EditOp::Replace { span, .. } => span.start,
            EditOp::Insert { position, .. } => *position,
            EditOp::Delete { span, .. } => span.start,
            EditOp::ReplaceLines { start_line, .. } => *start_line * 10000, // Approximate
        }
    }

    /// Check for overlapping operations.
    fn check_overlaps(&self, ops: &[&EditOp]) -> EditResult<()> {
        let mut spans: Vec<(u32, u32)> = ops
            .iter()
            .filter_map(|op| match op {
                EditOp::Replace { span, .. } => Some((span.start, span.end)),
                EditOp::Insert { position, .. } => Some((*position, *position)),
                EditOp::Delete { span, .. } => Some((span.start, span.end)),
                EditOp::ReplaceLines { .. } => None, // Line operations handled separately
            })
            .collect();

        spans.sort_by_key(|(start, _)| *start);

        for window in spans.windows(2) {
            if let [(_, end1), (start2, _)] = window {
                if end1 > start2 {
                    return Err(EditError::OverlappingEdits);
                }
            }
        }

        Ok(())
    }

    /// Extract the changed region for diff generation.
    fn extract_change_region(
        &self,
        old: &str,
        new: &str,
        op: &EditOp,
    ) -> (u32, u32, String, String) {
        match op {
            EditOp::ReplaceLines {
                start_line,
                end_line,
                new_content,
                ..
            } => {
                let old_lines: Vec<&str> = old.lines().collect();
                let old_region: String =
                    old_lines[(*start_line as usize - 1)..(*end_line as usize)].join("\n");
                (*start_line, *end_line, old_region, new_content.clone())
            }
            _ => {
                // For byte-based operations, find affected lines
                let old_lines: Vec<&str> = old.lines().collect();
                let new_lines: Vec<&str> = new.lines().collect();

                let mut first_diff = 0;
                for (i, (o, n)) in old_lines.iter().zip(new_lines.iter()).enumerate() {
                    if o != n {
                        first_diff = i;
                        break;
                    }
                }

                let start_line = (first_diff + 1) as u32;
                let end_line = start_line + 1;

                let old_region = old_lines.get(first_diff).unwrap_or(&"").to_string();
                let new_region = new_lines.get(first_diff).unwrap_or(&"").to_string();

                (start_line, end_line, old_region, new_region)
            }
        }
    }

    /// Generate unified diff format.
    fn generate_unified_diff(&self, path: &Path, old: &str, new: &str, start_line: u32) -> String {
        let path_str = path.display();
        let mut diff = String::new();

        diff.push_str(&format!("--- a/{}\n", path_str));
        diff.push_str(&format!("+++ b/{}\n", path_str));
        diff.push_str(&format!("@@ -{},1 +{},1 @@\n", start_line, start_line));

        for line in old.lines() {
            diff.push_str(&format!("-{}\n", line));
        }
        for line in new.lines() {
            diff.push_str(&format!("+{}\n", line));
        }

        diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::Span;
    use tempfile::tempdir;

    async fn create_test_file(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).await.unwrap();
        path
    }

    #[tokio::test]
    async fn test_replace_operation() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.rs", "fn hello() {}").await;

        let op = EditOp::replace(&path, Span::new(3, 8), "world");
        let editor = Editor::new();

        let result = editor.apply(&op).await.unwrap();
        assert_eq!(result.file_path, path);

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "fn world() {}");
    }

    #[tokio::test]
    async fn test_insert_operation() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.rs", "fn test() {}").await;

        let op = EditOp::insert(&path, 3, "pub ");
        let editor = Editor::new();

        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "fn pub test() {}");
    }

    #[tokio::test]
    async fn test_delete_operation() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.rs", "fn hello_world() {}").await;

        let op = EditOp::delete(&path, Span::new(8, 14)); // delete "_world"
        let editor = Editor::new();

        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "fn hello() {}");
    }

    #[tokio::test]
    async fn test_replace_lines_operation() {
        let dir = tempdir().unwrap();
        let content = "line1\nline2\nline3\nline4\n";
        let path = create_test_file(dir.path(), "test.txt", content).await;

        let op = EditOp::replace_lines(&path, 2, 3, "new_line2\nnew_line3\n");
        let editor = Editor::new();

        editor.apply(&op).await.unwrap();

        let result = fs::read_to_string(&path).await.unwrap();
        assert_eq!(result, "line1\nnew_line2\nnew_line3\nline4\n");
    }

    #[tokio::test]
    async fn test_optimistic_locking_success() {
        let dir = tempdir().unwrap();
        let content = "original content";
        let path = create_test_file(dir.path(), "test.txt", content).await;

        let hash = ContentHash::compute(content.as_bytes());
        let op = EditOp::replace(&path, Span::new(0, 8), "modified").with_expected_hash(hash);
        let editor = Editor::new();

        let result = editor.apply(&op).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_optimistic_locking_failure() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "original content").await;

        let wrong_hash = ContentHash::compute(b"different content");
        let op = EditOp::replace(&path, Span::new(0, 8), "modified").with_expected_hash(wrong_hash);
        let editor = Editor::new();

        let result = editor.apply(&op).await;
        assert!(matches!(result, Err(EditError::HashMismatch { .. })));
    }

    #[tokio::test]
    async fn test_preview_operation() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.rs", "fn old() {}").await;

        let op = EditOp::replace(&path, Span::new(3, 6), "new");
        let editor = Editor::new();

        let diff = editor.preview(&op).await.unwrap();
        assert!(diff.unified.contains("-fn old() {}"));
        assert!(diff.unified.contains("+fn new() {}"));

        // Original file should be unchanged
        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "fn old() {}");
    }

    #[tokio::test]
    async fn test_batch_operations() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "aaa bbb ccc").await;

        let ops = vec![
            EditOp::replace(&path, Span::new(0, 3), "AAA"),
            EditOp::replace(&path, Span::new(8, 11), "CCC"),
        ];
        let editor = Editor::new();

        let results = editor.apply_batch(&ops).await.unwrap();
        assert_eq!(results.len(), 1);

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "AAA bbb CCC");
    }

    #[tokio::test]
    async fn test_invalid_span() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "short").await;

        let op = EditOp::replace(&path, Span::new(0, 100), "new");
        let editor = Editor::new();

        let result = editor.apply(&op).await;
        assert!(matches!(result, Err(EditError::InvalidSpan { .. })));
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.txt");

        let op = EditOp::replace(&path, Span::new(0, 5), "new");
        let editor = Editor::new();

        let result = editor.apply(&op).await;
        assert!(matches!(result, Err(EditError::FileNotFound { .. })));
    }

    #[tokio::test]
    async fn test_backup_creation() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "original").await;

        let op = EditOp::replace(&path, Span::new(0, 8), "modified");
        let editor = Editor::new().with_backups();

        editor.apply(&op).await.unwrap();

        let backup_path = path.with_extension("bak");
        let backup_content = fs::read_to_string(&backup_path).await.unwrap();
        assert_eq!(backup_content, "original");
    }

    // ==================== Advanced Tests ====================

    #[tokio::test]
    async fn test_empty_file_operations() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "empty.txt", "").await;
        let editor = Editor::new();

        // Insert into empty file
        let op = EditOp::insert(&path, 0, "hello world");
        let result = editor.apply(&op).await.unwrap();
        assert_eq!(result.bytes_changed, 11);

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_unicode_content() {
        let dir = tempdir().unwrap();
        let original = "Привет мир! 你好世界 🦀🔥";
        let path = create_test_file(dir.path(), "unicode.txt", original).await;
        let editor = Editor::new();

        // Replace unicode characters - need byte offsets
        let privet_end = "Привет".len(); // 12 bytes in UTF-8

        let op = EditOp::replace(&path, Span::new(0, privet_end as u32), "Здравей");
        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.starts_with("Здравей"));
        assert!(content.contains("🦀"));
    }

    #[tokio::test]
    async fn test_multiline_replace() {
        let dir = tempdir().unwrap();
        let content = r#"fn main() {
    println!("Hello");
    println!("World");
}
"#;
        let path = create_test_file(dir.path(), "main.rs", content).await;
        let editor = Editor::new();

        // Replace lines 2-3 with a single line (4 lines -> 3 lines)
        let op = EditOp::replace_lines(&path, 2, 3, "    println!(\"Hello World\");\n");
        editor.apply(&op).await.unwrap();

        let result = fs::read_to_string(&path).await.unwrap();
        assert!(result.contains("Hello World"));
        assert!(!result.contains("\"Hello\""));
        assert!(!result.contains("\"World\""));
        // Original: 4 lines, replaced 2 lines with 1 = 3 lines total
        assert_eq!(result.lines().count(), 3);
    }

    #[tokio::test]
    async fn test_replace_first_line() {
        let dir = tempdir().unwrap();
        let content = "first\nsecond\nthird\n";
        let path = create_test_file(dir.path(), "test.txt", content).await;
        let editor = Editor::new();

        let op = EditOp::replace_lines(&path, 1, 1, "FIRST\n");
        editor.apply(&op).await.unwrap();

        let result = fs::read_to_string(&path).await.unwrap();
        assert_eq!(result, "FIRST\nsecond\nthird\n");
    }

    #[tokio::test]
    async fn test_replace_last_line() {
        let dir = tempdir().unwrap();
        let content = "first\nsecond\nthird\n";
        let path = create_test_file(dir.path(), "test.txt", content).await;
        let editor = Editor::new();

        let op = EditOp::replace_lines(&path, 3, 3, "THIRD\n");
        editor.apply(&op).await.unwrap();

        let result = fs::read_to_string(&path).await.unwrap();
        assert_eq!(result, "first\nsecond\nTHIRD\n");
    }

    #[tokio::test]
    async fn test_invalid_line_range_zero() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "line1\nline2\n").await;
        let editor = Editor::new();

        // Line 0 is invalid (1-indexed)
        let op = EditOp::replace_lines(&path, 0, 1, "new\n");
        let result = editor.apply(&op).await;
        assert!(matches!(result, Err(EditError::InvalidLineRange { .. })));
    }

    #[tokio::test]
    async fn test_invalid_line_range_reversed() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "line1\nline2\nline3\n").await;
        let editor = Editor::new();

        // start > end is invalid
        let op = EditOp::replace_lines(&path, 3, 1, "new\n");
        let result = editor.apply(&op).await;
        assert!(matches!(result, Err(EditError::InvalidLineRange { .. })));
    }

    #[tokio::test]
    async fn test_invalid_line_range_exceeds_file() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "line1\nline2\n").await;
        let editor = Editor::new();

        // Line 5 doesn't exist
        let op = EditOp::replace_lines(&path, 1, 5, "new\n");
        let result = editor.apply(&op).await;
        assert!(matches!(result, Err(EditError::InvalidLineRange { .. })));
    }

    #[tokio::test]
    async fn test_overlapping_edits_detection() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "0123456789").await;
        let editor = Editor::new();

        // These spans overlap: 2-6 and 4-8
        let ops = vec![
            EditOp::replace(&path, Span::new(2, 6), "XX"),
            EditOp::replace(&path, Span::new(4, 8), "YY"),
        ];

        let result = editor.apply_batch(&ops).await;
        assert!(matches!(result, Err(EditError::OverlappingEdits)));
    }

    #[tokio::test]
    async fn test_adjacent_edits_allowed() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "0123456789").await;
        let editor = Editor::new();

        // Adjacent spans: 0-3 and 3-6 (touching but not overlapping)
        let ops = vec![
            EditOp::replace(&path, Span::new(0, 3), "AAA"),
            EditOp::replace(&path, Span::new(3, 6), "BBB"),
        ];

        let results = editor.apply_batch(&ops).await.unwrap();
        assert_eq!(results.len(), 1);

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "AAABBB6789");
    }

    #[tokio::test]
    async fn test_batch_multiple_files() {
        let dir = tempdir().unwrap();
        let path1 = create_test_file(dir.path(), "file1.txt", "content1").await;
        let path2 = create_test_file(dir.path(), "file2.txt", "content2").await;
        let editor = Editor::new();

        let ops = vec![
            EditOp::replace(&path1, Span::new(0, 7), "CONTENT"),
            EditOp::replace(&path2, Span::new(0, 7), "MODIFIED"),
        ];

        let results = editor.apply_batch(&ops).await.unwrap();
        assert_eq!(results.len(), 2);

        let c1 = fs::read_to_string(&path1).await.unwrap();
        let c2 = fs::read_to_string(&path2).await.unwrap();
        assert_eq!(c1, "CONTENT1");
        assert_eq!(c2, "MODIFIED2");
    }

    #[tokio::test]
    async fn test_sequential_edits_same_file() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "hello world").await;
        let editor = Editor::new();

        // First edit
        let op1 = EditOp::replace(&path, Span::new(0, 5), "HELLO");
        let result1 = editor.apply(&op1).await.unwrap();

        // Second edit - use new hash
        let op2 =
            EditOp::replace(&path, Span::new(6, 11), "WORLD").with_expected_hash(result1.new_hash);
        editor.apply(&op2).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "HELLO WORLD");
    }

    #[tokio::test]
    async fn test_concurrent_modification_detected() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "original").await;
        let editor = Editor::new();

        // Get hash of original content
        let original_hash = ContentHash::compute(b"original");

        // Simulate another process modifying the file
        fs::write(&path, "modified by other").await.unwrap();

        // Try to edit with old hash - should fail
        let op =
            EditOp::replace(&path, Span::new(0, 8), "my edit").with_expected_hash(original_hash);

        let result = editor.apply(&op).await;
        assert!(matches!(result, Err(EditError::HashMismatch { .. })));

        // Verify the "other" modification is preserved
        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "modified by other");
    }

    #[tokio::test]
    async fn test_insert_at_end() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "hello").await;
        let editor = Editor::new();

        let op = EditOp::insert(&path, 5, " world");
        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_insert_at_start() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "world").await;
        let editor = Editor::new();

        let op = EditOp::insert(&path, 0, "hello ");
        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_delete_entire_content() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "delete me").await;
        let editor = Editor::new();

        let op = EditOp::delete(&path, Span::new(0, 9));
        let result = editor.apply(&op).await.unwrap();

        assert_eq!(result.bytes_changed, -9);

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "");
    }

    #[tokio::test]
    async fn test_preview_does_not_modify() {
        let dir = tempdir().unwrap();
        let original = "do not change";
        let path = create_test_file(dir.path(), "test.txt", original).await;
        let editor = Editor::new();

        // Preview multiple times
        for _ in 0..3 {
            let op = EditOp::replace(&path, Span::new(0, 2), "XX");
            let diff = editor.preview(&op).await.unwrap();
            assert!(diff.unified.contains("-do"));
            assert!(diff.unified.contains("+XX"));
        }

        // File should be unchanged
        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, original);
    }

    #[tokio::test]
    async fn test_result_stats_accuracy() {
        let dir = tempdir().unwrap();
        let original = "line1\nline2\nline3\n";
        let path = create_test_file(dir.path(), "test.txt", original).await;
        let editor = Editor::new();

        // Replace 2 lines with 3 lines
        let op = EditOp::replace_lines(&path, 1, 2, "new1\nnew2\nnew3\n");
        let result = editor.apply(&op).await.unwrap();

        assert_eq!(result.lines_changed, 1); // 3 lines - 2 lines = +1
        assert!(result.bytes_changed > 0);

        // Verify hashes are different
        assert_ne!(result.old_hash, result.new_hash);
    }

    #[tokio::test]
    async fn test_large_file_edit() {
        let dir = tempdir().unwrap();

        // Create a file with 1000 lines (1-indexed: line 1 = "Line number 1")
        let mut content = String::new();
        for i in 1..=1000 {
            content.push_str(&format!("Line number {}\n", i));
        }
        let path = create_test_file(dir.path(), "large.txt", &content).await;
        let editor = Editor::new();

        // Edit line 500 (which contains "Line number 500")
        let op = EditOp::replace_lines(&path, 500, 500, "MODIFIED LINE 500\n");
        let result = editor.apply(&op).await.unwrap();

        assert!(result.bytes_changed != 0);

        let result_content = fs::read_to_string(&path).await.unwrap();
        assert!(result_content.contains("MODIFIED LINE 500"));
        // Line 499 and 501 should be unchanged
        assert!(result_content.contains("Line number 499"));
        assert!(result_content.contains("Line number 501"));
        // Original line 500 should be gone
        assert!(!result_content.contains("Line number 500\n"));
    }

    #[tokio::test]
    async fn test_special_characters_in_content() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "placeholder").await;
        let editor = Editor::new();

        // Content with special regex/shell characters
        let special = r#"$HOME/*.rs | grep -E "test\d+" && echo 'done'"#;
        let op = EditOp::replace(&path, Span::new(0, 11), special);
        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, special);
    }

    #[tokio::test]
    async fn test_newline_handling_no_trailing() {
        let dir = tempdir().unwrap();
        // File without trailing newline
        let path = create_test_file(dir.path(), "test.txt", "no newline").await;
        let editor = Editor::new();

        let op = EditOp::replace(&path, Span::new(3, 10), "NEWLINE");
        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "no NEWLINE");
        assert!(!content.ends_with('\n'));
    }

    #[tokio::test]
    async fn test_newline_handling_with_trailing() {
        let dir = tempdir().unwrap();
        // File with trailing newline
        let path = create_test_file(dir.path(), "test.txt", "with newline\n").await;
        let editor = Editor::new();

        let op = EditOp::replace(&path, Span::new(5, 12), "NEWLINE");
        editor.apply(&op).await.unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "with NEWLINE\n");
    }

    #[tokio::test]
    async fn test_hash_chain_verification() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "step0").await;
        let editor = Editor::new();

        // Chain of edits, each using previous hash
        let mut current_hash = ContentHash::compute(b"step0");

        for i in 1..=5 {
            let new_content = format!("step{}", i);
            let op = EditOp::replace(&path, Span::new(0, 5), &new_content)
                .with_expected_hash(current_hash);

            let result = editor.apply(&op).await.unwrap();
            current_hash = result.new_hash;

            let content = fs::read_to_string(&path).await.unwrap();
            assert_eq!(content, new_content);
        }
    }

    #[tokio::test]
    async fn test_batch_with_inserts_and_deletes() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "AABBCCDD").await;
        let editor = Editor::new();

        // Multiple non-overlapping operations
        let ops = vec![
            EditOp::delete(&path, Span::new(0, 2)), // Remove "AA"
            EditOp::insert(&path, 4, "XX"),         // Insert after "BB"
            EditOp::delete(&path, Span::new(6, 8)), // Remove "DD"
        ];

        let results = editor.apply_batch(&ops).await.unwrap();
        assert_eq!(results.len(), 1);

        let content = fs::read_to_string(&path).await.unwrap();
        // After sorting by position (descending) and applying:
        // Original: AABBCCDD
        // Delete 6-8 (DD): AABBCC
        // Insert at 4 (XX): AABBXXCC
        // Delete 0-2 (AA): BBXXCC
        assert_eq!(content, "BBXXCC");
    }

    #[tokio::test]
    async fn test_preview_with_hash_mismatch() {
        let dir = tempdir().unwrap();
        let path = create_test_file(dir.path(), "test.txt", "content").await;
        let editor = Editor::new();

        let wrong_hash = ContentHash::compute(b"wrong");
        let op = EditOp::replace(&path, Span::new(0, 7), "new").with_expected_hash(wrong_hash);

        let result = editor.preview(&op).await;
        assert!(matches!(result, Err(EditError::HashMismatch { .. })));
    }
}
