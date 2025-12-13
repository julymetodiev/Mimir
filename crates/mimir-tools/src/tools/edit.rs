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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

/// Tool for editing source code.
pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit source code. PREFER 'Symbol Mode' or 'Text Replace Mode'. Avoid line numbers if possible.\n\
         MODES:\n\
         1. Symbol Mode: { \"symbol\": \"User\", \"content\": \"...\" }\n\
         2. Text Replace: { \"file\": \"src/main.rs\", \"old_content\": \"let x = 1;\", \"content\": \"let x = 2;\" }\n\
         3. Line Mode (Fallback): { \"file\": \"...\", \"start_line\": 10, \"content\": \"...\" }"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::optional_string(
                "symbol",
                "Symbol name to edit (e.g., 'User', 'impl User'). Replaces the ENTIRE symbol definition.",
            ))
            .with_param(ToolParameter::optional_string(
                "file",
                "File path. Required for Text Replace or Line Mode.",
            ))
            .with_param(ToolParameter::optional_string(
                "old_content",
                "EXACT text to find and replace. If provided, ignores line numbers. Best for specific changes.",
            ))
            .with_param(ToolParameter::optional_integer(
                "start_line",
                "Start line (1-based). Use ONLY if symbol/old_content are not suitable. Risky due to line shifts.",
            ))
            .with_param(ToolParameter::optional_integer(
                "end_line",
                "End line (1-based). Defaults to start_line.",
            ))
            .with_param(ToolParameter::required_string(
                "content",
                "New content. Must be a STRING. To delete, provide empty string \"\".",
            ))
            .with_param(ToolParameter::optional_string(
                "mode",
                "Edit mode: 'replace' (default), 'append' (add to end of file).",
            ))
            .with_param(ToolParameter::optional_bool(
                "preview",
                "Show diff without applying. Default: false.",
                false,
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let symbol = params["symbol"].as_str();
        let file = params["file"].as_str();
        let old_content = params["old_content"].as_str();
        let start_line = params["start_line"].as_u64().map(|n| n as u32);
        let end_line = params["end_line"].as_u64().map(|n| n as u32);

        // Handle content extraction (handling array edge case)
        let content_value = &params["content"];
        let content_string: String;
        let content: &str = if let Some(c) = content_value.as_str() {
            content_string = c.to_string();
            &content_string
        } else if let Some(arr) = content_value.as_array() {
            content_string = arr
                .iter()
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

        // 1. Symbol-based editing (Primary)
        if let Some(sym) = symbol {
            return self
                .handle_symbol_edit(sym, content, preview, context)
                .await;
        }

        let file_path = match file {
            Some(f) => f,
            None => {
                return ToolResult::error(
                    "One of 'symbol', 'file' (with old_content), or 'file' (with start_line) is required.",
                );
            }
        };

        // 2. Text-based Search & Replace (Secondary - Robust)
        if let Some(old) = old_content {
            return self
                .handle_text_replace(file_path, old, content, preview, context)
                .await;
        }

        // 3. Line-based editing (Fallback - Brittle)
        // ... existing line logic ...
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            context.project_root.join(file_path)
        };

        let file_content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(format!("Failed to read file '{}': {}", file_path, e));
            }
        };
        let total_lines = file_content.lines().count() as u32;

        let (start, end) = match mode {
            "append" => (total_lines + 1, total_lines),
            "insert" => {
                let line = start_line.unwrap_or(1);
                (line, line.saturating_sub(1))
            }
            "replace" | _ => {
                match start_line {
                    Some(s) => (s, end_line.unwrap_or(s)),
                    None => {
                        // If neither symbol, old_content, nor start_line is provided, assume append if mode is not explicit,
                        // or error out. But since default mode is "replace", we should error if no target specified.
                        if mode == "append" {
                            (total_lines + 1, total_lines)
                        } else {
                            return ToolResult::error(
                                "Missing 'start_line' for file replacement. Use 'append' mode, 'old_content', or 'symbol'.",
                            );
                        }
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
            None => {
                return ToolResult::error(
                    "Symbol-based editing requires daemon connection.\n\
                 Use file/start_line/end_line instead, or start daemon with: mimir daemon start",
                );
            }
        };

        // Try exact match first, then fuzzy search. Increase limit to ensure we find it.
        let search_result = client.search(symbol, None, 50).await;

        match search_result {
            Ok(result) if !result.symbols.is_empty() => {
                // Find best match (prefer exact name match)
                let best_match = result
                    .symbols
                    .iter()
                    .find(|s| s.name == symbol || s.name.ends_with(symbol));

                match best_match {
                    Some(m) => {
                        if preview || context.dry_run {
                            self.preview_edit(&m.file, m.start_line, m.end_line, content, context)
                        } else {
                            self.apply_edit(&m.file, m.start_line, m.end_line, content, context)
                        }
                    }
                    None => ToolResult::error(format!(
                        "Symbol '{}' not found exactly. Did you mean one of these? {:?}\n\
                             Please use the exact symbol name.",
                        symbol,
                        result
                            .symbols
                            .iter()
                            .map(|s| &s.name)
                            .take(5)
                            .collect::<Vec<_>>()
                    )),
                }
            }
            Ok(result) => {
                // Symbol not found - provide helpful suggestions
                let suggestion = if result.total > 0 {
                    format!(
                        "Did you mean one of these? {:?}",
                        result.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
                    )
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

    fn check_syntax_balance(content: &str) -> Option<String> {
        let mut brace_count = 0;
        for c in content.chars() {
            match c {
                '{' => brace_count += 1,
                '}' => brace_count -= 1,
                _ => {}
            }
        }
        if brace_count != 0 {
            Some(format!("Unbalanced braces detected (net count: {}). This edit might break the file structure.", brace_count))
        } else {
            None
        }
    }

    async fn handle_text_replace(
        &self,
        file_path: &str,
        old_content: &str,
        new_content: &str,
        preview: bool,
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

        // Normalize line endings to LF for comparison if needed, but for now stick to exact match
        // Count occurrences
        let matches: Vec<_> = original.match_indices(old_content).collect();

        if matches.is_empty() {
            // Try to be helpful: check if it matches ignoring whitespace
            let clean_old = old_content.split_whitespace().collect::<Vec<_>>().join(" ");
            let clean_orig = original.split_whitespace().collect::<Vec<_>>().join(" ");

            if clean_orig.contains(&clean_old) {
                return ToolResult::error(
                    "Exact match not found, but similar content exists (whitespace differences). \n\
                      Please read the file again and provide the EXACT content including indentation and newlines.",
                );
            }

            return ToolResult::error(format!(
                "Content to replace not found in '{}'. Please make sure 'old_content' matches exactly.",
                file_path
            ));
        }

        if matches.len() > 1 {
            return ToolResult::error(format!(
                "Content found {} times in '{}'. Please provide more context in 'old_content' to make it unique.",
                matches.len(),
                file_path
            ));
        }

        let (start_byte, _) = matches[0];

        // Calculate line numbers for reporting
        let pre_match = &original[..start_byte];
        let start_line = pre_match.lines().count() as u32 + 1;
        let old_lines_count = old_content.lines().count() as u32;
        let end_line = start_line + old_lines_count.saturating_sub(1);

        if preview || context.dry_run {
            // Generate diff
            // let lines: Vec<&str> = original.lines().collect();
            // Just reuse preview_edit logic if it maps to full lines, otherwise manual diff
            // For simplicity, let's just show a unified diff of the replacement
            let mut diff = String::new();
            diff.push_str(&format!("--- {}\n", file_path));
            diff.push_str(&format!("+++ {}\n", file_path));
            diff.push_str(&format!(
                "@@ -{},{} +{},? @@\n",
                start_line, old_lines_count, start_line
            ));

            for line in old_content.lines() {
                diff.push_str(&format!("-{}\n", line));
            }
            for line in new_content.lines() {
                diff.push_str(&format!("+{}\n", line));
            }

            return ToolResult::success(EditOutput {
                file: file_path.to_string(),
                start_line,
                end_line,
                lines_changed: 0, // Approximate
                preview: Some(diff),
                warnings: None,
            });
        }

        // Apply replacement
        let new_file_content = original.replace(old_content, new_content);

        // Check for basic syntax errors (unbalanced braces)
        let warnings = Self::check_syntax_balance(&new_file_content)
            .map(|w| vec![w]);

        if let Err(e) = fs::write(&path, &new_file_content) {
            return ToolResult::error(format!("Failed to write file: {}", e));
        }

        let new_lines_count = new_content.lines().count() as i32;
        let old_lines_cnt = old_content.lines().count() as i32;

        ToolResult::success(EditOutput {
            file: file_path.to_string(),
            start_line,
            end_line,
            lines_changed: new_lines_count - old_lines_cnt,
            preview: None,
            warnings,
        })
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
                "Start line {} is beyond file length {}. \n\
                 - To ADD new code to the end: Use mode='append' or the 'append' tool.\n\
                 - To EDIT existing code: Re-read the file to check line numbers, or use symbol-based editing (PREFERRED).",
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
        diff.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start_line,
            old_lines.len(),
            start_line,
            new_lines.len()
        ));

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
            warnings: None,
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
                "Start line {} is beyond file length {}. \n\
                 - To ADD new code to the end: Use mode='append' or the 'append' tool.\n\
                 - To EDIT existing code: Re-read the file to check line numbers, or use symbol-based editing (PREFERRED).",
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

        // Check for basic syntax errors (unbalanced braces)
        let warnings = Self::check_syntax_balance(&new_file_content)
            .map(|w| vec![w]);

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
            warnings,
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
