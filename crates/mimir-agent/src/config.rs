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
            // Reduced from 8192 to prevent models from generating massive patches
            // Models tend to fill available tokens, causing full file rewrites
            max_tokens: 4096,
            // Low temperature for deterministic code generation
            temperature: Some(0.1),
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
pub const DEFAULT_SYSTEM_PROMPT: &str = r##"You are Mimir, a precise coding assistant. You edit files using the `edit` tool.

## `edit` tool

Use `edit` to modify files by replacing exact text. Parameters:
- `file_path`: path to the file (relative to project root)
- `old_string`: exact text to find and replace (copy from file)
- `new_string`: replacement text
- `replace_all`: true to replace all occurrences (default: false)

### Rules:
1. ALWAYS read the file first with `read` tool
2. `old_string` must be EXACT text from the file (copy-paste)
3. Include 3+ lines of context to uniquely identify the location
4. To CREATE a new file: use empty `old_string` with content in `new_string`

### Example 1 - Add field to struct:
{
  "file_path": "src/user.rs",
  "old_string": "pub struct User {\n    id: u32,\n    name: String,\n}",
  "new_string": "pub struct User {\n    id: u32,\n    name: String,\n    role: Role,\n}"
}

### Example 2 - Add enum after struct:
{
  "file_path": "src/user.rs",
  "old_string": "    email: String,\n}\n\nimpl User {",
  "new_string": "    email: String,\n}\n\npub enum Role {\n    Admin,\n    User,\n    Guest,\n}\n\nimpl User {"
}

### Example 3 - Create new file:
{
  "file_path": "src/role.rs",
  "old_string": "",
  "new_string": "pub enum Role {\n    Admin,\n    User,\n    Guest,\n}"
}

### Workflow:
1. Use `read` to see current file content
2. Copy exact text to `old_string` (including whitespace!)
3. Put modified version in `new_string`
4. Verify with `cargo check` or equivalent

### Alternative: `write` tool
For creating new files or complete rewrites, use `write`:
- `path`: file path
- `content`: complete file content
"##;

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
