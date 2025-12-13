//! Task definitions and state management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique task identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    /// Generate a new unique task ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create from string.
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task is pending execution.
    Pending,
    /// Task is currently running.
    Running,
    /// Task is paused (can be resumed).
    Paused,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
    /// Task was cancelled.
    Cancelled,
}

impl TaskStatus {
    /// Check if task is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Check if task can be resumed.
    pub fn can_resume(&self) -> bool {
        matches!(self, Self::Paused | Self::Pending)
    }
}

/// Task priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Normal,
    High,
    Critical,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// A task for the agent to execute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier.
    pub id: TaskId,

    /// Human-readable goal/description.
    pub goal: String,

    /// Additional context for the task.
    pub context: Option<String>,

    /// Current status.
    pub status: TaskStatus,

    /// Priority level.
    pub priority: TaskPriority,

    /// Parent task ID (for subtasks).
    pub parent_id: Option<TaskId>,

    /// Working directory for the task.
    pub working_dir: Option<String>,

    /// Files relevant to this task.
    pub relevant_files: Vec<String>,

    /// Tags for categorization.
    pub tags: Vec<String>,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,

    /// Completion timestamp.
    pub completed_at: Option<DateTime<Utc>>,

    /// Error message if failed.
    pub error: Option<String>,

    /// Number of tool calls made.
    pub tool_calls: u32,

    /// Total tokens used.
    pub tokens_used: u32,
}

impl Task {
    /// Create a new task with a goal.
    pub fn new(goal: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: TaskId::new(),
            goal: goal.into(),
            context: None,
            status: TaskStatus::Pending,
            priority: TaskPriority::Normal,
            parent_id: None,
            working_dir: None,
            relevant_files: vec![],
            tags: vec![],
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
            tool_calls: 0,
            tokens_used: 0,
        }
    }

    /// Set context.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set parent task.
    pub fn with_parent(mut self, parent_id: TaskId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Set working directory.
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Add relevant files.
    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.relevant_files = files;
        self
    }

    /// Add tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Mark task as running.
    pub fn start(&mut self) {
        self.status = TaskStatus::Running;
        self.updated_at = Utc::now();
    }

    /// Pause the task.
    pub fn pause(&mut self) {
        self.status = TaskStatus::Paused;
        self.updated_at = Utc::now();
    }

    /// Complete the task successfully.
    pub fn complete(&mut self) {
        self.status = TaskStatus::Completed;
        self.updated_at = Utc::now();
        self.completed_at = Some(Utc::now());
    }

    /// Mark task as failed.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = TaskStatus::Failed;
        self.error = Some(error.into());
        self.updated_at = Utc::now();
        self.completed_at = Some(Utc::now());
    }

    /// Cancel the task.
    pub fn cancel(&mut self) {
        self.status = TaskStatus::Cancelled;
        self.updated_at = Utc::now();
        self.completed_at = Some(Utc::now());
    }

    /// Record a tool call.
    pub fn record_tool_call(&mut self, tokens: u32) {
        self.tool_calls += 1;
        self.tokens_used += tokens;
        self.updated_at = Utc::now();
    }

    /// Get task duration.
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.created_at)
    }
}

/// Builder for creating tasks.
#[derive(Debug, Default)]
pub struct TaskBuilder {
    goal: String,
    context: Option<String>,
    priority: TaskPriority,
    parent_id: Option<TaskId>,
    working_dir: Option<String>,
    relevant_files: Vec<String>,
    tags: Vec<String>,
}

impl TaskBuilder {
    /// Create a new task builder.
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            ..Default::default()
        }
    }

    /// Set context.
    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Set priority.
    pub fn priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set parent task.
    pub fn parent(mut self, parent_id: TaskId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Set working directory.
    pub fn working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Add a relevant file.
    pub fn file(mut self, file: impl Into<String>) -> Self {
        self.relevant_files.push(file.into());
        self
    }

    /// Add multiple relevant files.
    pub fn files(mut self, files: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.relevant_files.extend(files.into_iter().map(Into::into));
        self
    }

    /// Add a tag.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Build the task.
    pub fn build(self) -> Task {
        let mut task = Task::new(self.goal);
        task.context = self.context;
        task.priority = self.priority;
        task.parent_id = self.parent_id;
        task.working_dir = self.working_dir;
        task.relevant_files = self.relevant_files;
        task.tags = self.tags;
        task
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let task = Task::new("Fix the bug in auth module");
        assert_eq!(task.goal, "Fix the bug in auth module");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.priority, TaskPriority::Normal);
    }

    #[test]
    fn test_task_builder() {
        let task = TaskBuilder::new("Implement feature X")
            .context("User requested in issue #123")
            .priority(TaskPriority::High)
            .working_dir("/project")
            .file("src/main.rs")
            .file("src/lib.rs")
            .tag("feature")
            .build();

        assert_eq!(task.goal, "Implement feature X");
        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.relevant_files.len(), 2);
        assert!(task.context.is_some());
    }

    #[test]
    fn test_task_lifecycle() {
        let mut task = Task::new("Test task");

        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.status.can_resume());

        task.start();
        assert_eq!(task.status, TaskStatus::Running);

        task.pause();
        assert_eq!(task.status, TaskStatus::Paused);
        assert!(task.status.can_resume());

        task.start();
        task.complete();
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.status.is_terminal());
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_task_failure() {
        let mut task = Task::new("Failing task");
        task.start();
        task.fail("Something went wrong");

        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.error, Some("Something went wrong".to_string()));
        assert!(task.status.is_terminal());
    }

    #[test]
    fn test_task_metrics() {
        let mut task = Task::new("Metered task");
        task.start();
        task.record_tool_call(100);
        task.record_tool_call(150);

        assert_eq!(task.tool_calls, 2);
        assert_eq!(task.tokens_used, 250);
    }
}
