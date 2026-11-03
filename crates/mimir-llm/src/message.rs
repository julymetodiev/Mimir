//! Message types for LLM conversations.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Role in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Content block in a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content.
    Text { text: String },

    /// Tool use request from the assistant.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Tool result from the user.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

impl ContentBlock {
    /// Create a text content block.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Create a tool use content block.
    pub fn tool_use(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        Self::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    /// Create a tool result content block.
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error,
        }
    }

    /// Get text content if this is a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Check if this is a tool use block.
    pub fn is_tool_use(&self) -> bool {
        matches!(self, Self::ToolUse { .. })
    }
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message sender.
    pub role: Role,

    /// Content blocks in the message.
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Create a new message with text content.
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::text(text)],
        }
    }

    /// Create a user message.
    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, text)
    }

    /// Create an assistant message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, text)
    }

    /// Create an assistant message with tool use.
    pub fn assistant_tool_use(
        id: impl Into<String>,
        name: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::tool_use(id, name, input)],
        }
    }

    /// Create a user message with tool result.
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::tool_result(tool_use_id, content, is_error)],
        }
    }

    /// Add a content block to the message.
    pub fn with_content(mut self, block: ContentBlock) -> Self {
        self.content.push(block);
        self
    }

    /// Get all text content concatenated.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get tool use blocks.
    pub fn tool_uses(&self) -> Vec<&ContentBlock> {
        self.content.iter().filter(|c| c.is_tool_use()).collect()
    }

    /// Check if this message contains tool use.
    pub fn has_tool_use(&self) -> bool {
        self.content.iter().any(|c| c.is_tool_use())
    }
}

/// Stop reason for a completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Model finished naturally.
    EndTurn,
    /// Model wants to use a tool.
    ToolUse,
    /// Max tokens reached.
    MaxTokens,
    /// Stop sequence encountered.
    StopSequence,
}

/// Usage statistics for a completion.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Number of input tokens.
    pub input_tokens: u32,
    /// Number of output tokens.
    pub output_tokens: u32,
    /// Cache creation tokens (if applicable).
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
    /// Cache read tokens (if applicable).
    #[serde(default)]
    pub cache_read_input_tokens: u32,
}

impl Usage {
    /// Total tokens used.
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

/// Response from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    /// The response message.
    pub message: Message,
    /// Why the model stopped.
    pub stop_reason: StopReason,
    /// Token usage statistics.
    pub usage: Usage,
    /// Model that generated the response.
    pub model: String,
}

impl Completion {
    /// Get the text content of the response.
    pub fn text(&self) -> String {
        self.message.text()
    }

    /// Check if the model wants to use tools.
    pub fn wants_tool_use(&self) -> bool {
        self.stop_reason == StopReason::ToolUse || self.message.has_tool_use()
    }

    /// Get tool use requests.
    pub fn tool_uses(&self) -> Vec<&ContentBlock> {
        self.message.tool_uses()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "Hello");
    }

    #[test]
    fn test_message_tool_use() {
        let msg = Message::assistant_tool_use(
            "id-1",
            "read_file",
            serde_json::json!({"path": "test.rs"}),
        );
        assert!(msg.has_tool_use());
        assert_eq!(msg.tool_uses().len(), 1);
    }

    #[test]
    fn test_completion_text() {
        let completion = Completion {
            message: Message::assistant("Hello, world!"),
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
            model: "test".to_string(),
        };
        assert_eq!(completion.text(), "Hello, world!");
        assert!(!completion.wants_tool_use());
    }
}
