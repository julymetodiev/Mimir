//! Sync command: bring the graph up to date and wait until nothing about it is provisional.

use anyhow::Result;
use latt_client::Client;

/// Execute the sync command.
pub async fn execute(full: bool, json: bool) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    println!("Syncing project: {}", client.root().display());
    let result = client.sync(full, json).await?;
    let stats = client.stats().await?;
    println!(
        "Files: {} added, {} updated, {} removed",
        result.files_added, result.files_updated, result.files_removed
    );
    println!(
        "Indexed {} symbols, {} edges",
        stats.symbols_count, stats.references_count
    );
    if let Some(link) = result.link {
        println!(
            "Calls: {} semantic ({} files), {} from syntax only ({} files)",
            link.semantic_calls, link.semantic_files, link.syntax_calls, link.syntax_files
        );
        if link.live_files > 0 {
            println!(
                "Edited since the index: {} files, from a running analyzer (`latt sync --full` rewrites the index)",
                link.live_files
            );
        }
        println!("Trait method implementations: {}", link.implementations);
        println!("Functions used as values: {}", link.function_values);
        println!("Uses of constants: {}", link.constant_uses);
        println!("Uses of types: {}", link.type_uses);
        println!("Uses of fields: {}", link.field_uses);
    }
    for note in &result.notes {
        println!("Note: {note}");
    }
    if let Some(path) = result.saved_to {
        println!("Graph saved to {}", path.display());
    }
    Ok(())
}
