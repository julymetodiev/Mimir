//! Inspect tool - analyze a symbol deeply (code, context, dependencies).

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
struct InspectOutput {
    symbol: String,
    file: String,
    line: u32,
    code_preview: String,
    callers: Vec<String>,
    callees: Vec<String>,
    summary: String,
}

/// Tool for inspecting symbols deeply.
pub struct InspectTool;

#[async_trait]
impl Tool for InspectTool {
    fn name(&self) -> &str {
        "inspect"
    }

    fn description(&self) -> &str {
        "Analyze a symbol deeply. Returns its code, location, callers (who uses it), and callees (what it uses)."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description()).with_param(ToolParameter::required_string(
            "symbol",
            "Symbol name to inspect",
        ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let symbol_name = match params["symbol"].as_str() {
            Some(s) => s,
            None => return ToolResult::error("'symbol' parameter is required"),
        };

        let client = match &context.graph_client {
            Some(c) => c,
            None => return ToolResult::error("Inspect requires a running daemon."),
        };

        // 1. Get Code & Definition
        let (info, code) = match client.read_symbol(symbol_name).await {
            Ok(Some((i, c))) => (i, c),
            Ok(None) => return ToolResult::error(format!("Symbol '{}' not found.", symbol_name)),
            Err(e) => return ToolResult::error(format!("Graph error: {}", e)),
        };

        // 2. Get Dependencies
        let trace = match client.trace(symbol_name, 1).await {
            Ok(t) => t,
            Err(_) => crate::graph_client::TraceResult {
                symbol: info.clone(),
                callers: vec![],
                callees: vec![],
            },
        };

        // 3. Format Output
        let callers_list: Vec<String> = trace
            .callers
            .iter()
            .map(|s| format!("{} ({}:{})", s.name, s.file, s.start_line))
            .take(10)
            .collect();

        let callees_list: Vec<String> = trace
            .callees
            .iter()
            .map(|s| format!("{} ({}:{})", s.name, s.file, s.start_line))
            .take(10)
            .collect();

        // Summary
        let mut summary = String::new();
        summary.push_str(&format!("# Symbol: {}\n", info.name));
        summary.push_str(&format!(
            "- Location: {}:{}-{}\n",
            info.file, info.start_line, info.end_line
        ));

        summary.push_str("## Source Code\n```rust\n");
        summary.push_str(&code);
        summary.push_str("\n```\n");

        if !callers_list.is_empty() {
            summary.push_str("\n## Callers\n");
            for c in &callers_list {
                summary.push_str(&format!("- {}\n", c));
            }
        }

        ToolResult::success(InspectOutput {
            symbol: info.name,
            file: info.file,
            line: info.start_line,
            code_preview: code,
            callers: callers_list,
            callees: callees_list,
            summary,
        })
    }
}
