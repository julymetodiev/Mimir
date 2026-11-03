//! Search command: find symbols by pattern.

use anyhow::Result;
use mimir_client::Client;
use mimir_protocol::SearchOptions;

/// Lines of a symbol shown with `--code`.
const PREVIEW_LINES: u32 = 5;

/// Execute the search command.
pub async fn execute(
    pattern: &str,
    kind: Option<&str>,
    limit: usize,
    json: bool,
    show_code: bool,
) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let results = client
        .search(SearchOptions {
            pattern: pattern.to_string(),
            kind_filter: kind.map(str::to_string),
            limit: Some(limit as u32),
        })
        .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }
    if results.symbols.is_empty() {
        println!("No symbols found matching '{pattern}'");
        return Ok(());
    }

    println!(
        "Found {} symbols matching '{pattern}':\n",
        results.symbols.len()
    );
    for (i, symbol) in results.symbols.iter().enumerate() {
        let name = match &symbol.qualified_name {
            Some(qualified) if qualified != &symbol.name => {
                format!("{} ({qualified})", symbol.name)
            }
            _ => symbol.name.clone(),
        };
        println!(
            "{:3}. {name} [{}] - {}:{}",
            i + 1,
            symbol.kind,
            symbol.file_path.display(),
            symbol.start_line
        );
        if show_code {
            let source = client.source(symbol.id, Some(PREVIEW_LINES)).await?;
            println!("     ```");
            for (offset, line) in source.lines.iter().enumerate() {
                println!("     {:4}│ {line}", source.first_line as usize + offset);
            }
            if source.truncated {
                println!("     ...");
            }
            println!("     ```");
        }
    }
    println!("\nTotal: {} matches", results.total_count);
    Ok(())
}
