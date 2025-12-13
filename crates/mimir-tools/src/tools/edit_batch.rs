//! Batch edit tool - edit multiple symbols in one call.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

/// Single symbol edit in a batch.
#[derive(Debug, Deserialize)]
struct SymbolEdit {
    symbol: String,
    content: String,
}

/// Output from batch edit.
#[derive(Debug, Serialize)]
struct BatchEditOutput {
    success: bool,
    edits: Vec<EditResult>,
    message: String,
}

/// Result of a single edit.
#[derive(Debug, Serialize)]
struct EditResult {
    symbol: String,
    file: String,
    start_line: u32,
    end_line: u32,
    lines_changed: i32,
    error: Option<String>,
}

/// Tool for batch editing multiple symbols.
pub struct EditBatchTool;

#[async_trait]
impl Tool for EditBatchTool {
    fn name(&self) -> &str {
        "edit_batch"
    }

    fn description(&self) -> &str {
        "Edit multiple symbols in one atomic operation. More efficient than multiple single edits. \
         Each edit replaces the entire symbol (struct, impl, function) with new content."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::required_array(
                "edits",
                "Array of symbol edits. Each edit has 'symbol' (name) and 'content' (new code).",
            ))
            .with_param(ToolParameter::optional_bool(
                "preview",
                "If true, shows diff without applying changes.",
                false,
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let client = match &context.graph_client {
            Some(c) => c,
            None => return ToolResult::error(
                "Batch editing requires daemon connection. Start with: mimir daemon start"
            ),
        };

        let edits: Vec<SymbolEdit> = match serde_json::from_value(params["edits"].clone()) {
            Ok(e) => e,
            Err(e) => return ToolResult::error(format!(
                "Invalid edits format: {}. Expected: [{{\"symbol\": \"Name\", \"content\": \"code\"}}]",
                e
            )),
        };

        if edits.is_empty() {
            return ToolResult::error("No edits provided");
        }

        let preview = params["preview"].as_bool().unwrap_or(false);

        // Resolve all symbols first to check for errors before making changes
        let mut resolved: Vec<(SymbolEdit, String, u32, u32)> = Vec::new();

        for edit in edits {
            let result = client.search(&edit.symbol, None, 10).await;

            match result {
                Ok(r) if !r.symbols.is_empty() => {
                    // Find best match - prioritize exact name and struct/impl over enum_variant
                    let best = r.symbols.iter()
                        .filter(|s| s.name == edit.symbol || s.name.ends_with(&edit.symbol))
                        .max_by_key(|s| {
                            let kind_score = match s.kind.as_str() {
                                "impl" => 100,
                                "struct" | "class" => 90,
                                "function" | "method" => 80,
                                "enum" | "trait" | "interface" => 70,
                                "field" => 50,
                                "enum_variant" => 10,
                                _ => 60,
                            };
                            let exact_match = if s.name == edit.symbol { 1000 } else { 0 };
                            kind_score + exact_match
                        })
                        .unwrap_or(&r.symbols[0]);

                    resolved.push((edit, best.file.clone(), best.start_line, best.end_line));
                }
                Ok(_) => {
                    return ToolResult::error(format!(
                        "Symbol '{}' not found. Use 'search' tool to find available symbols.",
                        edit.symbol
                    ));
                }
                Err(e) => {
                    return ToolResult::error(format!("Failed to search for '{}': {}", edit.symbol, e));
                }
            }
        }

        // Now apply all edits (or preview them)
        let mut results: Vec<EditResult> = Vec::new();
        let mut all_success = true;

        for (edit, file, start, end) in resolved {
            let path = context.project_root.join(&file);

            let original = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    results.push(EditResult {
                        symbol: edit.symbol,
                        file,
                        start_line: start,
                        end_line: end,
                        lines_changed: 0,
                        error: Some(format!("Failed to read: {}", e)),
                    });
                    all_success = false;
                    continue;
                }
            };

            let lines: Vec<&str> = original.lines().collect();
            let start_idx = (start as usize).saturating_sub(1);
            let end_idx = (end as usize).min(lines.len());

            let old_len = end_idx - start_idx;
            let new_len = edit.content.lines().count();
            let lines_changed = new_len as i32 - old_len as i32;

            if !preview {
                // Build new content
                let mut new_lines: Vec<&str> = Vec::new();
                new_lines.extend_from_slice(&lines[..start_idx]);
                for line in edit.content.lines() {
                    new_lines.push(line);
                }
                new_lines.extend_from_slice(&lines[end_idx..]);

                let new_content = new_lines.join("\n");
                if let Err(e) = fs::write(&path, &new_content) {
                    results.push(EditResult {
                        symbol: edit.symbol,
                        file,
                        start_line: start,
                        end_line: end,
                        lines_changed: 0,
                        error: Some(format!("Failed to write: {}", e)),
                    });
                    all_success = false;
                    continue;
                }
            }

            results.push(EditResult {
                symbol: edit.symbol,
                file,
                start_line: start,
                end_line: end,
                lines_changed,
                error: None,
            });
        }

        let message = if preview {
            format!("Preview: {} edits ready to apply", results.len())
        } else if all_success {
            format!("Applied {} edits successfully", results.len())
        } else {
            format!("Completed with some errors ({} total)", results.len())
        };

        ToolResult::success(BatchEditOutput {
            success: all_success,
            edits: results,
            message,
        })
    }
}
