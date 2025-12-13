//! Agent command implementation.

use anyhow::{Context, Result};
use mimir_agent::{Agent, AgentCallback, AgentConfig};
use mimir_llm::{Completion, LlmProvider, Message, ZaiProvider};
use mimir_tasks::{Checkpoint, FileTaskStore, Task, TaskStatus};
use mimir_tools::ToolRegistry;
use std::path::Path;
use std::sync::Arc;

/// Callback that prints agent progress to stdout.
struct CliCallback {
    verbose: bool,
}

impl CliCallback {
    fn new(verbose: bool) -> Self {
        Self { verbose }
    }
}

impl AgentCallback for CliCallback {
    fn on_start(&self, task: &Task) {
        println!("\n🤖 Starting: {}\n", task.goal);
    }

    fn on_llm_request(&self, messages: &[Message]) {
        if self.verbose {
            println!("📤 Sending {} messages to LLM...", messages.len());
        }
    }

    fn on_llm_response(&self, completion: &Completion) {
        if self.verbose {
            println!(
                "📥 Response: {} tokens in, {} tokens out",
                completion.usage.input_tokens, completion.usage.output_tokens
            );
        }
    }

    fn on_tool_start(&self, name: &str, input: &serde_json::Value) {
        if self.verbose {
            println!("🔧 {} {:?}", name, input);
        } else {
            println!("🔧 {}", name);
        }
    }

    fn on_tool_result(&self, name: &str, result: &str, is_error: bool) {
        if is_error {
            println!("❌ {} failed: {}", name, &result[..result.len().min(100)]);
        } else if self.verbose {
            let preview = &result[..result.len().min(200)];
            println!("✅ {} = {}", name, preview);
            if result.len() > 200 {
                println!("   ... ({} more chars)", result.len() - 200);
            }
        }
    }

    fn on_checkpoint(&self, checkpoint: &Checkpoint) {
        if self.verbose {
            println!("💾 Checkpoint {} created", checkpoint.sequence);
        }
    }

    fn on_complete(&self, task: &Task) {
        println!("\n✨ Completed in {} tool calls", task.tool_calls);
        if self.verbose {
            println!("   Total tokens: {}", task.tokens_used);
        }
    }

    fn on_error(&self, error: &mimir_agent::AgentError) {
        eprintln!("\n❌ Error: {:?}", error);
    }

    fn approve_tool(&self, _name: &str, _input: &serde_json::Value) -> bool {
        true // Auto-approve in CLI mode
    }
}

/// Run an agent task.
pub async fn run_agent(
    goal: &str,
    model: Option<&str>,
    max_iterations: u32,
    verbose: bool,
    checkpoint: bool,
) -> Result<()> {
    // Get API key from environment
    let provider = ZaiProvider::from_env()
        .context("ZAI_API_KEY environment variable not set")?;

    // Create tool registry with built-in tools
    let registry = ToolRegistry::with_builtins();

    // Configure agent
    let model = model.unwrap_or("GLM-4.6");
    let config = AgentConfig::new(model)
        .with_max_iterations(max_iterations)
        .with_max_tokens(8192)
        .with_auto_approve(true)
        .with_checkpoints(checkpoint);

    // Create agent with CLI callback
    let agent = Agent::new(provider, config, registry)
        .with_callback(Arc::new(CliCallback::new(verbose)));

    // Create task
    let mut task = Task::new(goal);
    if let Ok(cwd) = std::env::current_dir() {
        task = task.with_working_dir(cwd.to_string_lossy().to_string());
    }

    // Run the agent
    match agent.run(&mut task).await {
        Ok(response) => {
            println!("\n📝 Response:\n{}", response);
            Ok(())
        }
        Err(e) => {
            eprintln!("\n❌ Agent failed: {:?}", e);
            Err(e.into())
        }
    }
}

/// List tasks from the task store.
pub async fn list_tasks(task_dir: &Path, status: Option<TaskStatus>) -> Result<()> {
    let store = FileTaskStore::new(task_dir)
        .context("Failed to open task store")?;

    let tasks = store.list_tasks()?;

    if tasks.is_empty() {
        println!("No tasks found.");
        return Ok(());
    }

    println!("{:<36} {:<10} {:<30}", "ID", "STATUS", "GOAL");
    println!("{}", "-".repeat(80));

    for task in tasks {
        if let Some(filter_status) = status {
            if task.status != filter_status {
                continue;
            }
        }

        let status_str = match task.status {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Paused => "paused",
            TaskStatus::Completed => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        };

        let goal_preview = if task.goal.len() > 28 {
            format!("{}...", &task.goal[..25])
        } else {
            task.goal.clone()
        };

        println!("{:<36} {:<10} {:<30}", task.id.0, status_str, goal_preview);
    }

    Ok(())
}

/// Resume a paused task.
pub async fn resume_task(
    task_id: &str,
    task_dir: &Path,
    verbose: bool,
) -> Result<()> {
    let store = FileTaskStore::new(task_dir)
        .context("Failed to open task store")?;

    let task_id = mimir_tasks::TaskId::from_string(task_id);
    let mut task = store.load_task(&task_id)
        .context("Task not found")?;

    if !task.status.can_resume() {
        anyhow::bail!(
            "Cannot resume task with status {:?}. Only pending or paused tasks can be resumed.",
            task.status
        );
    }

    // Load latest checkpoint if available
    let checkpoint = store.load_latest_checkpoint(&task_id)?;

    // Get API key from environment
    let provider = ZaiProvider::from_env()
        .context("ZAI_API_KEY environment variable not set")?;

    // Create tool registry
    let registry = ToolRegistry::with_builtins();

    // Configure agent
    let config = AgentConfig::new("GLM-4.6")
        .with_max_iterations(50)
        .with_max_tokens(8192)
        .with_auto_approve(true)
        .with_checkpoints(true);

    // Create agent with CLI callback
    let agent = Agent::new(provider, config, registry)
        .with_callback(Arc::new(CliCallback::new(verbose)));

    // Resume or run
    let result = if let Some(cp) = checkpoint {
        println!("Resuming from checkpoint {}...", cp.sequence);
        agent.resume(&mut task, cp).await
    } else {
        println!("No checkpoint found, running from start...");
        agent.run(&mut task).await
    };

    match result {
        Ok(response) => {
            // Save updated task
            store.save_task(&task)?;
            println!("\n📝 Response:\n{}", response);
            Ok(())
        }
        Err(e) => {
            // Save task state even on failure
            let _ = store.save_task(&task);
            eprintln!("\n❌ Agent failed: {:?}", e);
            Err(e.into())
        }
    }
}
