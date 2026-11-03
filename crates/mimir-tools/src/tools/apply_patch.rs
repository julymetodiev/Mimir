//! Apply patch tool for creating, modifying, and deleting files.
//!
//! Uses the same structured patch format as Codex for reliable file editing.
//! The format is simpler and more robust than ad-hoc diff formats.

use crate::{Tool, ToolContext, ToolResult, ToolSchema, ToolParameter};
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::Path;

// ============================================================================
// Patch Format Constants
// ============================================================================

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const ADD_FILE_MARKER: &str = "*** Add File: ";
const DELETE_FILE_MARKER: &str = "*** Delete File: ";
const UPDATE_FILE_MARKER: &str = "*** Update File: ";
const MOVE_TO_MARKER: &str = "*** Move to: ";
const EOF_MARKER: &str = "*** End of File";
const CHANGE_CONTEXT_MARKER: &str = "@@ ";
const EMPTY_CHANGE_CONTEXT_MARKER: &str = "@@";

/// Maximum allowed lines in a patch (prevents full-file rewrites)
const MAX_PATCH_LINES: usize = 100;
/// Maximum context lines per hunk
const MAX_CONTEXT_LINES: usize = 10;

// ============================================================================
// Data Structures
// ============================================================================

/// A parsed patch operation, named after the patch format's `*** Add File:` and so on.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum PatchOp {
    /// Create a new file with content
    AddFile { path: String, content: String },
    /// Delete an existing file
    DeleteFile { path: String },
    /// Update an existing file with chunks
    UpdateFile {
        path: String,
        move_to: Option<String>,
        chunks: Vec<UpdateChunk>,
    },
}

/// A chunk within an UpdateFile operation
#[derive(Debug, Clone)]
pub struct UpdateChunk {
    /// Optional context marker (e.g., "def foo():" or "impl User")
    pub context: Option<String>,
    /// Lines that should be found/removed (context + removed lines)
    pub old_lines: Vec<String>,
    /// Lines that should replace old_lines (context + added lines)
    pub new_lines: Vec<String>,
    /// Whether this chunk should match at end of file
    pub is_eof: bool,
}

/// Patch parsing/application errors
#[derive(Debug, Clone)]
pub enum PatchError {
    /// Missing Begin/End markers
    MissingMarkers(String),
    /// Invalid patch format
    InvalidFormat { message: String, line: usize },
    /// File not found for update/delete
    FileNotFound(String),
    /// File already exists for add
    FileAlreadyExists(String),
    /// Could not find lines to replace
    LinesNotFound { file: String, lines: Vec<String> },
    /// IO error
    IoError(String),
    /// Patch is too large (prevents full-file rewrites)
    PatchTooLarge { lines: usize, max: usize },
    /// Too many context lines in a chunk
    TooManyContextLines { count: usize, max: usize },
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::MissingMarkers(msg) => write!(f, "{}", msg),
            PatchError::InvalidFormat { message, line } => {
                write!(f, "Invalid patch at line {}: {}", line, message)
            }
            PatchError::FileNotFound(path) => {
                write!(f, "File not found: {}. Use '*** Add File:' to create new files.", path)
            }
            PatchError::FileAlreadyExists(path) => {
                write!(f, "File already exists: {}. Use '*** Update File:' to modify.", path)
            }
            PatchError::LinesNotFound { file, lines } => {
                write!(f, "Could not find lines in '{}':\n{}\nHint: Read the file to verify current content.",
                    file,
                    lines.iter().take(5).map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n"))
            }
            PatchError::IoError(msg) => write!(f, "IO error: {}", msg),
            PatchError::PatchTooLarge { lines, max } => {
                write!(f, "Patch too large: {} lines (max {}). Make smaller, targeted changes instead of rewriting entire files. Use multiple smaller patches if needed.", lines, max)
            }
            PatchError::TooManyContextLines { count, max } => {
                write!(f, "Too many context lines: {} (max {}). Include only 2-3 context lines before/after each change.", count, max)
            }
        }
    }
}

impl std::error::Error for PatchError {}

// ============================================================================
// Parser
// ============================================================================

pub struct ApplyPatchTool;

