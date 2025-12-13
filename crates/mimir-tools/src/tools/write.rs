//! Write tool - overwrite or create file content.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde_json::Value;
use std::fs;

/// Tool for writing files.
pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Overwrite a file with new content. Creates the file if it doesn't exist. USE CAREFULLY - this replaces the entire file content."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::required_string(
                "file",
                "Path to the file to write.",
            ))
            .with_param(ToolParameter::required_string(
                "content",
                "The full content to write to the file.",
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let file_path = match params["file"].as_str() {
            Some(f) => f,
            None => return ToolResult::error("Parameter 'file' is required"),
        };

        // Handle content similar to EditTool to be robust against array inputs
        let content_value = &params["content"];
        let content_string: String;
        let content: &str = if let Some(c) = content_value.as_str() {
            content_string = c.to_string();
            &content_string
        } else if let Some(arr) = content_value.as_array() {
            content_string = arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            &content_string
        } else {
            return ToolResult::error("Parameter 'content' is required and must be a string");
        };

        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            context.project_root.join(file_path)
        };

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return ToolResult::error(format!("Failed to create directory: {}", e));
            }
        }

        if let Err(e) = fs::write(&path, content) {
            return ToolResult::error(format!("Failed to write file: {}", e));
        }

        ToolResult::success(format!("Successfully wrote {} chars to {}", content.len(), file_path))
    }
}
