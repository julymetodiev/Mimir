//! Impact command: what a change touches, and what depends on it.

use anyhow::Result;
use latt_client::Client;

use crate::commands::trace;

/// Execute the impact command.
pub async fn execute(base: Option<String>, depth: Option<u32>, json: bool) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let impact = client.impact(base, depth).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&impact)?);
        return Ok(());
    }

    println!("Compared with: {}", impact.base);
    if !impact.changed.is_empty() {
        println!();
        println!("Changed ({}):", impact.changed.len());
    }
    for changed in &impact.changed {
        let symbol = &changed.symbol;
        let added = if changed.added { " [added]" } else { "" };
        println!(
            "  {} ({}){added}  {}:{}",
            symbol.qualified_name.as_deref().unwrap_or(&symbol.name),
            symbol.kind,
            symbol.file_path.display(),
            symbol.start_line
        );
        changed
            .dependents
            .iter()
            .for_each(|node| trace::show(node, "←"));
    }
    if !impact.removed.is_empty() {
        println!();
        println!("Removed ({}):", impact.removed.len());
    }
    for removed in &impact.removed {
        println!(
            "  {} ({})  was at {}:{}",
            removed.qualified_name,
            removed.kind,
            removed.file_path.display(),
            removed.line
        );
        for named in &removed.named_at {
            let holder = named
                .symbol
                .as_deref()
                .map(|symbol| format!(" in {symbol}"))
                .unwrap_or_default();
            println!(
                "    still named at {}:{}{holder}",
                named.path.display(),
                named.line
            );
        }
        let more = removed.named_total - removed.named_at.len();
        if more > 0 {
            println!("    and {more} more");
        }
    }
    if !impact.other_files.is_empty() {
        println!();
        println!("Other files changed ({}):", impact.other_files.len());
        for file in &impact.other_files {
            println!("  {}", file.display());
        }
    }
    if impact.provisional {
        println!();
        println!(
            "Partial: analysis is still running, so some dependents may be missing. Run this again shortly."
        );
    }
    Ok(())
}
