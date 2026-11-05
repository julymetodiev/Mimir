//! Status command.

use anyhow::Result;
use latt_client::Client;

/// Execute the status command.
pub async fn execute() -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let stats = client.stats().await?;
    println!("Project: {}", client.root().display());
    println!("Files: {}", stats.files_count);
    println!("Symbols: {}", stats.symbols_count);
    println!("Edges: {}", stats.references_count);
    for note in &stats.notes {
        println!("Note: {note}");
    }
    Ok(())
}
