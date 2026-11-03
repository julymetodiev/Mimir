//! Read command - read source code by symbol name.

use anyhow::{Context, Result};
use mimir_protocol::FindOptions;
use serde::Serialize;

use crate::app::{find_project_root, Config};
use crate::daemon::{check_daemon_running, DaemonClient};

/// Output structure for JSON mode.
#[derive(Serialize)]
struct ReadOutput {
    symbol: String,
    kind: String,
    file: String,
    start_line: u32,
    end_line: u32,
    content: String,
    lines: Vec<LineOutput>,
}

#[derive(Serialize)]
struct LineOutput {
    number: u32,
    content: String,
    is_symbol: bool,
}

/// Execute the read command.
pub async fn execute(
    symbol_name: &str,
    before: usize,
    after: usize,
    show_numbers: bool,
    json: bool,
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

    // Find symbol via daemon - get multiple results for priority selection
    let mut client = DaemonClient::connect(&config.agent_dir).await?;
    let options = FindOptions {
        limit: Some(100),
        ..Default::default()
    };
    let result = client.find_symbol(symbol_name, options).await?;

    if result.symbols.is_empty() {
        anyhow::bail!("Symbol '{}' not found", symbol_name);
    }

    // Select best match with priority: impl > struct > function > enum_variant
    let sym = result
        .symbols
        .iter()
        .filter(|s| s.name == symbol_name) // Exact name match
        .max_by_key(|s| {
            let kind = format!("{:?}", s.kind).to_lowercase();
            match kind.as_str() {
                "impl" => 100,
                "struct" | "class" => 90,
                "function" => 80,
                "enum" | "trait" | "interface" => 70,
                "enumvariant" | "enum_variant" => 10,
                _ => 60,
            }
        })
        .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found with exact name match", symbol_name))?;
    if sym.file_path.as_os_str().is_empty() {
        anyhow::bail!("Symbol '{}' has no file path", symbol_name);
    }

    // Read the source file
    let file_path = root.join(&sym.file_path);
    let source = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read {}", file_path.display()))?;

    let lines: Vec<&str> = source.lines().collect();
    let total_lines = lines.len();

    // Calculate range with context
    let sym_start = (sym.start_line as usize).saturating_sub(1);
    let sym_end = (sym.end_line as usize).min(total_lines);

    let display_start = sym_start.saturating_sub(before);
    let display_end = (sym_end + after).min(total_lines);

    if json {
        // JSON output
        let mut line_outputs = Vec::new();
        for (i, line) in lines.iter().enumerate().take(display_end).skip(display_start) {
            line_outputs.push(LineOutput {
                number: (i + 1) as u32,
                content: line.to_string(),
                is_symbol: i >= sym_start && i < sym_end,
            });
        }

        let output = ReadOutput {
            symbol: sym.name.clone(),
            kind: format!("{:?}", sym.kind),
            file: sym.file_path.display().to_string(),
            start_line: sym.start_line,
            end_line: sym.end_line,
            content: lines[sym_start..sym_end].join("\n"),
            lines: line_outputs,
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Human-readable output
        let kind_str = format!("{:?}", sym.kind).to_lowercase();
        println!(
            "{}  [{}]  {}:{}-{}",
            sym.name,
            kind_str,
            sym.file_path.display(),
            sym.start_line,
            sym.end_line
        );
        println!();

        for (i, line) in lines.iter().enumerate().take(display_end).skip(display_start) {
            let line_num = i + 1;
            let is_symbol_line = i >= sym_start && i < sym_end;

            // Marker for context vs symbol lines
            let marker = if !is_symbol_line { "·" } else { " " };

            if show_numbers {
                println!("{}{:4}│ {}", marker, line_num, line);
            } else {
                println!("{} {}", marker, line);
            }
        }
    }

    Ok(())
}
