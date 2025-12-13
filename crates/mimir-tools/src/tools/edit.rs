//! Edit tool - edit source code files.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

/// Output from the edit tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct EditOutput {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub lines_changed: i32,
    pub preview: Option<String>,
}

/// Tool for editing source code.
pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit source code by symbol name or file path. When editing by symbol, replaces the entire symbol. When editing by file, can specify line range or append to end."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::optional_string(
                "symbol",
                "Symbol name to edit (e.g., 'User', 'impl User', 'UserService'). Replaces entire symbol with new content.",
            ))
            .with_param(ToolParameter::optional_string(
                "file",
                "File path to edit. Required if symbol is not provided. Example: 'src/user.rs'",
            ))
            .with_param(ToolParameter::optional_integer(
                "start_line",
                "Start line number (1-based). Used with file parameter.",
            ))
            .with_param(ToolParameter::optional_integer(
                "end_line",
                "End line number (1-based, inclusive). Used with file parameter. Defaults to start_line.",
            ))
            .with_param(ToolParameter::required_string(
                "content",
                "New source code to insert. When editing symbol, do NOT include /// comments or #[derive].",
            ))
            .with_param(ToolParameter::optional_string(
                "mode",
                "Edit mode: 'replace' (default) replaces lines, 'append' adds to end of file, 'insert' inserts before start_line.",
            ))
            .with_param(ToolParameter::optional_bool(
                "preview",
                "If true, shows diff without applying changes.",
                false,
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let symbol = params["symbol"].as_str();
        let file = params["file"].as_str();
        let start_line = params["start_line"].as_u64().map(|n| n as u32);
        let end_line = params["end_line"].as_u64().map(|n| n as u32);
        // Handle content - can be string or (erroneously) array from some models
        let content_value = &params["content"];
        let content_string: String;
        let content: &str = if let Some(c) = content_value.as_str() {
            content_string = c.to_string();
            &content_string
        } else if let Some(arr) = content_value.as_array() {
            // Some models send content as array - join it
            content_string = arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if content_string.is_empty() {
                return ToolResult::error("'content' parameter is required (got empty array)");
            }
            &content_string
        } else {
            return ToolResult::error("'content' parameter is required");
        };
        let mode = params["mode"].as_str().unwrap_or("replace");
        let preview = params["preview"].as_bool().unwrap_or(false);

        // Handle symbol-based editing first
        if let Some(sym) = symbol {
            return self.handle_symbol_edit(sym, content, preview, context).await;
        }

        // File-based editing
        let file_path = match file {
            Some(f) => f,
            None => return ToolResult::error(
                "Either 'symbol' or 'file' must be provided. Examples:\n\
                 - symbol: 'UserService' (edits entire struct/impl)\n\
                 - file: 'src/user.rs' with start_line/end_line"
            ),
        };

        // Resolve file path and get file info
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            context.project_root.join(file_path)
        };

        let file_content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file '{}': {}", file_path, e)),
        };
        let total_lines = file_content.lines().count() as u32;

        // Determine start and end lines based on mode
        let (start, end) = match mode {
            "append" => {
                // Append mode: add to end of file
                (total_lines + 1, total_lines)
            }
            "insert" => {
                // Insert mode: insert before start_line
                let line = start_line.unwrap_or(1);
                (line, line.saturating_sub(1))
            }
            "replace" | _ => {
                // Replace mode: need start_line, end_line defaults to start_line
                // If no start_line given, auto-fallback to append mode
                match start_line {
                    Some(s) => {
                        let e = end_line.unwrap_or(s);
                        (s, e)
                    }
                    None => {
                        // Auto-fallback to append mode for convenience
                        (total_lines + 1, total_lines)
                    }
                }
            }
        };

        if context.dry_run || preview {
            self.preview_edit(file_path, start, end, content, context)
        } else {
            self.apply_edit(file_path, start, end, content, context)
        }
    }
}