impl ApplyPatchTool {
    /// Parse a patch string into operations
    pub fn parse_patch(input: &str) -> Result<Vec<PatchOp>, PatchError> {
        let input = input.replace("\r\n", "\n");
        let lines: Vec<&str> = input.trim().lines().collect();

        // Check for required markers
        let first_line = lines.first().map(|s| s.trim());
        let last_line = lines.last().map(|s| s.trim());

        if first_line != Some(BEGIN_PATCH_MARKER) {
            return Err(PatchError::MissingMarkers(
                "Patch must start with '*** Begin Patch'".into()
            ));
        }

        // End marker is optional (model often truncates output)
        let has_end_marker = last_line == Some(END_PATCH_MARKER);

        // Parse operations between markers
        let mut ops = Vec::new();
        let end_idx = if has_end_marker { lines.len() - 1 } else { lines.len() };
        let content_lines = &lines[1..end_idx];
        let mut idx = 0;
        let mut line_num = 2; // 1-indexed, starting after Begin Patch

        while idx < content_lines.len() {
            let line = content_lines[idx].trim();

            // Skip empty lines between operations
            if line.is_empty() {
                idx += 1;
                line_num += 1;
                continue;
            }

            // Skip intermediate *** End Patch markers (model error - should only be at end)
            if line == END_PATCH_MARKER {
                idx += 1;
                line_num += 1;
                continue;
            }

            if let Some(path) = line.strip_prefix(ADD_FILE_MARKER) {
                // Add File
                let (op, consumed) = Self::parse_add_file(path, &content_lines[idx+1..], line_num)?;
                ops.push(op);
                idx += 1 + consumed;
                line_num += 1 + consumed;
            } else if let Some(path) = line.strip_prefix(DELETE_FILE_MARKER) {
                // Delete File
                ops.push(PatchOp::DeleteFile { path: path.to_string() });
                idx += 1;
                line_num += 1;
            } else if let Some(path) = line.strip_prefix(UPDATE_FILE_MARKER) {
                // Update File
                let (op, consumed) = Self::parse_update_file(path, &content_lines[idx+1..], line_num)?;
                ops.push(op);
                idx += 1 + consumed;
                line_num += 1 + consumed;
            } else {
                return Err(PatchError::InvalidFormat {
                    message: format!("Expected file operation, got: '{}'", line),
                    line: line_num,
                });
            }
        }

        Ok(ops)
    }

    /// Parse an Add File section
    fn parse_add_file(path: &str, lines: &[&str], _start_line: usize) -> Result<(PatchOp, usize), PatchError> {
        let mut content = String::new();
        let mut consumed = 0;

        for line in lines {
            if let Some(text) = line.strip_prefix('+') {
                content.push_str(text);
                content.push('\n');
                consumed += 1;
            } else {
                break;
            }
        }

        Ok((PatchOp::AddFile {
            path: path.to_string(),
            content
        }, consumed))
    }

    /// Parse an Update File section
    fn parse_update_file(path: &str, lines: &[&str], start_line: usize) -> Result<(PatchOp, usize), PatchError> {
        let mut move_to = None;
        let mut chunks = Vec::new();
        let mut idx = 0;

        // Check for optional Move to
        if let Some(line) = lines.first()
            && let Some(new_path) = line.strip_prefix(MOVE_TO_MARKER) {
                move_to = Some(new_path.to_string());
                idx += 1;
            }

        // Parse chunks
        while idx < lines.len() {
            let line = lines[idx];

            // Stop at next file operation
            if line.starts_with("*** ") && !line.starts_with("*** End of File") {
                break;
            }

            // Skip empty lines between chunks
            if line.trim().is_empty() {
                idx += 1;
                continue;
            }

            // Parse one chunk
            let (chunk, consumed) = Self::parse_chunk(&lines[idx..], start_line + idx, chunks.is_empty())?;
            chunks.push(chunk);
            idx += consumed;
        }

        if chunks.is_empty() {
            return Err(PatchError::InvalidFormat {
                message: format!("Update file '{}' has no chunks", path),
                line: start_line,
            });
        }

        Ok((PatchOp::UpdateFile {
            path: path.to_string(),
            move_to,
            chunks,
        }, idx))
    }

