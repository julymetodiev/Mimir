//! Tool definitions for Mimir agent.
//!
//! This crate provides the core abstraction for tools that the agent can use,
//! including the Tool trait, ToolRegistry, and built-in tool implementations.

mod registry;
mod schema;
mod tools;

pub use registry::ToolRegistry;
pub use schema::{JsonSchema, ToolParameter, ToolSchema};
pub use tools::*;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Whether the tool execution succeeded.
    pub success: bool,
    /// Output from the tool (can be any JSON value).
    pub output: Value,
    /// Error message if the tool failed.
    pub error: Option<String>,
}

impl ToolResult {
    /// Create a successful result.
    pub fn success(output: impl Serialize) -> Self {
        Self {
            success: true,
            output: serde_json::to_value(output).unwrap_or(Value::Null),
            error: None,
        }
    }

    /// Create a failed result.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            output: Value::Null,
            error: Some(message.into()),
        }
    }

    /// Create a result from a Result type.
    pub fn from_result<T: Serialize, E: std::fmt::Display>(result: Result<T, E>) -> Self {
        match result {
            Ok(value) => Self::success(value),
            Err(e) => Self::error(e.to_string()),
        }
    }
}

/// Context provided to tools during execution.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Project root directory.
    pub project_root: std::path::PathBuf,
    /// Agent directory (.agent/).
    pub agent_dir: std::path::PathBuf,
    /// Whether to run in dry-run mode.
    pub dry_run: bool,
    /// Additional context values.
    pub extra: HashMap<String, Value>,
}

impl ToolContext {
    /// Create a new tool context.
    pub fn new(project_root: impl Into<std::path::PathBuf>) -> Self {
        let root = project_root.into();
        let agent_dir = root.join(".agent");
        Self {
            project_root: root,
            agent_dir,
            dry_run: false,
            extra: HashMap::new(),
        }
    }

    /// Set dry-run mode.
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }
}

/// Core trait for all tools.
///
/// Tools are the primary way the agent interacts with the codebase.
/// Each tool has a name, description, schema, and execute method.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name of the tool.
    fn name(&self) -> &str;

    /// Human-readable description of what the tool does.
    fn description(&self) -> &str;

    /// JSON schema for the tool's parameters.
    fn schema(&self) -> ToolSchema;

    /// Execute the tool with the given parameters.
    ///
    /// # Arguments
    /// * `params` - JSON object containing the tool parameters
    /// * `context` - Execution context with project info
    ///
    /// # Returns
    /// Result of the tool execution
    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult;
}

/// Type alias for a shared tool reference.
pub type SharedTool = Arc<dyn Tool>;

/// Tool invocation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Name of the tool to call.
    pub name: String,
    /// Parameters to pass to the tool.
    pub parameters: Value,
}

impl ToolCall {
    /// Create a new tool call.
    pub fn new(name: impl Into<String>, parameters: impl Serialize) -> Self {
        Self {
            name: name.into(),
            parameters: serde_json::to_value(parameters).unwrap_or(Value::Object(Default::default())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("hello");
        assert!(result.success);
        assert_eq!(result.output, Value::String("hello".to_string()));
        assert!(result.error.is_none());
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("something went wrong");
        assert!(!result.success);
        assert_eq!(result.error, Some("something went wrong".to_string()));
    }

    #[test]
    fn test_tool_result_from_result() {
        let ok: Result<&str, &str> = Ok("success");
        let result = ToolResult::from_result(ok);
        assert!(result.success);

        let err: Result<&str, &str> = Err("failed");
        let result = ToolResult::from_result(err);
        assert!(!result.success);
    }
}
