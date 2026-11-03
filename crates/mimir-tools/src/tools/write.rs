//! Write tool for creating or overwriting files.
//!
//! Simpler alternative to apply_patch when complete file replacement is needed.

use crate::{Tool, ToolContext, ToolResult, ToolSchema, ToolParameter};
use async_trait::async_trait;
use serde_json::Value;
use std::fs;

/// Write tool - create or overwrite entire files
pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        r#"Create a new file or completely overwrite an existing file.

USE THIS TOOL WHEN:
- Creating a new file from scratch
- Rewriting an entire file (not just modifying parts)
- The file is small enough to rewrite completely

DO NOT USE when:
- Making small edits to a large file (use apply_patch instead)
- You only need to change a few lines

PARAMETERS:
- path: File path relative to project root
- content: Complete file content

EXAMPLE:
{
  "path": "src/models/role.rs",
  "content": "pub enum Role {\n    Admin,\n    User,\n    Guest,\n}"
}"#
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new("write", self.description())
            .with_param(ToolParameter::required_string(
                "path",
                "File path relative to project root"
            ))
            .with_param(ToolParameter::required_string(
                "content",
                "Complete file content to write"
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let path = match params.get("path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing 'path' parameter"),
        };

        let content = match params.get("content").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing 'content' parameter"),
        };

        let full_path = context.project_root.join(path);

        if context.dry_run {
            let action = if full_path.exists() { "overwrite" } else { "create" };
            return ToolResult::success(format!(
                "[Dry run] Would {} '{}' ({} bytes)",
                action, path, content.len()
            ));
        }

        // Create parent directories if needed
        if let Some(parent) = full_path.parent()
            && let Err(e) = fs::create_dir_all(parent) {
                return ToolResult::error(format!("Failed to create directory: {}", e));
            }

        // Check if file exists for reporting
        let existed = full_path.exists();

        // Write the file
        match fs::write(&full_path, content) {
            Ok(_) => {
                let action = if existed { "M" } else { "A" };
                ToolResult::success(format!("{} {} ({} bytes)", action, path, content.len()))
            }
            Err(e) => ToolResult::error(format!("Failed to write file: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "path": "test.txt",
            "content": "Hello World"
        });

        let result = WriteTool.execute(params, &context).await;
        assert!(result.success, "Failed: {:?}", result.error);
        assert!(temp_dir.path().join("test.txt").exists());

        let content = fs::read_to_string(temp_dir.path().join("test.txt")).unwrap();
        assert_eq!(content, "Hello World");
    }

    #[tokio::test]
    async fn test_write_overwrite() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("existing.txt");
        fs::write(&file_path, "Old content").unwrap();

        let context = ToolContext::new(temp_dir.path());
        let params = serde_json::json!({
            "path": "existing.txt",
            "content": "New content"
        });

        let result = WriteTool.execute(params, &context).await;
        assert!(result.success);
        assert!(result.output.as_str().unwrap().starts_with("M "));

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "New content");
    }

    #[tokio::test]
    async fn test_write_creates_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "path": "deep/nested/dir/file.txt",
            "content": "Hello"
        });

        let result = WriteTool.execute(params, &context).await;
        assert!(result.success);
        assert!(temp_dir.path().join("deep/nested/dir/file.txt").exists());
    }
}