    /// Parse a single chunk (context marker + change lines)
    fn parse_chunk(lines: &[&str], start_line: usize, allow_no_marker: bool) -> Result<(UpdateChunk, usize), PatchError> {
        let mut context = None;
        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();
        let mut is_eof = false;
        let mut idx = 0;

        // Check for context marker
        if let Some(line) = lines.first() {
            if *line == EMPTY_CHANGE_CONTEXT_MARKER {
                idx = 1;
            } else if let Some(ctx) = line.strip_prefix(CHANGE_CONTEXT_MARKER) {
                context = Some(ctx.to_string());
                idx = 1;
            } else if !allow_no_marker {
                return Err(PatchError::InvalidFormat {
                    message: format!("Expected @@ context marker, got: '{}'", line),
                    line: start_line,
                });
            }
        }

        // Parse change lines
        let mut parsed_any = false;
        while idx < lines.len() {
            let line = lines[idx];

            if line == EOF_MARKER {
                is_eof = true;
                idx += 1;
                break;
            }

            // Stop at next chunk or file operation
            if line.starts_with("@@") || line.starts_with("*** ") {
                break;
            }

            let first_char = line.chars().next();
            match first_char {
                Some(' ') => {
                    // Context line - goes in both old and new
                    let text = &line[1..];
                    old_lines.push(text.to_string());
                    new_lines.push(text.to_string());
                    parsed_any = true;
                }
                Some('+') => {
                    // Add line - only in new
                    let text = &line[1..];
                    new_lines.push(text.to_string());
                    parsed_any = true;
                }
                Some('-') => {
                    // Remove line - only in old
                    let text = &line[1..];
                    old_lines.push(text.to_string());
                    parsed_any = true;
                }
                None => {
                    // Empty line - treat as context
                    old_lines.push(String::new());
                    new_lines.push(String::new());
                    parsed_any = true;
                }
                _ => {
                    // Line without valid prefix - reject with clear error
                    return Err(PatchError::InvalidFormat {
                        message: format!(
                            "Line must start with ' ' (space), '+', or '-'. Got: '{}'",
                            if line.len() > 40 { &line[..40] } else { line }
                        ),
                        line: start_line + idx,
                    });
                }
            }
            idx += 1;
        }

        if !parsed_any {
            return Err(PatchError::InvalidFormat {
                message: "Chunk contains no change lines".into(),
                line: start_line,
            });
        }

        Ok((UpdateChunk {
            context,
            old_lines,
            new_lines,
            is_eof,
        }, idx))
    }

    // ========================================================================
    // Application
    // ========================================================================

