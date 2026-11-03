//! Status command implementation.

use crate::app::{App, find_project_root};
use anyhow::{Result, bail};
use mimir_files::Scanner;
use std::collections::HashMap;

/// Execute the status command.
pub async fn execute() -> Result<()> {
    // Find project root
    let root = match find_project_root() {
        Some(r) => r,
        None => bail!("Not in a Mimir project. Run 'mimir init' first."),
    };

    let app = App::with_root(&root)?;
    let config = &app.config;

    if !config.is_initialized() {
        bail!("Mimir is not initialized. Run 'mimir init' first.");
    }

    // Get project name from directory
    let project_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    println!("Project: {}", project_name);
    println!("Root: {}", root.display());
    println!();

    // Count scanned code files by language
    let scanned = Scanner::new(&root).scan().await?;
    let mut language_counts: HashMap<String, usize> = HashMap::new();
    for file in &scanned {
        *language_counts
            .entry(format!("{:?}", file.language))
            .or_insert(0) += 1;
    }

    println!("Files:");
    println!("  Code files: {}", scanned.len());

    if !language_counts.is_empty() {
        let mut sorted: Vec<_> = language_counts.into_iter().collect();
        sorted.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let langs: Vec<_> = sorted
            .iter()
            .take(5)
            .map(|(lang, count)| format!("{} ({})", lang, count))
            .collect();
        println!("  Languages: {}", langs.join(", "));
    }
    println!();

    // Graph section
    let graph_file = config.graph_dir.join("graph.json");
    println!("Graph:");
    if graph_file.exists() {
        println!("  Built: {}", graph_file.display());
    } else {
        println!("  Not built. Run 'mimir sync'.");
    }

    Ok(())
}
