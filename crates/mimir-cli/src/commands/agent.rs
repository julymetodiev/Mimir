//! Agent command implementation.

use anyhow::{Context, Result};
use mimir_agent::{Agent, AgentCallback, AgentConfig};
use mimir_llm::{Completion, Message, ZaiProvider};
use mimir_tasks::{Checkpoint, FileTaskStore, Task, TaskStatus};
use mimir_tools::ToolRegistry;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::daemon::{check_daemon_running, DaemonGraphClient};

/// Spinner animation frames (Nordic runes - mystical rotation)
const SPINNER_FRAMES: &[&str] = &["ᚠ", "ᚢ", "ᚦ", "ᚨ", "ᚱ", "ᚲ", "ᚷ", "ᚹ", "ᚺ", "ᚾ", "ᛁ", "ᛃ", "ᛇ", "ᛈ", "ᛉ", "ᛊ"];

/// Spinner controller for async operations.
struct Spinner {
    active: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    fn start(&mut self, message: &str) {
        self.active.store(true, Ordering::SeqCst);
        let active = self.active.clone();
        let msg = message.to_string();

        self.handle = Some(thread::spawn(move || {
            let mut i = 0;
            while active.load(Ordering::SeqCst) {
                print!("\r  {}  {} ", SPINNER_FRAMES[i % SPINNER_FRAMES.len()], msg);
                let _ = io::stdout().flush();
                thread::sleep(Duration::from_millis(80));
                i += 1;
            }
            // Clear the spinner line
            print!("\r{}\r", " ".repeat(60));
            let _ = io::stdout().flush();
        }));
    }

