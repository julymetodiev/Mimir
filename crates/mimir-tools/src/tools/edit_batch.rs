//! Batch edit tool - edit multiple symbols in one call.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

/// Single symbol edit in a batch.
#[derive(Debug, Deserialize)]
struct SymbolEdit {
    symbol: String,
    content: String,
}

/// Resolved edit with file and line info.
#[derive(Debug)]
struct ResolvedEdit {
    symbol: String,
    content: String,
    file: String,
    start_line: u32,
    end_line: u32,
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
            None => {
                return ToolResult::error(
                    "Batch editing requires daemon connection. Start with: mimir daemon start",
                )
            }
        };

        let edits: Vec<SymbolEdit> = match serde_json::from_value(params["edits"].clone()) {
            Ok(e) => e,
            Err(e) => {
                return ToolResult::error(format!(
                    "Invalid edits format: {}. Expected: [{{\"symbol\": \"Name\", \"content\": \"code\"}}]",
                    e
                ))
            }
        };

        if edits.is_empty() {
            return ToolResult::error("No edits provided");
        }

        let preview = params["preview"].as_bool().unwrap_or(false);

        // Resolve all symbols first to check for errors before making changes
        let mut resolved: Vec<ResolvedEdit> = Vec::new();

        for edit in edits {
            // Search with high limit to find definitions buried under methods
            let result = client.search(&edit.symbol, None, 200).await;

            match result {
                Ok(r) if !r.symbols.is_empty() => {
                    // Find best match - MUST match name exactly, prioritize struct/impl
                    let best = r
                        .symbols
                        .iter()
                        .filter(|s| s.name == edit.symbol)
                        .max_by_key(|s| {
                            let kind_lower = s.kind.to_lowercase();
                            match kind_lower.as_str() {
                                "impl" => 100,
                                "struct" | "class" => 90,
                                "function" => 80,
                                "enum" | "trait" | "interface" => 70,
                                "enum_variant" => 10,
                                _ => 60,
                            }
                        });

                    match best {
                        Some(sym) => {
                            resolved.push(ResolvedEdit {
                                symbol: edit.symbol,
                                content: edit.content,
                                file: sym.file.clone(),
                                start_line: sym.start_line,
                                end_line: sym.end_line,
                            });
                        }
                        None => {
                            return ToolResult::error(format!(
                                "Symbol '{}' not found with exact name match. Available: {:?}",
                                edit.symbol,
                                r.symbols.iter().take(5).map(|s| &s.name).collect::<Vec<_>>()
                            ));
                        }
                    }
                }
                Ok(_) => {
                    return ToolResult::error(format!(
                        "Symbol '{}' not found. Use 'search' tool to find available symbols.",
                        edit.symbol
                    ));
                }
                Err(e) => {
                    return ToolResult::error(format!(
                        "Failed to search for '{}': {}",
                        edit.symbol, e
                    ));
                }
            }
        }

        // Group edits by file
        let mut edits_by_file: HashMap<String, Vec<ResolvedEdit>> = HashMap::new();
        for edit in resolved {
            edits_by_file
                .entry(edit.file.clone())
                .or_default()
                .push(edit);
        }

        // Sort each file's edits by start_line DESCENDING (bottom-up)
        // This ensures line numbers remain valid after each edit
        for file_edits in edits_by_file.values_mut() {
            file_edits.sort_by(|a, b| b.start_line.cmp(&a.start_line));
        }

        let mut results: Vec<EditResult> = Vec::new();
        let mut all_success = true;

        // Process each file
        for (file, file_edits) in edits_by_file {
            let path = context.project_root.join(&file);

            let original = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    for edit in file_edits {
                        results.push(EditResult {
                            symbol: edit.symbol,
                            file: file.clone(),
                            start_line: edit.start_line,
                            end_line: edit.end_line,
                            lines_changed: 0,
                            error: Some(format!("Failed to read: {}", e)),
                        });
                    }
                    all_success = false;
                    continue;
                }
            };

            // Preserve trailing newline
            let has_trailing_newline = original.ends_with('\n');
            let mut lines: Vec<String> = original.lines().map(|s| s.to_string()).collect();

            // Apply edits bottom-up (already sorted descending by start_line)
            for edit in file_edits {
                let start_idx = (edit.start_line as usize).saturating_sub(1);
                let end_idx = (edit.end_line as usize).min(lines.len());

                if start_idx >= lines.len() {
                    results.push(EditResult {
                        symbol: edit.symbol,
                        file: file.clone(),
                        start_line: edit.start_line,
                        end_line: edit.end_line,
                        lines_changed: 0,
                        error: Some("Invalid line range".to_string()),
                    });
                    all_success = false;
                    continue;
                }

                let old_len = end_idx - start_idx;
                let new_lines: Vec<String> = edit.content.lines().map(|s| s.to_string()).collect();
                let new_len = new_lines.len();
                let lines_changed = new_len as i32 - old_len as i32;

                if !preview {
                    // Remove old lines and insert new ones
                    lines.splice(start_idx..end_idx, new_lines);
                }

                results.push(EditResult {
                    symbol: edit.symbol,
                    file: file.clone(),
                    start_line: edit.start_line,
                    end_line: edit.end_line,
                    lines_changed,
                    error: None,
                });
            }

            if !preview {
                // Write back with proper line ending
                let mut new_content = lines.join("\n");
                if has_trailing_newline {
                    new_content.push('\n');
                }

                if let Err(e) = fs::write(&path, &new_content) {
                    // Mark all edits for this file as failed
                    for result in results.iter_mut().filter(|r| r.file == file) {
                        result.error = Some(format!("Failed to write: {}", e));
                    }
                    all_success = false;
                }
            }
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
