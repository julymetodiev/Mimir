//! Persistent storage for tasks and checkpoints.

use crate::{Checkpoint, Task, TaskError, TaskId, TaskResult, TaskStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// In-memory task store (for testing and simple use cases).
#[derive(Debug, Default)]
pub struct MemoryTaskStore {
    tasks: HashMap<String, Task>,
    checkpoints: HashMap<String, Vec<Checkpoint>>,
}

impl MemoryTaskStore {
    /// Create a new in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Save a task.
    pub fn save_task(&mut self, task: Task) -> TaskResult<()> {
        self.tasks.insert(task.id.0.clone(), task);
        Ok(())
    }

    /// Get a task by ID.
    pub fn get_task(&self, id: &TaskId) -> TaskResult<&Task> {
        self.tasks
            .get(&id.0)
            .ok_or_else(|| TaskError::NotFound(id.0.clone()))
    }

    /// Get a mutable task by ID.
    pub fn get_task_mut(&mut self, id: &TaskId) -> TaskResult<&mut Task> {
        self.tasks
            .get_mut(&id.0)
            .ok_or_else(|| TaskError::NotFound(id.0.clone()))
    }

    /// List all tasks.
    pub fn list_tasks(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    /// List tasks by status.
    pub fn list_by_status(&self, status: TaskStatus) -> Vec<&Task> {
        self.tasks
            .values()
            .filter(|t| t.status == status)
            .collect()
    }

    /// Delete a task.
    pub fn delete_task(&mut self, id: &TaskId) -> TaskResult<Task> {
        self.checkpoints.remove(&id.0);
        self.tasks
            .remove(&id.0)
            .ok_or_else(|| TaskError::NotFound(id.0.clone()))
    }

    /// Save a checkpoint.
    pub fn save_checkpoint(&mut self, checkpoint: Checkpoint) -> TaskResult<()> {
        let task_id = checkpoint.task_id.0.clone();
        self.checkpoints
            .entry(task_id)
            .or_default()
            .push(checkpoint);
        Ok(())
    }

    /// Get checkpoints for a task.
    pub fn get_checkpoints(&self, task_id: &TaskId) -> Vec<&Checkpoint> {
        self.checkpoints
            .get(&task_id.0)
            .map(|cps| cps.iter().collect())
            .unwrap_or_default()
    }

    /// Get latest checkpoint for a task.
    pub fn get_latest_checkpoint(&self, task_id: &TaskId) -> Option<&Checkpoint> {
        self.checkpoints
            .get(&task_id.0)
            .and_then(|cps| cps.last())
    }
}

/// File-based task store for persistence.
pub struct FileTaskStore {
    base_dir: PathBuf,
}

impl FileTaskStore {
    /// Create a new file-based store.
    pub fn new(base_dir: impl AsRef<Path>) -> TaskResult<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_dir)?;
        std::fs::create_dir_all(base_dir.join("tasks"))?;
        std::fs::create_dir_all(base_dir.join("checkpoints"))?;
        Ok(Self { base_dir })
    }

    fn task_path(&self, id: &TaskId) -> PathBuf {
        self.base_dir.join("tasks").join(format!("{}.json", id.0))
    }

    fn checkpoints_dir(&self, task_id: &TaskId) -> PathBuf {
        self.base_dir.join("checkpoints").join(&task_id.0)
    }

    /// Save a task.
    pub fn save_task(&self, task: &Task) -> TaskResult<()> {
        let path = self.task_path(&task.id);
        let json = serde_json::to_string_pretty(task)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a task by ID.
    pub fn load_task(&self, id: &TaskId) -> TaskResult<Task> {
        let path = self.task_path(id);
        if !path.exists() {
            return Err(TaskError::NotFound(id.0.clone()));
        }
        let json = std::fs::read_to_string(path)?;
        let task: Task = serde_json::from_str(&json)?;
        Ok(task)
    }

    /// List all tasks.
    pub fn list_tasks(&self) -> TaskResult<Vec<Task>> {
        let tasks_dir = self.base_dir.join("tasks");
        let mut tasks = vec![];

        for entry in std::fs::read_dir(tasks_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let json = std::fs::read_to_string(&path)?;
                let task: Task = serde_json::from_str(&json)?;
                tasks.push(task);
            }
        }

        // Sort by creation time (newest first)
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }

    /// Delete a task.
    pub fn delete_task(&self, id: &TaskId) -> TaskResult<()> {
        let task_path = self.task_path(id);
        if task_path.exists() {
            std::fs::remove_file(task_path)?;
        }

        let checkpoints_dir = self.checkpoints_dir(id);
        if checkpoints_dir.exists() {
            std::fs::remove_dir_all(checkpoints_dir)?;
        }

        Ok(())
    }

    /// Save a checkpoint.
    pub fn save_checkpoint(&self, checkpoint: &Checkpoint) -> TaskResult<()> {
        let dir = self.checkpoints_dir(&checkpoint.task_id);
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(format!("{:04}.json", checkpoint.sequence));
        let json = serde_json::to_string_pretty(checkpoint)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load all checkpoints for a task.
    pub fn load_checkpoints(&self, task_id: &TaskId) -> TaskResult<Vec<Checkpoint>> {
        let dir = self.checkpoints_dir(task_id);
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut checkpoints = vec![];
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let json = std::fs::read_to_string(&path)?;
                let checkpoint: Checkpoint = serde_json::from_str(&json)?;
                checkpoints.push(checkpoint);
            }
        }

        // Sort by sequence
        checkpoints.sort_by_key(|c| c.sequence);
        Ok(checkpoints)
    }

    /// Load latest checkpoint for a task.
    pub fn load_latest_checkpoint(&self, task_id: &TaskId) -> TaskResult<Option<Checkpoint>> {
        let checkpoints = self.load_checkpoints(task_id)?;
        Ok(checkpoints.into_iter().last())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store_tasks() {
        let mut store = MemoryTaskStore::new();

        let task = Task::new("Test task");
        let task_id = task.id.clone();

        store.save_task(task).unwrap();

        let loaded = store.get_task(&task_id).unwrap();
        assert_eq!(loaded.goal, "Test task");

        let tasks = store.list_tasks();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn test_memory_store_checkpoints() {
        let mut store = MemoryTaskStore::new();

        let task = Task::new("Test task");
        let task_id = task.id.clone();
        store.save_task(task).unwrap();

        let cp1 = Checkpoint::new(task_id.clone(), 0);
        let cp2 = Checkpoint::new(task_id.clone(), 1);

        store.save_checkpoint(cp1).unwrap();
        store.save_checkpoint(cp2).unwrap();

        let checkpoints = store.get_checkpoints(&task_id);
        assert_eq!(checkpoints.len(), 2);

        let latest = store.get_latest_checkpoint(&task_id).unwrap();
        assert_eq!(latest.sequence, 1);
    }

    #[test]
    fn test_memory_store_list_by_status() {
        let mut store = MemoryTaskStore::new();

        let mut task1 = Task::new("Task 1");
        task1.start();
        store.save_task(task1).unwrap();

        let mut task2 = Task::new("Task 2");
        task2.start();
        task2.complete();
        store.save_task(task2).unwrap();

        let task3 = Task::new("Task 3");
        store.save_task(task3).unwrap();

        let running = store.list_by_status(TaskStatus::Running);
        assert_eq!(running.len(), 1);

        let completed = store.list_by_status(TaskStatus::Completed);
        assert_eq!(completed.len(), 1);

        let pending = store.list_by_status(TaskStatus::Pending);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn test_file_store() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = FileTaskStore::new(temp_dir.path()).unwrap();

        // Save task
        let task = Task::new("Persistent task");
        let task_id = task.id.clone();
        store.save_task(&task).unwrap();

        // Load task
        let loaded = store.load_task(&task_id).unwrap();
        assert_eq!(loaded.goal, "Persistent task");

        // Save checkpoints
        let cp1 = Checkpoint::new(task_id.clone(), 0);
        let cp2 = Checkpoint::new(task_id.clone(), 1);
        store.save_checkpoint(&cp1).unwrap();
        store.save_checkpoint(&cp2).unwrap();

        // Load checkpoints
        let checkpoints = store.load_checkpoints(&task_id).unwrap();
        assert_eq!(checkpoints.len(), 2);

        // Load latest
        let latest = store.load_latest_checkpoint(&task_id).unwrap().unwrap();
        assert_eq!(latest.sequence, 1);

        // List tasks
        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);

        // Delete task
        store.delete_task(&task_id).unwrap();
        assert!(store.load_task(&task_id).is_err());
    }
}
