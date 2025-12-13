//! Test the agent with Z.ai GLM-4.6.
//!
//! Run with: ZAI_API_KEY=your-key cargo run -p mimir-agent --example test_agent

use mimir_agent::{Agent, AgentCallback, AgentConfig};
use mimir_llm::{Completion, ContentBlock, LlmProvider, Message, ZaiProvider};
use mimir_tasks::Task;
use mimir_tools::ToolRegistry;
use std::sync::Arc;

/// Simple callback that prints events.
struct PrintCallback;

impl AgentCallback for PrintCallback {
    fn on_start(&self, task: &Task) {
        println!("\n=== Starting task: {} ===\n", task.goal);
    }

    fn on_llm_request(&self, messages: &[Message]) {
        println!("[LLM] Sending {} messages", messages.len());
    }

    fn on_llm_response(&self, completion: &Completion) {
        let text = completion.text();
        if !text.is_empty() {
            println!("[LLM] Response: {}", &text[..text.len().min(200)]);
            if text.len() > 200 {
                println!("... ({} more chars)", text.len() - 200);
            }
        }
        println!(
            "[LLM] Tokens: {} in, {} out",
            completion.usage.input_tokens, completion.usage.output_tokens
        );
    }

    fn on_tool_start(&self, name: &str, input: &serde_json::Value) {
        println!("[TOOL] Executing: {} with {:?}", name, input);
    }

    fn on_tool_result(&self, name: &str, result: &str, is_error: bool) {
        let status = if is_error { "ERROR" } else { "OK" };
        let preview = &result[..result.len().min(100)];
        println!("[TOOL] {}: {} = {}", status, name, preview);
        if result.len() > 100 {
            println!("... ({} more chars)", result.len() - 100);
        }
    }

    fn on_complete(&self, task: &Task) {
        println!("\n=== Task completed ===");
        println!("Tool calls: {}", task.tool_calls);
        println!("Tokens used: {}", task.tokens_used);
    }

    fn approve_tool(&self, _name: &str, _input: &serde_json::Value) -> bool {
        true // Auto-approve for testing
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = ZaiProvider::from_env()?;

    println!("Provider: {}", provider.name());
    println!("Model: {}", provider.default_model());

    // Create tool registry with built-in tools
    let registry = ToolRegistry::with_builtins();

    // Configure agent
    let config = AgentConfig::new("GLM-4.6")
        .with_max_iterations(5)
        .with_max_tokens(4096)
        .with_auto_approve(true)
        .with_checkpoints(false); // Disable for simple test

    // Create agent
    let agent = Agent::new(provider, config, registry)
        .with_callback(Arc::new(PrintCallback));

    // Create a simple task
    let mut task = Task::new("List the files in the current directory and tell me what project this is");

    // Run the agent
    match agent.run(&mut task).await {
        Ok(response) => {
            println!("\n=== Final Response ===\n{}", response);
        }
        Err(e) => {
            println!("\n=== Error ===\n{:?}", e);
        }
    }

    Ok(())
}
