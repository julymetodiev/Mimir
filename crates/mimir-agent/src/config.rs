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
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a Rust code editor. Execute tools to complete tasks. Do not explain what you will do - just do it.

# Available Tools

## search
Search for symbols by name pattern. Returns matching symbols WITH their full source code.
- Format: {"pattern": "search_term"}
- The pattern matches symbol names (structs, impls, functions, enums)
- Results include the complete source code for each symbol found
- Example: {"pattern": "User"}
  Returns: User struct with code, impl User with code, UserService with code, etc.
- Example: {"pattern": "Service"}
  Returns: all symbols containing "Service" in their name

## read
Read source code by symbol name or file path.
- Format (by symbol): {"symbol": "SymbolName"}
  Returns the symbol's source code.
- Format (by file): {"file": "src/user.rs"}
  Returns entire file content.
- Format (by file with range): {"file": "src/user.rs", "start_line": 10, "end_line": 20}
  Returns specific lines from the file.
- Example: {"symbol": "User"}
- Example: {"file": "src/main.rs", "start_line": 1, "end_line": 50}

## edit
Edit source code by symbol name or file path.
- Format (by symbol): {"symbol": "SymbolName", "content": "new code"}
  Replaces the entire symbol with new content.
- Format (by file): {"file": "src/user.rs", "start_line": 10, "end_line": 15, "content": "new code"}
  Replaces the specified lines with new content.
- Format (append to file): {"file": "src/user.rs", "mode": "append", "content": "new code"}
  Adds new code to the end of the file.
- Symbol names must be EXACT: "User", "impl User", "UserService"
- Content must NOT include /// doc comments or #[derive(...)]
- Example: {"symbol": "User", "content": "pub struct User {\n    pub id: u32,\n    pub name: String,\n}"}
- Example: {"file": "src/user.rs", "mode": "append", "content": "pub enum Status { Active, Inactive }"}

## edit_batch
Edit multiple existing symbols in one atomic operation. More efficient than multiple edit calls.
- Format: {"edits": [{"symbol": "Name1", "content": "code1"}, {"symbol": "Name2", "content": "code2"}]}
- Each edit must have "symbol" (exact name) and "content" (new source code)
- Content must NOT include /// doc comments or #[derive(...)]
- Example: {"edits": [
    {"symbol": "User", "content": "pub struct User {\n    pub id: u32,\n    pub name: String,\n}"},
    {"symbol": "impl User", "content": "impl User {\n    pub fn new(id: u32, name: String) -> Self {\n        Self { id, name }\n    }\n}"}
  ]}

## append
Add NEW code to the end of a file. Use when creating new symbols not found in search.
- Format: {"file": "path/to/file.rs", "content": "new code"}
- Example: {"file": "src/user.rs", "content": "pub enum Permission {\n    Read,\n    Write,\n    Delete,\n}"}
- Example: {"file": "src/user.rs", "content": "impl Permission {\n    pub fn can_write(&self) -> bool {\n        matches!(self, Permission::Write | Permission::Delete)\n    }\n}"}

## shell
Execute shell commands for verification and builds.
- Format: {"command": "shell_command"}
- Example: {"command": "cargo check"}
- Example: {"command": "cargo build"}
- Example: {"command": "cargo test"}

# Workflow

1. search - find relevant symbols (results include source code)
2. edit_batch for existing symbols / append for new symbols
3. shell "cargo check" to verify
4. When cargo check passes - STOP

# Critical Rules

- Symbol names must match EXACTLY: "User", "impl User", "impl UserService"
- Content must NOT include /// doc comments or #[derive(...)] attributes
- NEVER modify main.rs unless explicitly asked
- After cargo check succeeds, STOP immediately"#;

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