impl EditTool {
    async fn handle_symbol_edit(
        &self,
        symbol: &str,
        content: &str,
        preview: bool,
        context: &ToolContext,
    ) -> ToolResult {
        let client = match &context.graph_client {
            Some(c) => c,
            None => return ToolResult::error(
                "Symbol-based editing requires daemon connection.\n\
                 Use file/start_line/end_line instead, or start daemon with: mimir daemon start"
            ),
        };

        // Try exact match first, then fuzzy search
        let search_result = client.search(symbol, None, 5).await;

        match search_result {
            Ok(result) if !result.symbols.is_empty() => {
                // Find best match (prefer exact name match)
                let best_match = result.symbols.iter()
                    .find(|s| s.name == symbol || s.name.ends_with(symbol))
                    .unwrap_or(&result.symbols[0]);

                if preview || context.dry_run {
                    self.preview_edit(&best_match.file, best_match.start_line, best_match.end_line, content, context)
                } else {
                    self.apply_edit(&best_match.file, best_match.start_line, best_match.end_line, content, context)
                }
            }
            Ok(result) => {
                // Symbol not found - provide helpful suggestions
                let suggestion = if result.total > 0 {
                    format!("Did you mean one of these? {:?}",
                        result.symbols.iter().map(|s| &s.name).collect::<Vec<_>>())
                } else {
                    "Try using 'search' tool first to find the symbol.".to_string()
                };

                ToolResult::error(format!(
                    "Symbol '{}' not found. {}\n\
                     Alternative: Use file/start_line/end_line parameters.",
                    symbol, suggestion
                ))
            }
            Err(e) => ToolResult::error(format!(
                "Failed to search for symbol '{}': {}\n\
                 Try using file/start_line/end_line parameters instead.",
                symbol, e
            )),
        }
    }

    fn preview_edit(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        new_content: &str,
        context: &ToolContext,
    ) -> ToolResult {
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            context.project_root.join(file_path)
        };

        let original = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        let lines: Vec<&str> = original.lines().collect();
        let start = (start_line as usize).saturating_sub(1);
        let end = (end_line as usize).min(lines.len());

        if start >= lines.len() {
            return ToolResult::error(format!(
                "Start line {} is beyond file length {}",
                start_line,
                lines.len()
            ));
        }

        // Build preview diff
        let old_lines: Vec<&str> = lines[start..end].to_vec();
        let new_lines: Vec<&str> = new_content.lines().collect();

        let mut diff = String::new();
        diff.push_str(&format!("--- {}\n", file_path));
        diff.push_str(&format!("+++ {}\n", file_path));
        diff.push_str(&format!("@@ -{},{} +{},{} @@\n", start_line, old_lines.len(), start_line, new_lines.len()));

        for line in &old_lines {
            diff.push_str(&format!("-{}\n", line));
        }
        for line in &new_lines {
            diff.push_str(&format!("+{}\n", line));
        }

        let output = EditOutput {
            file: file_path.to_string(),
            start_line,
            end_line,
            lines_changed: new_lines.len() as i32 - old_lines.len() as i32,
            preview: Some(diff),
        };

        ToolResult::success(output)
    }

    fn apply_edit(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        new_content: &str,
        context: &ToolContext,
    ) -> ToolResult {
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            context.project_root.join(file_path)
        };

        let original = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        let lines: Vec<&str> = original.lines().collect();
        let start = (start_line as usize).saturating_sub(1);
        let end = (end_line as usize).min(lines.len());

        if start >= lines.len() {
            return ToolResult::error(format!(
                "Start line {} is beyond file length {}",
                start_line,
                lines.len()
            ));
        }

        // Build new content
        let mut result = Vec::new();
        result.extend_from_slice(&lines[..start]);
        for line in new_content.lines() {
            result.push(line);
        }
        result.extend_from_slice(&lines[end..]);

        let new_file_content = result.join("\n");

        // Write back
        if let Err(e) = fs::write(&path, &new_file_content) {
            return ToolResult::error(format!("Failed to write file: {}", e));
        }

        let old_len = end - start;
        let new_len = new_content.lines().count();

        let output = EditOutput {
            file: file_path.to_string(),
            start_line,
            end_line,
            lines_changed: new_len as i32 - old_len as i32,
            preview: None,
        };

        ToolResult::success(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_edit_preview() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.rs");
        fs::write(&file_path, "line 1\nline 2\nline 3").unwrap();

        let tool = EditTool;
        let context = ToolContext::new(temp.path());
        let params = serde_json::json!({
            "file": "test.rs",
            "start_line": 2,
            "end_line": 2,
            "content": "new line 2",
            "preview": true
        });

        let result = tool.execute(params, &context).await;
        assert!(result.success);

        let output: EditOutput = serde_json::from_value(result.output).unwrap();
        assert!(output.preview.is_some());
        assert!(output.preview.unwrap().contains("-line 2"));
    }

    #[tokio::test]
    async fn test_edit_apply() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.rs");
        fs::write(&file_path, "line 1\nline 2\nline 3").unwrap();

        let tool = EditTool;
        let context = ToolContext::new(temp.path());
        let params = serde_json::json!({
            "file": "test.rs",
            "start_line": 2,
            "end_line": 2,
            "content": "new line 2",
            "preview": false
        });

        let result = tool.execute(params, &context).await;
        assert!(result.success);

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("new line 2"));
        // Original "line 2" should be replaced by "new line 2"
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[1], "new line 2");
    }
}
