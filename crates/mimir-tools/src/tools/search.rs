//! Search tool - search for symbols by pattern.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Search result item.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchMatch {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
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
        "Search for symbols (functions, structs, etc.) by name pattern. Supports regex patterns. Returns matching symbols with their locations."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::required_string(
                "pattern",
                "Search pattern (regex supported). Example: 'handle.*request' or 'User'",
            ))
            .with_param(ToolParameter::optional_string(
                "kind",
                "Filter by symbol kind: function, struct, enum, trait, method, impl, module",
            ))
            .with_param(ToolParameter::optional_integer(
                "limit",
                "Maximum number of results to return (default: 20)",
            ))
    }

    async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
        let pattern = match params["pattern"].as_str() {
            Some(p) => p,
            None => return ToolResult::error("'pattern' parameter is required"),
        };

        let _kind = params["kind"].as_str();
        let _limit = params["limit"].as_u64().unwrap_or(20) as usize;

        // TODO: Integrate with daemon client for actual search
        // For now, return a placeholder
        let output = SearchOutput {
            pattern: pattern.to_string(),
            matches: vec![],
            total_count: 0,
            truncated: false,
        };

        ToolResult::success(output)
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
