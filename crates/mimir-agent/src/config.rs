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
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a Rust code editor. Use tools to modify code. Do not explain - just execute.

# Tools

## search
Find symbols. Returns symbol names AND their full source code.
Format: {"pattern": "SearchTerm"}
Example: {"pattern": "User"}
Result: Returns User struct code, impl User code, UserService code, etc.

## edit_batch
Edit multiple existing symbols at once. This is the PREFERRED tool for making changes.
Format: {"edits": [{"symbol": "ExactName", "content": "new code"}, ...]}

IMPORTANT:
- "symbol" must be the EXACT name from search results: "User", "impl User", "UserService"
- "content" is the complete new code for that symbol
- "content" must NOT include /// doc comments
- "content" must NOT include #[derive(...)] attributes

Example - modify struct and its impl:
{"edits": [
  {"symbol": "User", "content": "pub struct User {\n    pub id: u32,\n    pub name: String,\n    pub active: bool,\n}"},
  {"symbol": "impl User", "content": "impl User {\n    pub fn new(id: u32, name: String) -> Self {\n        Self { id, name, active: true }\n    }\n    pub fn is_active(&self) -> bool {\n        self.active\n    }\n}"}
]}

Example - add new enum by including it in an impl block's file context:
First search for existing symbols, then use edit_batch to modify "impl User" to include the new functionality.

## edit
Edit a single symbol. Use edit_batch instead when possible.
Format: {"symbol": "ExactName", "content": "new code"}
Example: {"symbol": "User", "content": "pub struct User {\n    pub id: u32,\n}"}

## shell
Execute shell commands. Use for verification.
Format: {"command": "command string"}
Example: {"command": "cargo check"}

# Workflow

Step 1: Search for relevant symbols
{"pattern": "User"}
This returns the current code for User, impl User, etc.

Step 2: Modify using edit_batch
Take the code from search results, modify it, and pass to edit_batch.
{"edits": [{"symbol": "User", "content": "modified code..."}, {"symbol": "impl User", "content": "modified impl..."}]}

Step 3: Verify with cargo check
{"command": "cargo check"}

Step 4: If cargo check passes, STOP. Task complete.

# Critical Rules

1. ONLY use symbol parameter in edit/edit_batch. NEVER use file, start_line, end_line.
2. Symbol names must be EXACT match from search: "User", "impl User", "impl UserService"
3. Content must NOT have /// comments or #[derive]
4. NEVER modify main.rs
5. After cargo check succeeds → STOP immediately
6. To add new types (enum, struct): include them in the content when editing an existing impl block

# How to Remove Code

To remove a method from an impl block:
1. Search for the impl block: {"pattern": "impl User"}
2. Copy the ENTIRE impl block content from search results
3. Remove the unwanted methods from the content
4. Use edit_batch with the modified content (without the removed methods)

To remove an enum/struct: You cannot delete symbols directly. Instead, remove all references to it from other symbols, and it will become unused.

Example - remove method "foo" from impl User:
Original impl User has: new(), foo(), bar()
New content should have: new(), bar() (without foo)"#;

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