    /// Apply a patch operation to the filesystem
    fn apply_operation(op: &PatchOp, root: &Path) -> Result<String, PatchError> {
        match op {
            PatchOp::AddFile { path, content } => {
                let full_path = root.join(path);

                if full_path.exists() {
                    return Err(PatchError::FileAlreadyExists(path.clone()));
                }

                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| PatchError::IoError(format!("Failed to create directory: {}", e)))?;
                }

                fs::write(&full_path, content)
                    .map_err(|e| PatchError::IoError(format!("Failed to write file: {}", e)))?;

                Ok(format!("A {}", path))
            }
            PatchOp::DeleteFile { path } => {
                let full_path = root.join(path);

                if !full_path.exists() {
                    return Err(PatchError::FileNotFound(path.clone()));
                }

                fs::remove_file(&full_path)
                    .map_err(|e| PatchError::IoError(format!("Failed to delete file: {}", e)))?;

                Ok(format!("D {}", path))
            }
            PatchOp::UpdateFile { path, move_to, chunks } => {
                let full_path = root.join(path);

                if !full_path.exists() {
                    return Err(PatchError::FileNotFound(path.clone()));
                }

                let content = fs::read_to_string(&full_path)
                    .map_err(|e| PatchError::IoError(format!("Failed to read file: {}", e)))?;

                let mut lines: Vec<String> = content.lines().map(String::from).collect();

                // Apply chunks in order, tracking position
                let mut search_start = 0;
                for chunk in chunks {
                    search_start = Self::apply_chunk(&mut lines, chunk, path, search_start)?;
                }

                let new_content = if lines.is_empty() {
                    String::new()
                } else {
                    lines.join("\n") + "\n"
                };

                // Handle move/rename
                let target_path = if let Some(new_path) = move_to {
                    let new_full_path = root.join(new_path);
                    if let Some(parent) = new_full_path.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|e| PatchError::IoError(format!("Failed to create directory: {}", e)))?;
                    }
                    fs::remove_file(&full_path)
                        .map_err(|e| PatchError::IoError(format!("Failed to delete old file: {}", e)))?;
                    new_full_path
                } else {
                    full_path
                };

                fs::write(&target_path, new_content)
                    .map_err(|e| PatchError::IoError(format!("Failed to write file: {}", e)))?;

                if move_to.is_some() {
                    Ok(format!("R {} -> {}", path, move_to.as_ref().unwrap()))
                } else {
                    Ok(format!("M {}", path))
                }
            }
        }
    }

    /// Apply a single chunk to lines, return new search position
    fn apply_chunk(lines: &mut Vec<String>, chunk: &UpdateChunk, path: &str, start: usize) -> Result<usize, PatchError> {
        // If we have a context marker, find it first
        let search_start = if let Some(ref ctx) = chunk.context {
            Self::find_context(lines, ctx, start).unwrap_or(start)
        } else {
            start
        };

        // If old_lines is empty, this is a pure addition
        if chunk.old_lines.is_empty() {
            // Add at end of file (or after context if found)
            let insert_pos = if chunk.is_eof || chunk.context.is_none() {
                lines.len()
            } else {
                // Insert after the context line
                search_start + 1
            };

            for (i, new_line) in chunk.new_lines.iter().enumerate() {
                if insert_pos + i <= lines.len() {
                    lines.insert(insert_pos + i, new_line.clone());
                } else {
                    lines.push(new_line.clone());
                }
            }
            return Ok(insert_pos + chunk.new_lines.len());
        }

        // Find old_lines in the file using fuzzy matching
        let found_pos = Self::seek_sequence(lines, &chunk.old_lines, search_start, chunk.is_eof)
            .ok_or_else(|| PatchError::LinesNotFound {
                file: path.to_string(),
                lines: chunk.old_lines.clone(),
            })?;

        // Replace old_lines with new_lines
        // Remove old lines
        for _ in 0..chunk.old_lines.len() {
            if found_pos < lines.len() {
                lines.remove(found_pos);
            }
        }
        // Insert new lines
        for (i, new_line) in chunk.new_lines.iter().enumerate() {
            lines.insert(found_pos + i, new_line.clone());
        }

        Ok(found_pos + chunk.new_lines.len())
    }

    /// Find a context marker in lines
    fn find_context(lines: &[String], marker: &str, start: usize) -> Option<usize> {
        for (i, line) in lines.iter().enumerate().skip(start) {
            if line.contains(marker) {
                return Some(i);
            }
        }
        None
    }

    /// Find a sequence of lines using 4-stage fuzzy matching (like Codex)
    fn seek_sequence(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
        if pattern.is_empty() {
            return Some(start);
        }
        if pattern.len() > lines.len() {
            return None;
        }

        let search_start = if eof && lines.len() >= pattern.len() {
            lines.len() - pattern.len()
        } else {
            start
        };

        // Stage 1: Exact match
        for i in search_start..=lines.len().saturating_sub(pattern.len()) {
            if Self::matches_exact(lines, pattern, i) {
                return Some(i);
            }
        }

        // Stage 2: Trim trailing whitespace
        for i in search_start..=lines.len().saturating_sub(pattern.len()) {
            if Self::matches_rstrip(lines, pattern, i) {
                return Some(i);
            }
        }

        // Stage 3: Trim all whitespace
        for i in search_start..=lines.len().saturating_sub(pattern.len()) {
            if Self::matches_trim(lines, pattern, i) {
                return Some(i);
            }
        }

        // Stage 4: Unicode normalization
        (search_start..=lines.len().saturating_sub(pattern.len())).find(|&i| Self::matches_normalized(lines, pattern, i))
    }

    fn matches_exact(lines: &[String], pattern: &[String], pos: usize) -> bool {
        pattern.iter().enumerate().all(|(j, p)| lines[pos + j] == *p)
    }

    fn matches_rstrip(lines: &[String], pattern: &[String], pos: usize) -> bool {
        pattern.iter().enumerate().all(|(j, p)| {
            lines[pos + j].trim_end() == p.trim_end()
        })
    }

    fn matches_trim(lines: &[String], pattern: &[String], pos: usize) -> bool {
        pattern.iter().enumerate().all(|(j, p)| {
            lines[pos + j].trim() == p.trim()
        })
    }

    fn matches_normalized(lines: &[String], pattern: &[String], pos: usize) -> bool {
        pattern.iter().enumerate().all(|(j, p)| {
            Self::normalize(&lines[pos + j]) == Self::normalize(p)
        })
    }

    /// Normalize unicode characters (fancy quotes, dashes → ASCII)
    fn normalize(s: &str) -> String {
        s.trim().chars().map(|c| match c {
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{2018}'..='\u{201B}' => '\'',
            '\u{201C}'..='\u{201F}' => '"',
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
            other => other,
        }).collect()
    }

    /// Validate that patch operations aren't too large
    fn validate_patch(ops: &[PatchOp]) -> Result<(), PatchError> {
        for op in ops {
            match op {
                PatchOp::AddFile { content, .. } => {
                    let lines = content.lines().count();
                    if lines > MAX_PATCH_LINES {
                        return Err(PatchError::PatchTooLarge {
                            lines,
                            max: MAX_PATCH_LINES,
                        });
                    }
                }
                PatchOp::UpdateFile { chunks, .. } => {
                    for chunk in chunks {
                        // Count context lines (lines that appear in both old and new)
                        let context_count = chunk.old_lines.iter()
                            .zip(&chunk.new_lines)
                            .filter(|(old, new)| old == new)
                            .count();

                        if context_count > MAX_CONTEXT_LINES {
                            return Err(PatchError::TooManyContextLines {
                                count: context_count,
                                max: MAX_CONTEXT_LINES,
                            });
                        }
                    }
                }
                PatchOp::DeleteFile { .. } => {}
            }
        }
        Ok(())
    }
}

