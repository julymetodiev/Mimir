//! Integration test for apply_patch tool with Z.ai.
//!
//! Run with: ZAI_API_KEY="..." cargo run -p mimir-agent --example test_apply_patch
//!
//! This test:
//! 1. Uses the test-project directory
//! 2. Tasks the agent to add roles and permissions to UserService
//! 3. Verifies the changes with cargo check

use mimir_agent::{Agent, AgentCallback, AgentConfig};
use mimir_llm::{Completion, LlmProvider, Message, ZaiProvider};
use mimir_tasks::Task;
use mimir_tools::{DaemonGraphClient, ToolRegistry};
use std::path::PathBuf;
use std::sync::Arc;

/// Verbose callback that prints all events.
struct VerboseCallback;

impl AgentCallback for VerboseCallback {
    fn on_start(&self, task: &Task) {
        println!("\n{}", "=".repeat(60));
        println!("TASK: {}", task.goal);
        println!("{}\n", "=".repeat(60));
    }

    fn on_llm_request(&self, messages: &[Message]) {
        println!("\n[LLM REQUEST] {} messages", messages.len());
    }

    fn on_llm_response(&self, completion: &Completion) {
        println!("[LLM RESPONSE]");
        let text = completion.text();
        if !text.is_empty() {
            // Print full response for debugging
            println!("--- Text ---");
            println!("{}", text);
            println!("------------");
        }
        println!(
            "Tokens: {} in, {} out | Stop: {:?}",
            completion.usage.input_tokens,
            completion.usage.output_tokens,
            completion.stop_reason
        );
    }

    fn on_tool_start(&self, name: &str, input: &serde_json::Value) {
        println!("\n[TOOL START] {}", name);
        // Pretty print input
        if let Ok(pretty) = serde_json::to_string_pretty(input) {
            let preview = if pretty.len() > 500 {
                format!("{}...\n[{} more chars]", &pretty[..500], pretty.len() - 500)
            } else {
                pretty
            };
            println!("Input:\n{}", preview);
        }
    }

    fn on_tool_result(&self, name: &str, result: &str, is_error: bool) {
        let status = if is_error { "❌ ERROR" } else { "✅ OK" };
        println!("\n[TOOL RESULT] {} {}", name, status);
        let preview = if result.len() > 300 {
            format!("{}...\n[{} more chars]", &result[..300], result.len() - 300)
        } else {
            result.to_string()
        };
        println!("Output:\n{}", preview);
    }

    fn on_tool_cache_hit(&self, name: &str, _input: &serde_json::Value) {
        println!("[CACHE HIT] {}", name);
    }

    fn on_complete(&self, task: &Task) {
        println!("\n{}", "=".repeat(60));
        println!("COMPLETED");
        println!("Tool calls: {}", task.tool_calls);
        println!("Tokens: {}", task.tokens_used);
        println!("{}\n", "=".repeat(60));
    }

    fn on_error(&self, error: &mimir_agent::AgentError) {
        println!("\n[ERROR] {:?}", error);
    }

    fn approve_tool(&self, name: &str, _input: &serde_json::Value) -> bool {
        println!("[APPROVE] {} -> auto-approved", name);
        true
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize from env or use provided key
    let provider = match ZaiProvider::from_env() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("ZAI_API_KEY not set in environment");
            eprintln!("Usage: ZAI_API_KEY=your-key cargo run -p mimir-agent --example test_apply_patch");
            std::process::exit(1);
        }
    };

    println!("Provider: {}", provider.name());
    println!("Model: {}", provider.default_model());
    println!("Ready: {}", provider.is_ready());

    let project_root = PathBuf::from("/Volumes/JuliusML/dev/JuliusML/Mimir/test-project");
    let agent_dir = project_root.join(".mimir");

    // Connect to daemon for graph operations
    let graph_client = match DaemonGraphClient::connect(&agent_dir, project_root.clone()).await {
        Ok(client) => {
            println!("Connected to daemon ✅");
            Some(Arc::new(client) as Arc<dyn mimir_tools::GraphClient>)
        }
        Err(e) => {
            eprintln!("Warning: Could not connect to daemon: {}", e);
            eprintln!("Search and trace tools will not work.");
            None
        }
    };

    // Create tool registry with built-in tools
    let registry = ToolRegistry::with_builtins();
    println!("Available tools: {:?}", registry.names());

    // Configure agent with the new apply_patch-focused prompt
    let config = AgentConfig::new("GLM-4.6")
        .with_max_iterations(30)
        .with_max_tokens(8192)
        .with_temperature(0.1)
        .with_auto_approve(true)
        .with_checkpoints(false)
        .with_working_dir(project_root.to_string_lossy());

    // Create agent with graph client
    let mut agent = Agent::new(provider, config, registry)
        .with_callback(Arc::new(VerboseCallback));

    if let Some(client) = graph_client {
        agent = agent.with_graph_client(client);
    }

    // Create the task
    let mut task = Task::new(
        "Добави сериализация към User struct - derive Serialize и Deserialize от serde. \
         Добави нов метод to_json() който връща String с JSON представяне на user-а. \
         Добави метод from_json(s: &str) -> Result<User, serde_json::Error> за десериализация. \
         Добави serde и serde_json зависимости към Cargo.toml."
    );
    task.working_dir = Some(project_root.to_string_lossy().to_string());

    println!("\nStarting agent...\n");

    // Run the agent
    match agent.run(&mut task).await {
        Ok(response) => {
            println!("\n{}", "=".repeat(60));
            println!("FINAL RESPONSE");
            println!("{}", "=".repeat(60));
            println!("{}", response);

            // Verify with cargo check
            println!("\n[VERIFICATION] Running cargo check...");
            let output = std::process::Command::new("cargo")
                .arg("check")
                .current_dir("/Volumes/JuliusML/dev/JuliusML/Mimir/test-project")
                .output()?;

            if output.status.success() {
                println!("✅ cargo check passed!");
            } else {
                println!("❌ cargo check failed:");
                println!("{}", String::from_utf8_lossy(&output.stderr));
            }
        }
        Err(e) => {
            println!("\n{}", "=".repeat(60));
            println!("AGENT ERROR");
            println!("{}", "=".repeat(60));
            println!("{:?}", e);
        }
    }

    Ok(())
}
