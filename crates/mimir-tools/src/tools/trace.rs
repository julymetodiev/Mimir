//! Trace tool - trace symbol call chains.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Symbol info in trace result.
#[derive(Debug, Serialize, Deserialize)]
pub struct TraceSymbol {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub depth: u32,
}

/// Output from the trace tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct TraceOutput {
    pub symbol: String,
    pub kind: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub callers: Vec<TraceSymbol>,
    pub callees: Vec<TraceSymbol>,
    pub source_code: Option<String>,
}

/// Tool for tracing symbol relationships.
pub struct TraceTool;

#[async_trait]
impl Tool for TraceTool {
    fn name(&self) -> &str {
        "trace"
    }

    fn description(&self) -> &str {
        "Trace call chains for a symbol. Shows what calls this symbol (callers) and what this symbol calls (callees). Useful for understanding code relationships."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::required_string(
                "symbol",
                "Symbol name to trace (function, method, etc.)",
            ))
            .with_param(ToolParameter::optional_integer(
                "depth",
                "Maximum depth to trace (default: 3)",
            ))
            .with_param(ToolParameter::optional_bool(
                "callers_only",
                "Only show callers (who calls this symbol)",
                false,
            ))
            .with_param(ToolParameter::optional_bool(
                "callees_only",
                "Only show callees (what this symbol calls)",
                false,
            ))
            .with_param(ToolParameter::optional_bool(
                "include_code",
                "Include source code of the symbol",
                false,
            ))
    }

    async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
        let symbol = match params["symbol"].as_str() {
            Some(s) => s,
            None => return ToolResult::error("'symbol' parameter is required"),
        };

        let _depth = params["depth"].as_u64().unwrap_or(3) as usize;
        let _callers_only = params["callers_only"].as_bool().unwrap_or(false);
        let _callees_only = params["callees_only"].as_bool().unwrap_or(false);
        let _include_code = params["include_code"].as_bool().unwrap_or(false);

        // TODO: Integrate with daemon client for actual tracing
        // For now, return a placeholder
        let output = TraceOutput {
            symbol: symbol.to_string(),
            kind: "unknown".to_string(),
            file: "".to_string(),
            start_line: 0,
            end_line: 0,
            callers: vec![],
            callees: vec![],
            source_code: None,
        };

        ToolResult::success(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_trace_requires_symbol() {
        let tool = TraceTool;
        let context = ToolContext::new("/tmp");
        let params = serde_json::json!({});

        let result = tool.execute(params, &context).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("symbol"));
    }

    #[tokio::test]
    async fn test_trace_schema() {
        let tool = TraceTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "trace");
        assert!(schema.parameters.required.as_ref().unwrap().contains(&"symbol".to_string()));
    }
}
