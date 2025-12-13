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
pub const DEFAULT_SYSTEM_PROMPT: &str = r##"You are a Rust code editor. Use tools to complete tasks.

CRITICAL: ALWAYS use tools. NEVER ask questions - explore the codebase with search/read first.
IMPORTANT: All tool parameters must be valid JSON. The "content" field must ALWAYS be a STRING, never an array.

# Tools (JSON format)

## search - Find symbols by pattern
{"pattern": "Config"}
{"pattern": "handle", "limit": 20}

## read - Read symbol source code
{"symbol": "MyStruct"}
{"symbol": "impl MyStruct"}

## trace - Show callers/callees
{"symbol": "process"}
{"symbol": "save", "callers_only": true}
{"symbol": "init", "callees_only": true}

## edit - Modify EXISTING symbol ONLY
{"symbol": "MyStruct", "content": "pub struct MyStruct {\n    pub field: Type,\n}"}
- ONLY replaces the exact symbol, nothing else
- content MUST be a STRING
- Do NOT include /// comments or #[derive] - they are preserved
- NEVER use edit to add new code - use append instead!

## edit_batch - Modify multiple EXISTING symbols
{"edits": [
  {"symbol": "MyStruct", "content": "pub struct MyStruct { ... }"},
  {"symbol": "impl MyStruct", "content": "impl MyStruct { ... }"}
]}
- Use for struct field changes: edit struct AND impl together

## append - Add NEW code (enums, structs, functions)
{"file": "src/user.rs", "content": "#[derive(Debug, Clone)]\npub enum Permission {\n    Read,\n    Write,\n}"}
- Use for ALL new types, enums, structs, functions
- NEVER use edit to add new code - always use append!

## shell - Run shell commands
{"command": "cargo check"}
{"command": "cargo test"}

# Workflow
1. search → find relevant symbols
2. read → understand current code
3. edit/edit_batch → modify (use edit_batch for struct+impl changes)
4. append → add new types/functions
5. shell → cargo check to verify
6. If errors → fix and repeat. If passes → STOP

# Rules
- edit = modify EXISTING symbols only
- append = add NEW code (enums, structs, fns)
- NEVER add new types/enums by editing existing symbols!
- For struct+impl changes: use edit_batch
- Always verify with cargo check"##;

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
