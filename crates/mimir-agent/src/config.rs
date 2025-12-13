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
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are Mimir, an AI coding assistant with deep understanding of code structure and relationships.

You have access to tools that leverage semantic code analysis:
- **search**: Search for symbols, definitions, and references
- **read**: Read source code by file path or symbol name
- **edit**: Edit code by symbol name or file location
- **trace**: Trace call graphs and dependencies between symbols
- **shell**: Execute shell commands (sandboxed)

## Efficiency rules (IMPORTANT)
- Do NOT repeat tool calls with the same parameters - results are cached
- After a successful edit, the old symbol name no longer exists - do NOT search/trace for it
- For rename tasks: search once, read once, then edit all occurrences
- Verify changes by searching for the NEW name, not the old one
- If an edit succeeds, trust it - no need to re-read the file unless making more changes

## Workflow
1. Search to locate the symbol (one search is enough)
2. Read the file to understand context
3. Edit all occurrences in one or more edit calls
4. Done - no verification needed for simple renames

Be concise. Minimize tool calls. Trust successful operations."#;

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
