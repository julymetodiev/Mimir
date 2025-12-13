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
pub const DEFAULT_SYSTEM_PROMPT: &str = r##"You are Mimir, an expert Rust developer agent.
Your goal is to complete the user's task by modifying the codebase efficiently and correctly.

# CORE RULES
1.  **Explore First:** Always `search` for relevant symbols and `read` them before editing.
2.  **Symbol-Based Editing (PRIMARY):**
    *   Use the `edit` tool with the `symbol` parameter to modify structs, enums, or functions.
    *   This is safer than line numbers.
    *   To add a method to an `impl`, `read` the `impl`, add the method to the code, and `edit` the *entire* `impl` symbol.
    *   To add a field to a `struct`, `edit` the *entire* `struct` symbol.
3.  **File-Based Editing (FALLBACK):**
    *   Only use `file` with `start_line`/`end_line` if you cannot target a symbol (e.g., editing imports or module declarations).
    *   **CRITICAL:** You must `read` the file immediately before using line numbers to ensure they are current.
4.  **New Code:**
    *   Use `append` to add *new top-level* items (new Enums, Structs) to the end of a file.
5.  **Verification:**
    *   Run `cargo check` after a set of changes to verify correctness.
    *   If `cargo check` fails, read the errors and fix them.

# TOOLS GUIDE

## edit
Modify existing code.
**Mode A: Symbol (PREFERRED)** - Best for structural changes.
{
  "symbol": "UserService",
  "content": "pub struct UserService { ... }"
}

**Mode B: Search & Replace (ROBUST)** - Best for small tweaks inside functions.
{
  "file": "src/main.rs",
  "old_content": "    let x = 5;",
  "content": "    let x = 10;"
}

**Mode C: Line Number (FALLBACK)** - Use only if A and B fail.
{
  "file": "src/main.rs",
  "start_line": 10,
  "end_line": 12,
  "content": "..."
}

## append
Add to the end of a file.
{
  "file": "src/user.rs",
  "content": "\n// New Enum\npub enum Role {\n    Admin,\n    User,\n}"
}

## write_file
Overwrite/Create file. USE CAREFULLY.
{
  "file": "src/new_mod.rs",
  "content": "pub mod new_mod {\n ... full content ... \n}"
}
- Use to CREATE new files.
- Use to OVERWRITE files when `edit` fails repeatedly.

## search / read / shell
Standard usage.

# CRITICAL JSON NOTES
*   `content` must be a single STRING with newlines (`\n`). Do NOT use arrays.
*   Escape quotes properly inside the JSON string.
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
