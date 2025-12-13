//! Checkpoint system for saving and restoring agent state.

use crate::{TaskId, TaskResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Unique checkpoint identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CheckpointId(pub String);

impl CheckpointId {
    /// Generate a new unique checkpoint ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for CheckpointId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    /// Role (user, assistant, system).
    pub role: String,
    /// Message content.
    pub content: Value,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

impl ConversationMessage {
    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Value::String(content.into()),
            timestamp: Utc::now(),
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Value::String(content.into()),
            timestamp: Utc::now(),
        }
    }

    /// Create a message with structured content (for tool use).
    pub fn with_content(role: impl Into<String>, content: Value) -> Self {
        Self {
            role: role.into(),
            content,
            timestamp: Utc::now(),
        }
    }
}

/// A checkpoint capturing agent state at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Unique identifier.
    pub id: CheckpointId,

    /// Associated task ID.
    pub task_id: TaskId,

    /// Sequence number within the task.
    pub sequence: u32,

    /// Conversation history up to this point.
    pub messages: Vec<ConversationMessage>,

    /// Pending tool calls.
    pub pending_tool_calls: Vec<PendingToolCall>,

    /// Files modified during the task.
    pub modified_files: Vec<ModifiedFile>,

    /// Agent's working memory/notes.
    pub working_memory: Option<String>,

    /// Custom metadata.
    pub metadata: Value,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Description of what was done.
    pub description: Option<String>,
}

impl Checkpoint {
    /// Create a new checkpoint.
    pub fn new(task_id: TaskId, sequence: u32) -> Self {
        Self {
            id: CheckpointId::new(),
            task_id,
            sequence,
            messages: vec![],
            pending_tool_calls: vec![],
            modified_files: vec![],
            working_memory: None,
            metadata: Value::Null,
            created_at: Utc::now(),
            description: None,
        }
    }

    /// Add a message to the checkpoint.
    pub fn with_message(mut self, message: ConversationMessage) -> Self {
        self.messages.push(message);
        self
    }

    /// Set messages.
    pub fn with_messages(mut self, messages: Vec<ConversationMessage>) -> Self {
        self.messages = messages;
        self
    }

    /// Add a pending tool call.
    pub fn with_pending_tool_call(mut self, tool_call: PendingToolCall) -> Self {
        self.pending_tool_calls.push(tool_call);
        self
    }

    /// Add a modified file.
    pub fn with_modified_file(mut self, file: ModifiedFile) -> Self {
        self.modified_files.push(file);
        self
    }

    /// Set working memory.
    pub fn with_working_memory(mut self, memory: impl Into<String>) -> Self {
        self.working_memory = Some(memory.into());
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// A pending tool call that hasn't been executed yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingToolCall {
    /// Tool call ID.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Tool input parameters.
    pub input: Value,
}

impl PendingToolCall {
    /// Create a new pending tool call.
    pub fn new(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input,
        }
    }
}

/// Record of a file modification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedFile {
    /// File path.
    pub path: String,
    /// Type of modification.
    pub modification_type: ModificationType,
    /// Original content hash (for verification).
    pub original_hash: Option<String>,
    /// New content hash.
    pub new_hash: Option<String>,
    /// Timestamp of modification.
    pub modified_at: DateTime<Utc>,
}

impl ModifiedFile {
    /// Record a file creation.
    pub fn created(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            modification_type: ModificationType::Created,
            original_hash: None,
            new_hash: None,
            modified_at: Utc::now(),
        }
    }

    /// Record a file edit.
    pub fn edited(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            modification_type: ModificationType::Edited,
            original_hash: None,
            new_hash: None,
            modified_at: Utc::now(),
        }
    }

    /// Record a file deletion.
    pub fn deleted(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            modification_type: ModificationType::Deleted,
            original_hash: None,
            new_hash: None,
            modified_at: Utc::now(),
        }
    }
}

/// Type of file modification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModificationType {
    Created,
    Edited,
    Deleted,
}

