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
pub const DEFAULT_SYSTEM_PROMPT: &str = r##"You are a Rust code editor. Complete tasks efficiently with minimal tool calls.

CRITICAL: ALWAYS use tools. NEVER ask questions - explore with search/read first.
IMPORTANT: All "content" fields must be a STRING, never an array.

# Tools (JSON format)

## search - Find symbols by pattern
{"pattern": "User"}
{"pattern": "handle", "limit": 20}
Search ONCE with the main keyword from the task.

## read - Read symbol source code
{"symbol": "MyStruct"}
{"symbol": "impl MyStruct"}
Only read what you need to modify.

## trace - Show callers/callees
{"symbol": "process"}
{"symbol": "save", "callers_only": true}

## edit - Modify EXISTING symbol ONLY
{"symbol": "MyStruct", "content": "pub struct MyStruct {\n    pub field: Type,\n}", "preview": true}
- ALWAYS use "preview": true FIRST to see the diff
- Then call again WITHOUT preview to apply changes
- content MUST be a STRING
- Do NOT include /// comments or #[derive] - they are preserved
- NEVER use edit to add new code - use append instead!

## edit_batch - Modify multiple EXISTING symbols
{"edits": [
  {"symbol": "MyStruct", "content": "pub struct MyStruct { ... }"},
  {"symbol": "impl MyStruct", "content": "impl MyStruct { ... }"}
], "preview": true}
- Use "preview": true FIRST, then apply
- Use for struct field changes: edit struct AND impl together

## append - Add NEW code (enums, structs, functions)
{"file": "src/user.rs", "content": "#[derive(Debug, Clone)]\npub enum Permission {\n    Read,\n    Write,\n}"}
- Use for ALL new types, enums, structs, functions
- NEVER use edit to add new code - always use append!

## shell - Run shell commands
{"command": "cargo check"}
{"command": "cargo test"}

# Workflow
1. search ONCE → find relevant symbols
2. read → only what you'll modify
3. edit with preview:true → verify the diff looks correct
4. edit without preview → apply changes
5. append → for new types/functions
6. cargo check ONCE → if pass, STOP immediately

# Rules
- Be efficient: 1 search, minimal reads, 1 cargo check
- ALWAYS preview before applying edits
- edit = EXISTING symbols only
- append = NEW code (enums, structs, fns)
- NEVER add new types by editing existing symbols!
- For struct+impl changes: use edit_batch
- Don't re-read after editing
- STOP immediately when cargo check passes"##;

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
