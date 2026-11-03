//! Grep command: search the text of the project's files, and say which symbol each match is in.

use anyhow::Result;
use mimir_client::Client;
use mimir_protocol::GrepOptions;

/// Execute the grep command.
pub async fn execute(options: GrepOptions, json: bool) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let found = client.grep(options).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
        return Ok(());
    }
    for each in &found.matches {
        let first = each.line as usize - each.before.len();
        for (offset, line) in each.before.iter().enumerate() {
            println!("{}:{}-{line}", each.path.display(), first + offset);
        }
        match &each.symbol {
            Some(symbol) => println!(
                "{}:{}:{} [in {symbol}]",
                each.path.display(),
                each.line,
                each.text
            ),
            None => println!("{}:{}:{}", each.path.display(), each.line, each.text),
        }
        for (offset, line) in each.after.iter().enumerate() {
            println!(
                "{}:{}-{line}",
                each.path.display(),
                each.line as usize + 1 + offset
            );
        }
    }
    if found.matches.len() < found.total {
        println!("Showing {} of {} matches", found.matches.len(), found.total);
    }
    for file in &found.too_large {
        println!("Not searched, too large: {}", file.display());
    }
    Ok(())
}
