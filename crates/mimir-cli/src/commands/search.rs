//! Search command - search for symbols by pattern.

use anyhow::{Context, Result};
use mimir_protocol::SearchOptions;
use regex::Regex;

use crate::app::{Config, find_project_root};
use crate::daemon::{DaemonClient, check_daemon_running};

/// Execute the search command.
pub async fn execute(
    pattern: &str,
    kind: Option<&str>,
    limit: usize,
    json: bool,
    show_code: bool,
) -> Result<()> {
    // Find project root
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    // Check if daemon is running
    if !check_daemon_running(&config.agent_dir) {
        anyhow::bail!("Daemon not running. Start with 'mimir daemon start'.");
    }

    // Validate regex pattern
    let _regex = Regex::new(pattern).context("Invalid regex pattern")?;

    // Execute via daemon
    let mut client = DaemonClient::connect(&config.agent_dir).await?;

    let options = SearchOptions {
        pattern: pattern.to_string(),
        kind_filter: kind.map(|s| s.to_string()),
        limit: Some(limit as u32),
    };

    let results = client.search(options).await?;

    // Output results
    if json {
        let output = serde_json::to_string_pretty(&results)?;
        println!("{}", output);
    } else {
        if results.symbols.is_empty() {
            println!("No symbols found matching '{}'", pattern);
            return Ok(());
        }

        println!(
            "Found {} symbols matching '{}':\n",
            results.symbols.len(),
            pattern
        );

        for (i, sym) in results.symbols.iter().enumerate() {
            let kind_str = format!("{:?}", sym.kind).to_lowercase();
            let file_display = if sym.file_path.as_os_str().is_empty() {
                format!("file:{}", sym.file_id)
            } else {
                sym.file_path.display().to_string()
            };

            // Show qualified name if different from name
            let display_name = if let Some(ref qn) = sym.qualified_name {
                if qn != &sym.name {
                    format!("{} ({})", sym.name, qn)
                } else {
                    sym.name.clone()
                }
            } else {
                sym.name.clone()
            };

            println!(
                "{:3}. {} [{}] - {}:{}",
                i + 1,
                display_name,
                kind_str,
                file_display,
                sym.start_line
            );

            // Show code snippet if requested
            if show_code && !sym.file_path.as_os_str().is_empty() {
                let file_path = config.root.join(&sym.file_path);
                if let Ok(source) = std::fs::read_to_string(&file_path) {
                    let lines: Vec<&str> = source.lines().collect();
                    let start = (sym.start_line as usize).saturating_sub(1);
                    let end = (sym.end_line as usize).min(lines.len());
                    let preview_end = (start + 5).min(end); // Show max 5 lines

                    println!("     ```");
                    for (idx, line) in lines[start..preview_end].iter().enumerate() {
                        println!("     {:4}│ {}", start + idx + 1, line);
                    }
                    if preview_end < end {
                        println!("     ... ({} more lines)", end - preview_end);
                    }
                    println!("     ```");
                }
            }
        }

        println!("\nTotal: {} matches", results.total_count);
    }

    Ok(())
}
