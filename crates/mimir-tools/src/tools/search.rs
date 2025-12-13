//! Search tool - search for symbols by pattern.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

/// Search result item.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchMatch {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Output from the search tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchOutput {
    pub pattern: String,
    pub matches: Vec<SearchMatch>,
    pub total_count: usize,
    pub truncated: bool,
}

/// Tool for searching symbols.
pub struct SearchTool;

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search for symbols by name pattern. Use code=true to include source code in results (recommended - avoids needing separate read calls)."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::required_string(
                "pattern",
                "Search pattern. Example: 'User' finds User struct and impl User block.",
            ))
            .with_param(ToolParameter::optional_bool(
                "definitions",
                "Only show definitions (struct, impl, enum, trait, function) - excludes methods/fields. Default: true.",
                true,
            ))
            .with_param(ToolParameter::optional_integer(
                "limit",
                "Maximum results (default: 10)",
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let pattern = match params["pattern"].as_str() {
            Some(p) => p,
            None => return ToolResult::error("'pattern' parameter is required"),
        };

        let definitions_only = params["definitions"].as_bool().unwrap_or(true);
        let limit = params["limit"].as_u64().unwrap_or(10) as usize;

        // Use graph client if available
        let client = match &context.graph_client {
            Some(c) => c,
            None => return ToolResult::error("Search requires daemon. Start with: mimir daemon start"),
        };

        // Search with higher limit to account for filtering
        let search_limit = if definitions_only { limit * 5 } else { limit };

        match client.search(pattern, None, search_limit).await {
            Ok(result) => {
                let matches: Vec<SearchMatch> = result
                    .symbols
                    .into_iter()
                    .filter(|s| {
                        if definitions_only {
                            // Only keep top-level definitions
                            matches!(s.kind.to_lowercase().as_str(),
                                "struct" | "impl" | "enum" | "trait" | "function" | "interface" | "class")
                        } else {
                            true
                        }
                    })
                    .take(limit)
                    .map(|s| {
                        let code = self.read_symbol_code(&s.file, s.start_line, s.end_line, context);
                        SearchMatch {
                            name: s.name,
                            qualified_name: s.qualified_name,
                            kind: s.kind,
                            file: s.file,
                            start_line: s.start_line,
                            end_line: s.end_line,
                            code,
                        }
                    })
                    .collect();

                let output = SearchOutput {
                    pattern: pattern.to_string(),
                    matches,
                    total_count: result.total,
                    truncated: result.total > limit,
                };
                ToolResult::success(output)
            }
            Err(e) => ToolResult::error(format!("Search failed: {}", e)),
        }
    }
}

impl SearchTool {
    fn read_symbol_code(
        &self,
        file: &str,
        start_line: u32,
        end_line: u32,
        context: &ToolContext,
    ) -> Option<String> {
        let path = context.project_root.join(file);
        let content = fs::read_to_string(&path).ok()?;
        let lines: Vec<&str> = content.lines().collect();

        let start = (start_line as usize).saturating_sub(1);
        let end = (end_line as usize).min(lines.len());

        if start >= lines.len() {
            return None;
        }

        Some(lines[start..end].join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_requires_pattern() {
        let tool = SearchTool;
        let context = ToolContext::new("/tmp");
        let params = serde_json::json!({});

        let result = tool.execute(params, &context).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("pattern"));
    }

    #[tokio::test]
    async fn test_search_schema() {
        let tool = SearchTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "search");
        assert!(schema.parameters.required.as_ref().unwrap().contains(&"pattern".to_string()));
    }
}
