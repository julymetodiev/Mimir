//! Read tool - read source code by symbol name or file path.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

/// Output from the read tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadOutput {
    pub symbol: Option<String>,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub lines: Vec<LineInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LineInfo {
    pub number: u32,
    pub content: String,
}

/// Tool for reading source code.
pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read source code by symbol name or file path. When reading by symbol, returns only the symbol's code. When reading by file, can specify line range."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::optional_string(
                "symbol",
                "Symbol name to read (e.g., function or struct name). If provided, file is optional.",
            ))
            .with_param(ToolParameter::optional_string(
                "file",
                "File path to read. Required if symbol is not provided.",
            ))
            .with_param(ToolParameter::optional_integer(
                "start_line",
                "Start line number (1-based). Only used with file, not symbol.",
            ))
            .with_param(ToolParameter::optional_integer(
                "end_line",
                "End line number (1-based, inclusive). Only used with file, not symbol.",
            ))
            .with_param(ToolParameter::optional_integer(
                "context_before",
                "Lines of context before the symbol (only with symbol).",
            ))
            .with_param(ToolParameter::optional_integer(
                "context_after",
                "Lines of context after the symbol (only with symbol).",
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let symbol = params["symbol"].as_str();
        let file = params["file"].as_str();
        let start_line = params["start_line"].as_u64().map(|n| n as u32);
        let end_line = params["end_line"].as_u64().map(|n| n as u32);
        let context_before = params["context_before"].as_u64().unwrap_or(0) as usize;
        let context_after = params["context_after"].as_u64().unwrap_or(0) as usize;

        if let Some(sym_name) = symbol {
            // Read by symbol - use graph client if available
            if let Some(client) = &context.graph_client {
                match client.read_symbol(sym_name).await {
                    Ok(Some((info, code))) => {
                        // If context lines requested, read them from file
                        let (full_content, lines) = if context_before > 0 || context_after > 0 {
                            self.read_with_context(
                                &info.file,
                                info.start_line,
                                info.end_line,
                                context_before,
                                context_after,
                                context,
                            )
                        } else {
                            let lines: Vec<LineInfo> = code.lines()
                                .enumerate()
                                .map(|(i, line)| LineInfo {
                                    number: info.start_line + i as u32,
                                    content: line.to_string(),
                                })
                                .collect();
                            (code, lines)
                        };

                        let output = ReadOutput {
                            symbol: Some(sym_name.to_string()),
                            file: info.file,
                            start_line: info.start_line,
                            end_line: info.end_line,
                            content: full_content,
                            lines,
                        };
                        return ToolResult::success(output);
                    }
                    Ok(None) => {
                        return ToolResult::error(format!("Symbol '{}' not found", sym_name));
                    }
                    Err(e) => {
                        return ToolResult::error(format!("Failed to read symbol: {}", e));
                    }
                }
            } else {
                return ToolResult::error("Symbol-based read requires daemon. Start with 'mimir daemon start'");
            }
        } else if let Some(file_path) = file {
            // Read by file
            self.read_file(file_path, start_line, end_line, context)
        } else {
            ToolResult::error("Either 'symbol' or 'file' must be provided")
        }
    }
}

impl ReadTool {
    fn read_file(
        &self,
        file_path: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
        context: &ToolContext,
    ) -> ToolResult {
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            context.project_root.join(file_path)
        };

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();

        let start = start_line.map(|n| (n as usize).saturating_sub(1)).unwrap_or(0);
        let end = end_line.map(|n| (n as usize).min(total_lines)).unwrap_or(total_lines);

        if start >= total_lines {
            return ToolResult::error(format!(
                "Start line {} is beyond file length {}",
                start + 1,
                total_lines
            ));
        }

        let lines: Vec<LineInfo> = all_lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| LineInfo {
                number: (start + i + 1) as u32,
                content: line.to_string(),
            })
            .collect();

        let selected_content = all_lines[start..end].join("\n");

        let output = ReadOutput {
            symbol: None,
            file: file_path.to_string(),
            start_line: (start + 1) as u32,
            end_line: end as u32,
            content: selected_content,
            lines,
        };

        ToolResult::success(output)
    }

    fn read_with_context(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        context_before: usize,
        context_after: usize,
        ctx: &ToolContext,
    ) -> (String, Vec<LineInfo>) {
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            ctx.project_root.join(file_path)
        };

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return (String::new(), vec![]),
        };

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();

        let actual_start = (start_line as usize).saturating_sub(1).saturating_sub(context_before);
        let actual_end = ((end_line as usize) + context_after).min(total_lines);

        let lines: Vec<LineInfo> = all_lines[actual_start..actual_end]
            .iter()
            .enumerate()
            .map(|(i, line)| LineInfo {
                number: (actual_start + i + 1) as u32,
                content: line.to_string(),
            })
            .collect();

        let selected_content = all_lines[actual_start..actual_end].join("\n");
        (selected_content, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.rs");
        fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5").unwrap();

        let tool = ReadTool;
        let context = ToolContext::new(temp.path());
        let params = serde_json::json!({
            "file": "test.rs"
        });

        let result = tool.execute(params, &context).await;
        assert!(result.success);

        let output: ReadOutput = serde_json::from_value(result.output).unwrap();
        assert_eq!(output.lines.len(), 5);
        assert_eq!(output.start_line, 1);
        assert_eq!(output.end_line, 5);
    }

    #[tokio::test]
    async fn test_read_file_range() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.rs");
        fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5").unwrap();

        let tool = ReadTool;
        let context = ToolContext::new(temp.path());
        let params = serde_json::json!({
            "file": "test.rs",
            "start_line": 2,
            "end_line": 4
        });

        let result = tool.execute(params, &context).await;
        assert!(result.success);

        let output: ReadOutput = serde_json::from_value(result.output).unwrap();
        assert_eq!(output.lines.len(), 3);
        assert_eq!(output.lines[0].number, 2);
        assert_eq!(output.lines[0].content, "line 2");
    }
}
