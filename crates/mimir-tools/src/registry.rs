//! Tool registry for managing available tools.

use crate::{SharedTool, Tool, ToolContext, ToolResult, ToolSchema};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of available tools.
///
/// The registry manages all tools available to the agent and provides
/// methods for looking up and executing tools by name.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, SharedTool>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Create a registry with all built-in tools.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(crate::tools::SearchTool);
        registry.register(crate::tools::ReadTool);
        registry.register(crate::tools::TraceTool);
        registry.register(crate::tools::InspectTool);
        registry.register(crate::tools::EditTool);  // Primary edit tool (string replacement)
        registry.register(crate::tools::WriteTool);
        registry.register(crate::tools::ShellTool);
        registry
    }

    /// Register a tool.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    /// Register a shared tool.
    pub fn register_shared(&mut self, tool: SharedTool) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&SharedTool> {
        self.tools.get(name)
    }

    /// Check if a tool exists.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Get all tool names.
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Get all tool schemas.
    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    /// Get tool schemas in Anthropic format.
    pub fn to_anthropic_tools(&self) -> Vec<Value> {
        self.schemas().iter().map(|s| s.to_anthropic_format()).collect()
    }

    /// Get tool schemas in OpenAI format.
    pub fn to_openai_functions(&self) -> Vec<Value> {
        self.schemas().iter().map(|s| s.to_openai_format()).collect()
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, params: Value, context: &ToolContext) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.execute(params, context).await,
            None => ToolResult::error(format!("Tool '{}' not found", name)),
        }
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.names())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolParameter;
    use async_trait::async_trait;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo the input"
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema::new("echo", "Echo the input")
                .with_param(ToolParameter::required_string("message", "Message to echo"))
        }

        async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
            let message = params["message"].as_str().unwrap_or("no message");
            ToolResult::success(message)
        }
    }

    #[test]
    fn test_registry_register() {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);
        assert!(registry.contains("echo"));
        assert_eq!(registry.len(), 1);
    }

    #[tokio::test]
    async fn test_registry_execute() {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);

        let context = ToolContext::new("/tmp");
        let params = serde_json::json!({"message": "hello"});

        let result = registry.execute("echo", params, &context).await;
        assert!(result.success);
        assert_eq!(result.output, Value::String("hello".to_string()));
    }

    #[tokio::test]
    async fn test_registry_execute_unknown() {
        let registry = ToolRegistry::new();
        let context = ToolContext::new("/tmp");

        let result = registry.execute("unknown", Value::Null, &context).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not found"));
    }

    #[test]
    fn test_registry_schemas() {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);

        let schemas = registry.schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "echo");
    }
}