// ============================================================================
// Tool Implementation
// ============================================================================

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        r#"Use the `apply_patch` tool to edit files.
Your patch language is a stripped-down, file-oriented diff format designed to be easy to parse and safe to apply. You can think of it as a high-level envelope:

*** Begin Patch
[ one or more file sections ]
*** End Patch

Within that envelope, you get a sequence of file operations.
You MUST include a header to specify the action you are taking.
Each operation starts with one of three headers:

*** Add File: <path> - create a new file. Every following line is a + line (the initial contents).
*** Delete File: <path> - remove an existing file. Nothing follows.
*** Update File: <path> - patch an existing file in place.

Then one or more "hunks", each introduced by @@ (optionally followed by a hunk header).
Within a hunk each line starts with:

' ' (space) = context line - MUST already exist in file (used to locate where to apply changes)
'-' = line to remove - MUST already exist in file
'+' = line to add - this is NEW content

For instructions on [context_before] and [context_after]:
- By default, show 3 lines of code immediately above and 3 lines immediately below each change.
- If 3 lines of context is insufficient to uniquely identify the snippet of code within the file, use the @@ operator to indicate the class or function to which the snippet belongs. For instance:
@@ impl User
 [3 lines of pre-context]
-[old_code]
+[new_code]
 [3 lines of post-context]

The full grammar definition is below:
Patch := Begin { FileOp } End
Begin := "*** Begin Patch" NEWLINE
End := "*** End Patch" NEWLINE
FileOp := AddFile | DeleteFile | UpdateFile
AddFile := "*** Add File: " path NEWLINE { "+" line NEWLINE }
DeleteFile := "*** Delete File: " path NEWLINE
UpdateFile := "*** Update File: " path NEWLINE { Hunk }
Hunk := "@@" [ header ] NEWLINE { HunkLine } [ "*** End of File" NEWLINE ]
HunkLine := (" " | "-" | "+") text NEWLINE

A full patch can combine several operations:

*** Begin Patch
*** Add File: src/role.rs
+pub enum Role {
+    Admin,
+    User,
+    Guest,
+}
*** Update File: src/user.rs
@@ pub struct User
 pub struct User {
     id: u32,
     name: String,
+    role: Role,
 }
*** End Patch

IMPORTANT SIZE LIMITS:
- Maximum 100 lines per patch (total)
- Maximum 10 context lines per hunk
- Use 2-3 context lines, NOT the entire file
- Make small, targeted changes - DO NOT rewrite entire files
- If you need more changes, use multiple smaller patches

It is important to remember:

