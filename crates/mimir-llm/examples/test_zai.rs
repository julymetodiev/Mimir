//! Test Z.ai GLM-4.6 API connection.
//!
//! Run with: ZAI_API_KEY=your-key cargo run -p mimir-llm --example test_zai

use mimir_llm::{CompletionRequest, LlmProvider, Message, ZaiProvider};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = ZaiProvider::from_env()?;

    println!("Testing Z.ai GLM-4.6...");
    println!("Provider: {}", provider.name());
    println!("Default model: {}", provider.default_model());
    println!();

    let request = CompletionRequest::new(
        "GLM-4.6",
        vec![Message::user("Say 'Hello from GLM-4.6!' and nothing else.")],
    )
    .with_max_tokens(100);

    println!("Sending request...");

    match provider.complete(request).await {
        Ok(response) => {
            println!("Success!");
            println!("Model: {}", response.model);
            println!("Response: {}", response.text());
            println!(
                "Tokens: {} input, {} output",
                response.usage.input_tokens, response.usage.output_tokens
            );
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    Ok(())
}
