//! Tool executor for running tools based on LLM requests.

use mimir_llm::ContentBlock;
use mimir_tools::{ToolContext, ToolRegistry, ToolResult};
use serde_json::Value;
use std::sync::Arc;

/// Executes tools based on LLM tool use requests.
pub struct ToolExecutor {
    registry: Arc<ToolRegistry>,
    context: ToolContext,
}

impl ToolExecutor {
    /// Create a new tool executor.
    pub fn new(registry: Arc<ToolRegistry>, context: ToolContext) -> Self {
        Self { registry, context }
    }

    /// Execute a single tool call.
    pub async fn execute_tool(&self, name: &str, input: Value) -> ToolResult {
        self.registry.execute(name, input, &self.context).await
    }

    /// Execute multiple tool calls from content blocks.
    pub async fn execute_tool_calls(&self, blocks: &[ContentBlock]) -> Vec<ToolCallResult> {
        let mut results = vec![];

        for block in blocks {
            if let ContentBlock::ToolUse { id, name, input } = block {
                let result = self.execute_tool(name, input.clone()).await;
                results.push(ToolCallResult {
                    tool_use_id: id.clone(),
                    name: name.clone(),
                    result,
                });
            }
        }

        results
    }

    /// Get available tool schemas.
    pub fn tool_schemas(&self) -> Vec<mimir_tools::ToolSchema> {
        self.registry.schemas()
    }
}

/// Result of a tool call execution.
#[derive(Debug)]
pub struct ToolCallResult {
    /// ID of the tool use request.
    pub tool_use_id: String,
    /// Tool name.
    pub name: String,
    /// Execution result.
    pub result: ToolResult,
}

impl ToolCallResult {
    /// Convert to LLM content block for sending back.
    pub fn to_content_block(&self) -> ContentBlock {
        let content = if self.result.success {
            self.result.output.to_string()
        } else {
            self.result.error.clone().unwrap_or_else(|| "Unknown error".to_string())
        };

        ContentBlock::ToolResult {
            tool_use_id: self.tool_use_id.clone(),
            content,
            is_error: !self.result.success,
        }
    }

    /// Check if the execution was successful.
    pub fn is_success(&self) -> bool {
        self.result.success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_result_success() {
        let result = ToolCallResult {
            tool_use_id: "call_123".to_string(),
            name: "read".to_string(),
            result: ToolResult::success("file contents"),
        };

        assert!(result.is_success());

        let block = result.to_content_block();
        match block {
            ContentBlock::ToolResult { is_error, content, .. } => {
                assert!(!is_error);
                assert!(content.contains("file contents"));
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_tool_call_result_error() {
        let result = ToolCallResult {
            tool_use_id: "call_456".to_string(),
            name: "edit".to_string(),
            result: ToolResult::error("bad input"),
        };

        assert!(!result.is_success());

        let block = result.to_content_block();
        match block {
            ContentBlock::ToolResult { is_error, content, .. } => {
                assert!(is_error);
                assert!(content.contains("bad input"));
            }
            _ => panic!("Expected ToolResult"),
        }
    }
}