- You must include a header with your intended action (Add/Delete/Update)
- You must prefix new lines with `+` even when creating a new file
- Context lines (space prefix) MUST already exist in the file - they locate where changes go
- File references can only be relative, NEVER ABSOLUTE.
- NEVER include entire file content as context - only 2-3 lines around each change"#
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new("apply_patch", self.description())
            .with_param(ToolParameter::required_string(
                "patch",
                "The patch content with *** Begin Patch and *** End Patch markers"
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let patch_str = match params.get("patch").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error(
                "Missing 'patch' parameter. Provide patch content with *** Begin Patch and *** End Patch markers."
            ),
        };

        // Check raw patch size first (quick reject for massive patches)
        let line_count = patch_str.lines().count();
        if line_count > MAX_PATCH_LINES {
            return ToolResult::error(format!(
                "Patch too large: {} lines (max {}). Make smaller, targeted changes instead of rewriting entire files. Use multiple smaller patches if needed.",
                line_count, MAX_PATCH_LINES
            ));
        }

        // Parse the patch
        let ops = match Self::parse_patch(patch_str) {
            Ok(ops) => ops,
            Err(e) => return ToolResult::error(e.to_string()),
        };

        if ops.is_empty() {
            return ToolResult::error("Patch contains no operations");
        }

        // Validate patch chunks aren't too large
        if let Err(e) = Self::validate_patch(&ops) {
            return ToolResult::error(e.to_string());
        }

        if context.dry_run {
            let summary = ops.iter().map(|op| match op {
                PatchOp::AddFile { path, .. } => format!("Would create: {}", path),
                PatchOp::DeleteFile { path } => format!("Would delete: {}", path),
                PatchOp::UpdateFile { path, move_to, .. } => {
                    if let Some(new_path) = move_to {
                        format!("Would rename: {} -> {}", path, new_path)
                    } else {
                        format!("Would modify: {}", path)
                    }
                }
            }).collect::<Vec<_>>().join("\n");
            return ToolResult::success(format!("[Dry run]\n{}", summary));
        }

        // Apply each operation
        let mut results = Vec::new();
        for op in &ops {
            match Self::apply_operation(op, &context.project_root) {
                Ok(result) => results.push(result),
                Err(e) => return ToolResult::error(e.to_string()),
            }
        }

        ToolResult::success(results.join("\n"))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_add_file() {
        let patch = r#"*** Begin Patch
*** Add File: src/new.rs
+pub fn hello() {
+    println!("Hello!");
+}
*** End Patch"#;

        let ops = ApplyPatchTool::parse_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);

        if let PatchOp::AddFile { path, content } = &ops[0] {
            assert_eq!(path, "src/new.rs");
            assert!(content.contains("pub fn hello()"));
        } else {
            panic!("Expected AddFile operation");
        }
    }

    #[test]
    fn test_parse_update_file() {
        let patch = r#"*** Begin Patch
*** Update File: src/main.rs
@@ fn main
 fn main() {
-    old();
+    new();
 }
*** End Patch"#;

        let ops = ApplyPatchTool::parse_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);

        if let PatchOp::UpdateFile { path, chunks, .. } = &ops[0] {
            assert_eq!(path, "src/main.rs");
            assert_eq!(chunks.len(), 1);
            assert_eq!(chunks[0].context, Some("fn main".to_string()));
            assert_eq!(chunks[0].old_lines, vec!["fn main() {", "    old();", "}"]);
            assert_eq!(chunks[0].new_lines, vec!["fn main() {", "    new();", "}"]);
        } else {
            panic!("Expected UpdateFile operation");
        }
    }

    #[test]
    fn test_missing_begin_marker() {
        let patch = "*** Update File: test.rs\n@@\n+line\n*** End Patch";
        assert!(ApplyPatchTool::parse_patch(patch).is_err());
    }

    #[test]
    fn test_end_marker_is_optional() {
        let patch = "*** Begin Patch\n*** Update File: test.rs\n@@\n+line";
        assert_eq!(ApplyPatchTool::parse_patch(patch).unwrap().len(), 1);
    }

    #[test]
    fn test_fuzzy_matching() {
        let lines: Vec<String> = vec![
            "fn main() {".to_string(),
            "    old();  ".to_string(),  // trailing whitespace
            "}".to_string(),
        ];

        let pattern = vec!["    old();".to_string()];

        // Should find with rstrip matching
        let pos = ApplyPatchTool::seek_sequence(&lines, &pattern, 0, false);
        assert_eq!(pos, Some(1));
    }

    #[tokio::test]
    async fn test_apply_add_file() {
        let temp_dir = TempDir::new().unwrap();
        let context = ToolContext::new(temp_dir.path());

        let patch = r#"*** Begin Patch
*** Add File: test.txt
+Hello World
*** End Patch"#;

        let params = serde_json::json!({ "patch": patch });
        let result = ApplyPatchTool.execute(params, &context).await;

        assert!(result.success, "Failed: {:?}", result.error);
        assert!(temp_dir.path().join("test.txt").exists());
    }

    #[tokio::test]
    async fn test_apply_update_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "foo\nbar\nbaz\n").unwrap();

        let context = ToolContext::new(temp_dir.path());

        let patch = r#"*** Begin Patch
*** Update File: test.txt
@@
 foo
-bar
+BAR
 baz
*** End Patch"#;

        let params = serde_json::json!({ "patch": patch });
        let result = ApplyPatchTool.execute(params, &context).await;

        assert!(result.success, "Failed: {:?}", result.error);
        let content = fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "foo\nBAR\nbaz\n");
    }
}
