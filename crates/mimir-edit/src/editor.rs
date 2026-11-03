//! Editor for applying file operations.

use crate::op::{Diff, EditOp, EditResult as OpResult};
use crate::{EditError, EditResult};
use mimir_types::ContentHash;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Editor for applying file operations with conflict detection.
#[derive(Debug, Default)]
pub struct Editor {
    /// Whether to create backup files before editing.
    create_backups: bool,
    /// What paths in diffs are shown relative to.
    root: Option<PathBuf>,
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

    /// Show paths in diffs relative to this directory.
    pub fn with_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = Some(root.into());
        self
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
                    expected: *expected,
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
                if let Some(expected) = op.expected_hash()
                    && &old_hash != expected
                {
                    return Err(EditError::HashMismatch {
                        path: path.clone(),
                        expected: *expected,
                        actual: old_hash,
                    });
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

    /// Write a whole file, making the directories it needs. A file already there is left alone
    /// unless `overwrite` says otherwise; only then is it replaced, and only if it is text.
    pub async fn write_file(
        &self,
        path: &Path,
        content: &str,
        overwrite: bool,
    ) -> EditResult<OpResult> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let old = if overwrite {
            match fs::read_to_string(path).await {
                Ok(old) => old,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(e.into()),
            }
        } else {
            String::new()
        };
        if overwrite {
            fs::write(path, content).await?;
        } else {
            self.create_file(path, content).await?;
        }
        Ok(OpResult {
            file_path: path.to_path_buf(),
            old_hash: ContentHash::compute(old.as_bytes()),
            new_hash: ContentHash::compute(content.as_bytes()),
            bytes_changed: content.len() as i64 - old.len() as i64,
            lines_changed: content.lines().count() as i64 - old.lines().count() as i64,
            lines_modified: old.lines().count().max(content.lines().count()),
        })
    }

    /// Write a file that must not exist yet. Asking for that in one step, not by looking first,
    /// means nothing that appears in between is lost.
    async fn create_file(&self, path: &Path, content: &str) -> EditResult<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => EditError::FileExists {
                    path: path.to_path_buf(),
                },
                _ => EditError::Io(e),
            })?;
        let written = async {
            file.write_all(content.as_bytes()).await?;
            file.flush().await
        };
        if let Err(e) = written.await {
            fs::remove_file(path).await?;
            return Err(e.into());
        }
        Ok(())
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
            if let [(_, end1), (start2, _)] = window
                && end1 > start2
            {
                return Err(EditError::OverlappingEdits);
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
        let shown = self
            .root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);
        let mut diff = String::new();

        diff.push_str(&format!("--- {}\n", shown.display()));
        diff.push_str(&format!("+++ {}\n", shown.display()));
        diff.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start_line,
            old.lines().count(),
            start_line,
            new.lines().count()
        ));

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
mod tests;