    fn stop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Callback that prints agent progress to stdout.
struct CliCallback {
    verbose: bool,
    spinner: std::sync::Mutex<Spinner>,
    last_tokens: std::sync::atomic::AtomicU32,
}

impl CliCallback {
    fn new(verbose: bool) -> Self {
        Self {
            verbose,
            spinner: std::sync::Mutex::new(Spinner::new()),
            last_tokens: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

impl AgentCallback for CliCallback {
    fn on_start(&self, task: &Task) {
        println!("\n  ᛗ  Mímir awakens\n");
        println!("  {}  {}\n", "◈", task.goal);
    }

    fn on_llm_request(&self, messages: &[Message]) {
        let msg = if self.verbose {
            format!("consulting the oracle... ({} messages)", messages.len())
        } else {
            "consulting the oracle...".to_string()
        };
        if let Ok(mut spinner) = self.spinner.lock() {
            spinner.start(&msg);
        }
    }

    fn on_llm_response(&self, completion: &Completion) {
        if let Ok(mut spinner) = self.spinner.lock() {
            spinner.stop();
        }
        // Store tokens to display with tool action
        let total = completion.usage.total_tokens();
        self.last_tokens.store(total, Ordering::SeqCst);
    }

    fn on_tool_start(&self, name: &str, input: &serde_json::Value) {
        let (rune, desc) = format_tool_action(name, input);
        let tokens = self.last_tokens.load(Ordering::SeqCst);
        println!("  {}  {} ({} tokens)", rune, desc, tokens);

        // In verbose mode, show edit_batch contents
        if self.verbose && name == "edit_batch" {
            if let Some(edits) = input.get("edits").and_then(|v| v.as_array()) {
                for edit in edits {
                    if let Some(symbol) = edit.get("symbol").and_then(|v| v.as_str()) {
                        let content_len = edit.get("content")
                            .and_then(|v| v.as_str())
                            .map(|s| s.len())
                            .unwrap_or(0);
                        println!("       → {} ({} chars)", symbol, content_len);
                    }
                }
            }
        }

        // Start spinner for shell commands (they can take a while)
        if name == "shell" {
            if let Ok(mut spinner) = self.spinner.lock() {
                spinner.start("executing...");
            }
        }
    }

    fn on_tool_result(&self, name: &str, result: &str, is_error: bool) {
        // Stop spinner if it was running
        if name == "shell" {
            if let Ok(mut spinner) = self.spinner.lock() {
                spinner.stop();
            }
        }

        if is_error {
            let preview = &result[..result.len().min(80)];
            println!("  ᚦ  {} failed: {}", name, preview);
        } else if self.verbose {
            let preview = &result[..result.len().min(120)];
            println!("     → {}", preview);
            if result.len() > 120 {
                println!("       ... (+{} chars)", result.len() - 120);
            }
        }
    }

    fn on_tool_cache_hit(&self, name: &str, input: &serde_json::Value) {
        let (rune, desc) = format_tool_action(name, input);
        println!("  {}  {} (cached)", rune, desc);
    }

    fn on_checkpoint(&self, checkpoint: &Checkpoint) {
        if self.verbose {
            println!("  ᛉ  checkpoint {}", checkpoint.sequence);
        }
    }

    fn on_complete(&self, task: &Task) {
        // Make sure spinner is stopped
        if let Ok(mut spinner) = self.spinner.lock() {
            spinner.stop();
        }
        println!("\n  ᛏ  complete ({} actions, {} tokens)\n", task.tool_calls, task.tokens_used);
    }

    fn on_error(&self, error: &mimir_agent::AgentError) {
        // Make sure spinner is stopped
        if let Ok(mut spinner) = self.spinner.lock() {
            spinner.stop();
        }
        eprintln!("\n  ᚦ  {:?}", error);
    }

    fn approve_tool(&self, _name: &str, _input: &serde_json::Value) -> bool {
        true // Auto-approve in CLI mode
    }
}

/// Format tool action with Nordic rune and description.
fn format_tool_action(name: &str, input: &serde_json::Value) -> (&'static str, String) {
    match name {
        "read" | "read_file" => {
            let path = input.get("file")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            // Show just filename or last path component
            let short = path.rsplit('/').next().unwrap_or(path);
            ("ᚲ ", format!("read {}", short))
        }
        "edit" | "edit_file" => {
            // Check for file/path first, then symbol for symbol-based editing
            let target = input.get("file")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
                .or_else(|| input.get("symbol").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| "?".to_string());
            ("ᛟ ", format!("edit {}", target))
        }
        "edit_batch" => {
            let edits = input.get("edits")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            ("᛫ ", format!("edit_batch ({} edits)", edits))
        }
        "shell" => {
            let cmd = input.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            // Truncate long commands
            let preview = if cmd.len() > 50 {
                format!("{}...", &cmd[..47])
            } else {
                cmd.to_string()
            };
            ("ᛊ ", format!("shell {}", preview))
        }
        "search" | "search_symbol" => {
            let pattern = input.get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            ("ᚱ ", format!("search \"{}\"", pattern))
        }
        "trace" | "trace_symbol" => {
            let symbol = input.get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            ("ᚠ ", format!("trace {}", symbol))
        }
        _ => ("᛫ ", format!("{}", name))
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
    let mut agent = Agent::new(provider, config, registry)
        .with_callback(Arc::new(CliCallback::new(verbose)));

    // Try to connect to daemon for semantic operations
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let agent_dir = cwd.join(".mimir");

    if check_daemon_running(&agent_dir) {
        match DaemonGraphClient::connect(&agent_dir, cwd.clone()).await {
            Ok(client) => {
                if verbose {
                    println!("  ᛗ  connected to daemon\n");
                }
                agent = agent.with_graph_client(Arc::new(client));
            }
            Err(e) => {
                if verbose {
                    eprintln!("  ᚦ  daemon connection failed: {}", e);
                }
            }
        }
    } else if verbose {
        println!("  ᛗ  daemon not running (semantic tools disabled)\n");
    }

    // Create task
    let mut task = Task::new(goal);
    task = task.with_working_dir(cwd.to_string_lossy().to_string());

    // Run the agent
    match agent.run(&mut task).await {
        Ok(response) => {
            println!("  ◈  Response\n");
            println!("{}\n", response);
            Ok(())
        }
        Err(e) => {
            eprintln!("\n  ᚦ  Agent failed: {:?}", e);
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
    let mut agent = Agent::new(provider, config, registry)
        .with_callback(Arc::new(CliCallback::new(verbose)));

    // Try to connect to daemon for semantic operations
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let agent_dir = cwd.join(".mimir");

    if check_daemon_running(&agent_dir) {
        if let Ok(client) = DaemonGraphClient::connect(&agent_dir, cwd).await {
            agent = agent.with_graph_client(Arc::new(client));
        }
    }

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
            println!("  ◈  Response\n");
            println!("{}\n", response);
            Ok(())
        }
        Err(e) => {
            // Save task state even on failure
            let _ = store.save_task(&task);
            eprintln!("\n  ᚦ  Agent failed: {:?}", e);
            Err(e.into())
        }
    }
}
