//! Append tool - add new code to a file.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::fs;

#[derive(Debug, Serialize)]
struct AppendOutput {
    file: String,
    lines_added: usize,
}

/// Tool for appending new code to files.
pub struct AppendTool;

#[async_trait]
impl Tool for AppendTool {
    fn name(&self) -> &str {
        "append"
    }

    fn description(&self) -> &str {
        "Add new code to the end of a file. Use for NEW symbols not in search results."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::required_string(
                "file",
                "File path, e.g. 'src/user.rs'",
            ))
            .with_param(ToolParameter::required_string(
                "content",
                "New code to add at end of file",
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let file = match params["file"].as_str() {
            Some(f) => f,
            None => return ToolResult::error("'file' is required"),
        };
        let content = match params["content"].as_str() {
            Some(c) => c,
            None => return ToolResult::error("'content' is required"),
        };

        let path = if std::path::Path::new(file).is_absolute() {
            std::path::PathBuf::from(file)
        } else {
            context.project_root.join(file)
        };

        // Read existing content
        let mut existing = fs::read_to_string(&path).unwrap_or_default();

        // Ensure newline before appending
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push('\n');
        existing.push_str(content);
        if !content.ends_with('\n') {
            existing.push('\n');
        }

        // Write back
        if let Err(e) = fs::write(&path, &existing) {
            return ToolResult::error(format!("Failed to write: {}", e));
        }

        ToolResult::success(AppendOutput {
            file: file.to_string(),
            lines_added: content.lines().count(),
        })
    }
}