/// Manager for creating and loading checkpoints.
pub struct CheckpointManager {
    checkpoints: Vec<Checkpoint>,
    task_id: TaskId,
    next_sequence: u32,
}

impl CheckpointManager {
    /// Create a new checkpoint manager for a task.
    pub fn new(task_id: TaskId) -> Self {
        Self {
            checkpoints: vec![],
            task_id,
            next_sequence: 0,
        }
    }

    /// Create a new checkpoint.
    pub fn create_checkpoint(&mut self) -> Checkpoint {
        let checkpoint = Checkpoint::new(self.task_id.clone(), self.next_sequence);
        self.next_sequence += 1;
        checkpoint
    }

    /// Save a checkpoint.
    pub fn save(&mut self, checkpoint: Checkpoint) {
        self.checkpoints.push(checkpoint);
    }

    /// Get the latest checkpoint.
    pub fn latest(&self) -> Option<&Checkpoint> {
        self.checkpoints.last()
    }

    /// Get checkpoint by sequence number.
    pub fn get_by_sequence(&self, sequence: u32) -> Option<&Checkpoint> {
        self.checkpoints.iter().find(|c| c.sequence == sequence)
    }

    /// Get all checkpoints.
    pub fn all(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    /// Get checkpoint count.
    pub fn count(&self) -> usize {
        self.checkpoints.len()
    }

    /// Load checkpoints from serialized data.
    pub fn load(task_id: TaskId, checkpoints: Vec<Checkpoint>) -> TaskResult<Self> {
        let next_sequence = checkpoints
            .iter()
            .map(|c| c.sequence)
            .max()
            .map(|s| s + 1)
            .unwrap_or(0);

        Ok(Self {
            checkpoints,
            task_id,
            next_sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_creation() {
        let task_id = TaskId::new();
        let checkpoint = Checkpoint::new(task_id.clone(), 0);

        assert_eq!(checkpoint.task_id, task_id);
        assert_eq!(checkpoint.sequence, 0);
        assert!(checkpoint.messages.is_empty());
    }

    #[test]
    fn test_checkpoint_builder() {
        let task_id = TaskId::new();
        let checkpoint = Checkpoint::new(task_id, 0)
            .with_message(ConversationMessage::user("Hello"))
            .with_message(ConversationMessage::assistant("Hi there!"))
            .with_working_memory("User wants to fix a bug")
            .with_description("Initial conversation");

        assert_eq!(checkpoint.messages.len(), 2);
        assert!(checkpoint.working_memory.is_some());
        assert!(checkpoint.description.is_some());
    }

    #[test]
    fn test_checkpoint_manager() {
        let task_id = TaskId::new();
        let mut manager = CheckpointManager::new(task_id);

        let cp1 = manager.create_checkpoint()
            .with_message(ConversationMessage::user("First"));
        manager.save(cp1);

        let cp2 = manager.create_checkpoint()
            .with_message(ConversationMessage::user("Second"));
        manager.save(cp2);

        assert_eq!(manager.count(), 2);
        assert_eq!(manager.latest().unwrap().sequence, 1);
        assert_eq!(manager.get_by_sequence(0).unwrap().sequence, 0);
    }

    #[test]
    fn test_modified_file() {
        let created = ModifiedFile::created("src/new.rs");
        assert_eq!(created.modification_type, ModificationType::Created);

        let edited = ModifiedFile::edited("src/main.rs");
        assert_eq!(edited.modification_type, ModificationType::Edited);

        let deleted = ModifiedFile::deleted("src/old.rs");
        assert_eq!(deleted.modification_type, ModificationType::Deleted);
    }

    #[test]
    fn test_pending_tool_call() {
        let tool_call = PendingToolCall::new(
            "call_123",
            "read_file",
            serde_json::json!({"path": "src/main.rs"}),
        );

        assert_eq!(tool_call.name, "read_file");
        assert_eq!(tool_call.input["path"], "src/main.rs");
    }
}
