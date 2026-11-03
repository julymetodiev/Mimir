//! Edit tool - string replacement approach (like Qwen Code).
//!
//! Uses old_string/new_string for precise text replacement.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde_json::Value;
use std::fs;

/// Edit tool - replaces text within a file using exact string matching.
pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        r#"Replaces text within a file. By default, replaces a single occurrence. Set `replace_all` to true when you intend to modify every instance of `old_string`.

This tool requires providing significant context around the change to ensure precise targeting. Always use the `read` tool to examine the file's current content before attempting a text replacement.

PARAMETERS:
- `file_path`: The path to the file to modify (relative to project root)
- `old_string`: The exact literal text to replace (including all whitespace, indentation, newlines)
- `new_string`: The exact literal text to replace it with
- `replace_all`: Replace all occurrences (default: false)

RULES:
1. `old_string` MUST be the EXACT text from the file (copy-paste from read output)
2. Include at least 3 lines of context BEFORE and AFTER the target text
3. `old_string` must uniquely identify ONE location (unless using replace_all)
4. To CREATE a new file: use empty `old_string` with the full content in `new_string`
5. NEVER escape the strings - use exact literal text

EXAMPLE - Modify existing code:
{
  "file_path": "src/user.rs",
  "old_string": "pub struct User {\n    id: u32,\n    name: String,\n}",
  "new_string": "pub struct User {\n    id: u32,\n    name: String,\n    role: Role,\n}"
}

EXAMPLE - Create new file:
{
  "file_path": "src/role.rs",
  "old_string": "",
  "new_string": "pub enum Role {\n    Admin,\n    User,\n    Guest,\n}"
}

