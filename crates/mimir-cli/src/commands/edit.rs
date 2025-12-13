//! Edit command - perform file edits with conflict detection.

use anyhow::{Context, Result};
use mimir_edit::{EditOp, Editor};
use mimir_protocol::FindOptions;
use mimir_types::Span;
use std::path::PathBuf;

use crate::app::{Config, find_project_root};
use crate::commands::sync::sync_project;
use crate::daemon::{check_daemon_running, DaemonClient};

/// Edit mode specifying the type of edit operation.
#[derive(Debug, Clone, Copy)]
pub enum EditMode {
    /// Replace a byte range.
    Replace,
    /// Insert at a position.
    Insert,
    /// Delete a byte range.
    Delete,
    /// Replace line range.
    Lines,
}

impl std::str::FromStr for EditMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "replace" => Ok(Self::Replace),
            "insert" => Ok(Self::Insert),
            "delete" => Ok(Self::Delete),
            "lines" => Ok(Self::Lines),
            _ => Err(format!(
                "Unknown edit mode: {}. Use: replace, insert, delete, lines",
                s
            )),
        }
    }
}

/// Execute the edit command.
pub async fn execute(
    file: Option<PathBuf>,
    symbol: Option<String>,
    mode: EditMode,
    start: u32,
    end: Option<u32>,
    content: Option<String>,
    preview: bool,
    backup: bool,
) -> Result<()> {
    // Verify we're in a mimir project
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    // Resolve file path and line offset based on symbol or file
    let (file_path, line_offset) = match (&symbol, &file) {
        (Some(sym_name), _) => {
            // Symbol-based editing: look up symbol via daemon
            if !check_daemon_running(&config.agent_dir) {
                anyhow::bail!("Daemon not running. Start with 'mimir daemon start' for symbol-based editing.");
            }

            let mut client = DaemonClient::connect(&config.agent_dir).await?;
            let options = FindOptions::default();
            let result = client.find_symbol(sym_name, options).await?;

            if result.symbols.is_empty() {
                anyhow::bail!("Symbol '{}' not found", sym_name);
            }

            let sym = &result.symbols[0];
            if sym.file_path.as_os_str().is_empty() {
                anyhow::bail!("Symbol '{}' has no file path. Cannot edit.", sym_name);
            }

            let resolved_path = root.join(&sym.file_path);
            println!("Resolved symbol '{}' to {}:{}", sym_name, sym.file_path.display(), sym.start_line);

            // Line offset is the symbol's start line (1-based) minus 1
            (resolved_path, sym.start_line.saturating_sub(1))
        }
        (None, Some(f)) => {
            // File-based editing: use provided file directly
            let resolved = if f.is_absolute() {
                f.clone()
            } else {
                root.join(f)
            };
            (resolved, 0)
        }
        (None, None) => {
            anyhow::bail!("Either --file or --symbol must be provided");
        }
    };

    if !file_path.exists() {
        anyhow::bail!("File not found: {}", file_path.display());
    }

    // Convert relative lines to absolute lines when using symbol
    let (abs_start, abs_end) = if symbol.is_some() {
        let abs_start = start + line_offset;
        let abs_end = end.map(|e| e + line_offset);
        (abs_start, abs_end)
    } else {
        (start, end)
    };

    // Build the edit operation
    let op = match mode {
        EditMode::Replace => {
            let end_val = abs_end.context("Replace mode requires --end parameter")?;
            let content = content.context("Replace mode requires --content parameter")?;
            EditOp::replace(&file_path, Span::new(abs_start, end_val), content)
        }
        EditMode::Insert => {
            let content = content.context("Insert mode requires --content parameter")?;
            EditOp::insert(&file_path, abs_start, content)
        }
        EditMode::Delete => {
            let end_val = abs_end.context("Delete mode requires --end parameter")?;
            EditOp::delete(&file_path, Span::new(abs_start, end_val))
        }
        EditMode::Lines => {
            let end_val = abs_end.context("Lines mode requires --end parameter")?;
            let content = content.context("Lines mode requires --content parameter")?;
            EditOp::replace_lines(&file_path, abs_start, end_val, content)
        }
    };

    // Create editor
    let editor = if backup {
        Editor::new().with_backups()
    } else {
        Editor::new()
    };

    if preview {
        // Preview mode - show diff without applying
        let diff = editor.preview(&op).await?;
        println!("Preview of changes to: {}", file_path.display());
        println!("{}", diff.unified);
    } else {
        // Apply the edit
        let result = editor.apply(&op).await?;
        println!("Edited: {}", result.file_path.display());
        println!(
            "  Bytes: {} → {} ({:+})",
            result.old_hash.to_hex()[..8].to_string(),
            result.new_hash.to_hex()[..8].to_string(),
            result.bytes_changed
        );
        println!(
            "  Lines: {} modified (net: {:+})",
            result.lines_modified,
            result.lines_changed
        );

        // Auto-sync after edit if enabled (default: true)
        if config.sync.sync_on_edit {
            let (_graph, _ids, _file_ids, stats) = sync_project(&config, false).await?;
            println!("  Graph synced: {} symbols", stats.symbols_count);
        }
    }

    Ok(())
}

/// Execute batch edit from JSON input.
pub async fn execute_batch(json_input: &str, preview: bool, backup: bool) -> Result<()> {
    let ops: Vec<EditOp> =
        serde_json::from_str(json_input).context("Failed to parse JSON edit operations")?;

    if ops.is_empty() {
        anyhow::bail!("No edit operations provided");
    }

    let editor = if backup {
        Editor::new().with_backups()
    } else {
        Editor::new()
    };

    if preview {
        for op in &ops {
            let diff = editor.preview(op).await?;
            println!("--- {} ---", op.file_path().display());
            println!("{}", diff.unified);
        }
    } else {
        let results = editor.apply_batch(&ops).await?;
        println!("Applied {} edits:", results.len());
        for result in results {
            println!(
                "  {} ({:+} bytes, {:+} lines)",
                result.file_path.display(),
                result.bytes_changed,
                result.lines_changed
            );
        }
    }

    Ok(())
}
