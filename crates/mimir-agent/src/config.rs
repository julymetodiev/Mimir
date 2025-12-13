//! Agent configuration.

use serde::{Deserialize, Serialize};

/// Agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// LLM model to use.
    pub model: String,

    /// Maximum iterations (tool call rounds).
    pub max_iterations: u32,

    /// Maximum tokens per request.
    pub max_tokens: u32,

    /// Temperature for LLM responses.
    pub temperature: Option<f32>,

    /// System prompt.
    pub system_prompt: Option<String>,

    /// Whether to create checkpoints after each iteration.
    pub checkpoint_enabled: bool,

    /// Working directory for the agent.
    pub working_dir: Option<String>,

    /// Whether to auto-approve tool calls.
    pub auto_approve: bool,

    /// Tools to enable (empty = all).
    pub enabled_tools: Vec<String>,

    /// Tools to disable.
    pub disabled_tools: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "GLM-4.6".to_string(),
            max_iterations: 50,
            max_tokens: 8192,
            temperature: None,
            system_prompt: None,
            checkpoint_enabled: true,
            working_dir: None,
            auto_approve: false,
            enabled_tools: vec![],
            disabled_tools: vec![],
        }
    }
}

impl AgentConfig {
    /// Create a new config with model.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    /// Set max iterations.
    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    /// Set max tokens.
    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = max;
        self
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Enable checkpoints.
    pub fn with_checkpoints(mut self, enabled: bool) -> Self {
        self.checkpoint_enabled = enabled;
        self
    }

    /// Set working directory.
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Enable auto-approve for tool calls.
    pub fn with_auto_approve(mut self, auto: bool) -> Self {
        self.auto_approve = auto;
        self
    }

    /// Enable specific tools only.
    pub fn with_enabled_tools(mut self, tools: Vec<String>) -> Self {
        self.enabled_tools = tools;
        self
    }

    /// Disable specific tools.
    pub fn with_disabled_tools(mut self, tools: Vec<String>) -> Self {
        self.disabled_tools = tools;
        self
    }
}

/// Default system prompt for the agent.
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are Mimir, a Rust code editor. Be efficient - batch edits when possible.

WORKFLOW: search → read → edit → shell(cargo check)

TOOLS:
1. search {"pattern": "X"} - find symbols (returns code with line numbers)
2. read {"symbol": "Name"} or {"file": "path"} - read symbol/file
3. trace {"symbol": "fn"} - show callers/callees
4. edit - two modes:
   a) EXISTING symbol: {"symbol": "User", "content": "pub struct User {...}"}
   b) NEW code: {"file": "src/x.rs", "mode": "append", "content": "pub enum Foo {...}"}
5. edit_batch {"edits": [{"symbol": "X", "content": "..."}]} - edit MULTIPLE existing symbols
6. shell {"command": "cargo check"} - verify changes

EDIT MODES (for file-based edits):
- "replace" (default): replaces lines start_line..end_line
- "append": adds content at end of file
- "insert": inserts before start_line

EXAMPLES:
# Edit existing symbol
edit: {"symbol": "User", "content": "pub struct User {\n    id: u32,\n    name: String,\n}"}

# Add NEW symbol (not in index yet)
edit: {"file": "src/user.rs", "mode": "append", "content": "pub enum Status {\n    Active,\n    Inactive,\n}"}

# Batch edit (efficient for multiple changes)
edit_batch: {"edits": [
  {"symbol": "User", "content": "pub struct User { id: u32, name: String }"},
  {"symbol": "impl User", "content": "impl User {\n    pub fn new(id: u32) -> Self { Self { id, name: String::new() } }\n}"}
]}

CRITICAL RULES:
1. Symbol names must be EXACT: "User", "impl User", "UserService"
2. Content must NOT include /// doc comments or #[derive(...)] - those are NOT part of the symbol
3. When adding field to struct → edit BOTH struct AND impl (update constructors)
4. For NEW symbols → use edit with file + mode:"append"
5. Always verify with: shell {"command": "cargo check"}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AgentConfig::default();
        assert_eq!(config.model, "GLM-4.6");
        assert_eq!(config.max_iterations, 50);
        assert!(config.checkpoint_enabled);
    }

    #[test]
    fn test_config_builder() {
        let config = AgentConfig::new("claude-3-opus")
            .with_max_iterations(100)
            .with_max_tokens(4096)
            .with_temperature(0.7)
            .with_system_prompt("Custom prompt")
            .with_auto_approve(true);

        assert_eq!(config.model, "claude-3-opus");
        assert_eq!(config.max_iterations, 100);
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.temperature, Some(0.7));
        assert!(config.auto_approve);
    }
}