EXAMPLE - Replace all occurrences:
{
  "file_path": "src/main.rs",
  "old_string": "old_function",
  "new_string": "new_function",
  "replace_all": true
}"#
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new("edit", self.description())
            .with_param(ToolParameter::required_string(
                "file_path",
                "Path to the file to modify (relative to project root)",
            ))
            .with_param(ToolParameter::required_string(
                "old_string",
                "The exact literal text to replace. Use empty string to create a new file.",
            ))
            .with_param(ToolParameter::required_string(
                "new_string",
                "The exact literal text to replace old_string with",
            ))
            .with_param(ToolParameter::optional_bool(
                "replace_all",
                "Replace all occurrences instead of requiring unique match (default: false)",
                false,
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing 'file_path' parameter"),
        };

        let old_string = match params.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing 'old_string' parameter"),
        };

        let new_string = match params.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing 'new_string' parameter"),
        };

        let replace_all = params
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Normalize line endings
        let old_string = old_string.replace("\r\n", "\n");
        let new_string = new_string.replace("\r\n", "\n");

        let full_path = context.project_root.join(file_path);
        let file_exists = full_path.exists();

        // Handle file creation (empty old_string)
        if old_string.is_empty() {
            if file_exists {
                return ToolResult::error(
                    "File already exists. Use a non-empty old_string to edit existing files, or delete the file first.",
                );
            }

            if context.dry_run {
                return ToolResult::success(format!("[Dry run] Would create: {}", file_path));
            }

            // Create parent directories if needed
            if let Some(parent) = full_path.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                return ToolResult::error(format!("Failed to create directory: {}", e));
            }

            // Write new file
            match fs::write(&full_path, &new_string) {
                Ok(_) => {
                    return ToolResult::success(format!(
                        "Created new file: {} ({} bytes)",
                        file_path,
                        new_string.len()
                    ));
                }
                Err(e) => return ToolResult::error(format!("Failed to create file: {}", e)),
            }
        }

        // Read existing file
        if !file_exists {
            return ToolResult::error(format!(
                "File not found: {}. Use empty old_string to create a new file.",
                file_path
            ));
        }

        let current_content = match fs::read_to_string(&full_path) {
            Ok(content) => content.replace("\r\n", "\n"),
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        // Count occurrences
        let occurrences = current_content.matches(&old_string).count();

        if occurrences == 0 {
            // Try to give helpful error
            let preview = if old_string.len() > 100 {
                format!("{}...", &old_string[..100])
            } else {
                old_string.clone()
            };
            return ToolResult::error(format!(
                "Could not find the specified text in {}.\n\
                 Searched for:\n{}\n\n\
                 Hint: Use the `read` tool to verify the current file content. \
                 Make sure old_string matches exactly (including whitespace and indentation).",
                file_path, preview
            ));
        }

        if occurrences > 1 && !replace_all {
            return ToolResult::error(format!(
                "Found {} occurrences of the text in {}. \n\
                 Either:\n\
                 1. Add more context to old_string to uniquely identify one location, or\n\
                 2. Set replace_all: true to replace all occurrences.",
                occurrences, file_path
            ));
        }

        // Check if change would actually modify the file
        if old_string == new_string {
            return ToolResult::error(
                "No changes to apply. The old_string and new_string are identical.",
            );
        }

        // Apply the replacement
        let new_content = if replace_all {
            current_content.replace(&old_string, &new_string)
        } else {
            current_content.replacen(&old_string, &new_string, 1)
        };

        if current_content == new_content {
            return ToolResult::error(
                "No changes resulted from the replacement. The file content is unchanged.",
            );
        }

        if context.dry_run {
            return ToolResult::success(format!(
                "[Dry run] Would modify: {} ({} occurrence(s))",
                file_path, occurrences
            ));
        }

        // Write the modified content
        match fs::write(&full_path, &new_content) {
            Ok(_) => {
                let msg = if replace_all && occurrences > 1 {
                    format!("Modified {} ({} replacements)", file_path, occurrences)
                } else {
                    format!("Modified {}", file_path)
                };
                ToolResult::success(msg)
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
    async fn test_create_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "file_path": "new_file.rs",
            "old_string": "",
            "new_string": "pub fn hello() {}\n"
        });

        let result = EditTool.execute(params, &context).await;
        assert!(result.success, "Failed: {:?}", result.error);

        let content = fs::read_to_string(temp_dir.path().join("new_file.rs")).unwrap();
        assert_eq!(content, "pub fn hello() {}\n");
    }

    #[tokio::test]
    async fn test_modify_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, "fn old() {}\n").unwrap();

        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "file_path": "test.rs",
            "old_string": "fn old() {}",
            "new_string": "fn new() {}"
        });

        let result = EditTool.execute(params, &context).await;
        assert!(result.success, "Failed: {:?}", result.error);

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "fn new() {}\n");
    }

    #[tokio::test]
    async fn test_replace_all() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, "foo foo foo\n").unwrap();

        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "file_path": "test.rs",
            "old_string": "foo",
            "new_string": "bar",
            "replace_all": true
        });

        let result = EditTool.execute(params, &context).await;
        assert!(result.success, "Failed: {:?}", result.error);

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "bar bar bar\n");
    }

    #[tokio::test]
    async fn test_multiple_occurrences_error() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, "foo foo foo\n").unwrap();

        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "file_path": "test.rs",
            "old_string": "foo",
            "new_string": "bar"
        });

        let result = EditTool.execute(params, &context).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("3 occurrences"));
    }

    #[tokio::test]
    async fn test_not_found_error() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, "hello world\n").unwrap();

        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "file_path": "test.rs",
            "old_string": "not found",
            "new_string": "bar"
        });

        let result = EditTool.execute(params, &context).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Could not find"));
    }

    #[tokio::test]
    async fn test_file_not_exists() {
        let temp_dir = TempDir::new().unwrap();
        let context = ToolContext::new(temp_dir.path());

        let params = serde_json::json!({
            "file_path": "nonexistent.rs",
            "old_string": "foo",
            "new_string": "bar"
        });

        let result = EditTool.execute(params, &context).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("File not found"));
    }
}
