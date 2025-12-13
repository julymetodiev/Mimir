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
        "Edit source code by replacing lines or entire symbols. Supports editing by line numbers or by symbol name. Always use preview mode first to verify changes."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::optional_string(
                "symbol",
                "Symbol name to edit. If provided, replaces the entire symbol with new content.",
            ))
            .with_param(ToolParameter::optional_string(
                "file",
                "File path to edit. Required if symbol is not provided.",
            ))
            .with_param(ToolParameter::optional_integer(
                "start_line",
                "Start line number (1-based). Required for line-based editing.",
            ))
            .with_param(ToolParameter::optional_integer(
                "end_line",
                "End line number (1-based, inclusive). Required for line-based editing.",
            ))
            .with_param(ToolParameter::required_string(
                "content",
                "New content to replace the specified lines or symbol.",
            ))
            .with_param(ToolParameter::optional_bool(
                "preview",
                "If true, only shows what would change without applying. Always preview first!",
                true,
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let symbol = params["symbol"].as_str();
        let file = params["file"].as_str();
        let start_line = params["start_line"].as_u64().map(|n| n as u32);
        let end_line = params["end_line"].as_u64().map(|n| n as u32);
        let content = match params["content"].as_str() {
            Some(c) => c,
            None => return ToolResult::error("'content' parameter is required"),
        };
        let preview = params["preview"].as_bool().unwrap_or(true);

        if context.dry_run || preview {
            // Preview mode - just show what would happen
            if symbol.is_some() {
                return ToolResult::error("Symbol-based edit preview requires daemon integration (TODO)");
            }

            if let Some(file_path) = file {
                let (start, end) = match (start_line, end_line) {
                    (Some(s), Some(e)) => (s, e),
                    _ => return ToolResult::error("'start_line' and 'end_line' required for file editing"),
                };
                return self.preview_edit(file_path, start, end, content, context);
            }

            return ToolResult::error("Either 'symbol' or 'file' must be provided");
        }

        // Actual edit
        if symbol.is_some() {
            ToolResult::error("Symbol-based editing requires daemon integration (TODO)")
        } else if let Some(file_path) = file {
            let (start, end) = match (start_line, end_line) {
                (Some(s), Some(e)) => (s, e),
                _ => return ToolResult::error("'start_line' and 'end_line' required for file editing"),
            };
            self.apply_edit(file_path, start, end, content, context)
        } else {
            ToolResult::error("Either 'symbol' or 'file' must be provided")
        }
    }
}

impl EditTool {
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
