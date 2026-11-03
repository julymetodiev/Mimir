//! Edit command - perform file edits with conflict detection.

use anyhow::{Context, Result};
use mimir_edit::{EditOp, Editor};
use mimir_protocol::{FindOptions, SymbolInfo};
use mimir_types::{Span, SymbolKind};
use std::path::PathBuf;

use crate::app::{Config, find_project_root};
use crate::commands::sync::sync_project;
use crate::daemon::{DaemonClient, check_daemon_running};

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
    /// Replace entire symbol (requires --symbol).
    Symbol,
}

impl std::str::FromStr for EditMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "replace" => Ok(Self::Replace),
            "insert" => Ok(Self::Insert),
            "delete" => Ok(Self::Delete),
            "lines" => Ok(Self::Lines),
            "symbol" => Ok(Self::Symbol),
            _ => Err(format!(
                "Unknown edit mode: {}. Use: replace, insert, delete, lines, symbol",
                s
            )),
        }
    }
}

/// Symbol info for edit operations.
struct SymbolLocation {
    file_path: PathBuf,
    start_line: u32,
    end_line: u32,
}

/// What the edit command was asked to do.
pub struct EditRequest<'a> {
    pub file: Option<PathBuf>,
    pub symbol: Option<&'a str>,
    pub mode: EditMode,
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub content: Option<String>,
    pub preview: bool,
    pub backup: bool,
}

/// Execute the edit command.
pub async fn execute(request: EditRequest<'_>) -> Result<()> {
    let EditRequest {
        file,
        symbol,
        mode,
        start,
        end,
        content,
        preview,
        backup,
    } = request;
    // Verify we're in a mimir project
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    // For symbol mode, we need a symbol
    if matches!(mode, EditMode::Symbol) && symbol.is_none() {
        anyhow::bail!("Symbol mode requires --symbol parameter");
    }

    // Resolve symbol location if provided
    let symbol_loc = if let Some(sym_name) = symbol {
        if !check_daemon_running(&config.agent_dir) {
            anyhow::bail!(
                "Daemon not running. Start with 'mimir daemon start' for symbol-based editing."
            );
        }

        let mut client = DaemonClient::connect(&config.agent_dir).await?;
        let options = FindOptions::default();
        let result = client.find_symbol(sym_name, options).await?;

        if result.symbols.is_empty() {
            anyhow::bail!("Symbol '{}' not found", sym_name);
        }

        let sym = select_best_symbol(&result.symbols, sym_name);
        if sym.file_path.as_os_str().is_empty() {
            anyhow::bail!("Symbol '{}' has no file path. Cannot edit.", sym_name);
        }

        let resolved_path = root.join(&sym.file_path);
        println!(
            "Resolved symbol '{}' to {}:{}-{}",
            sym_name,
            sym.file_path.display(),
            sym.start_line,
            sym.end_line
        );

        Some(SymbolLocation {
            file_path: resolved_path,
            start_line: sym.start_line,
            end_line: sym.end_line,
        })
    } else {
        None
    };

    // Determine file path
    let file_path = if let Some(ref loc) = symbol_loc {
        loc.file_path.clone()
    } else if let Some(f) = file {
        if f.is_absolute() { f } else { root.join(f) }
    } else {
        anyhow::bail!("Either --file or --symbol must be provided");
    };

    if !file_path.exists() {
        anyhow::bail!("File not found: {}", file_path.display());
    }

    // Build the edit operation based on mode
    let op = match mode {
        EditMode::Symbol => {
            // Replace entire symbol
            let loc = symbol_loc.as_ref().unwrap(); // Safe: checked above
            let content = content.context("Symbol mode requires --content parameter")?;
            EditOp::replace_lines(&file_path, loc.start_line, loc.end_line, content)
        }
        EditMode::Lines => {
            let line_offset = symbol_loc
                .as_ref()
                .map(|l| l.start_line.saturating_sub(1))
                .unwrap_or(0);
            let start_val = start.context("Lines mode requires --start parameter")?;
            let end_val = end.context("Lines mode requires --end parameter")?;
            let content = content.context("Lines mode requires --content parameter")?;
            EditOp::replace_lines(
                &file_path,
                start_val + line_offset,
                end_val + line_offset,
                content,
            )
        }
        EditMode::Replace => {
            let start_val = start.context("Replace mode requires --start parameter")?;
            let end_val = end.context("Replace mode requires --end parameter")?;
            let content = content.context("Replace mode requires --content parameter")?;
            EditOp::replace(&file_path, Span::new(start_val, end_val), content)
        }
        EditMode::Insert => {
            let start_val = start.context("Insert mode requires --start parameter")?;
            let content = content.context("Insert mode requires --content parameter")?;
            EditOp::insert(&file_path, start_val, content)
        }
        EditMode::Delete => {
            let start_val = start.context("Delete mode requires --start parameter")?;
            let end_val = end.context("Delete mode requires --end parameter")?;
            EditOp::delete(&file_path, Span::new(start_val, end_val))
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
            &result.old_hash.to_hex()[..8],
            &result.new_hash.to_hex()[..8],
            result.bytes_changed
        );
        println!(
            "  Lines: {} modified (net: {:+})",
            result.lines_modified, result.lines_changed
        );

        // Auto-sync after edit if enabled (default: true)
        if config.sync.sync_on_edit {
            let (_graph, _ids, _file_ids, stats) = sync_project(&config, false).await?;
            println!("  Graph synced: {} symbols", stats.symbols_count);
        }
    }

    Ok(())
}

/// Select the best symbol match when multiple candidates exist.
/// Prioritizes: impl > struct > function/method > enum > field > enum_variant
fn select_best_symbol<'a>(symbols: &'a [SymbolInfo], query: &str) -> &'a SymbolInfo {
    // Priority order (lower = better)
    fn kind_priority(kind: &SymbolKind) -> u8 {
        match kind {
            SymbolKind::Impl => 0,   // impl blocks highest priority
            SymbolKind::Struct => 1, // structs next
            SymbolKind::Class => 1,
            SymbolKind::Function => 2,
            SymbolKind::Method => 2,
            SymbolKind::Enum => 3,
            SymbolKind::Trait => 3,
            SymbolKind::Interface => 3,
            SymbolKind::Field => 4,
            SymbolKind::EnumVariant => 5, // enum variants lowest
            _ => 3,
        }
    }

    symbols
        .iter()
        .min_by(|a, b| {
            // First: prefer exact name match
            let a_exact = a.name == query;
            let b_exact = b.name == query;
            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }

            // Second: prefer by kind priority
            let a_priority = kind_priority(&a.kind);
            let b_priority = kind_priority(&b.kind);
            a_priority.cmp(&b_priority)
        })
        .unwrap_or(&symbols[0])
}

/// Symbol batch edit operation.
#[derive(serde::Deserialize)]
pub struct SymbolEdit {
    /// Symbol name to edit.
    pub symbol: String,
    /// New content for the symbol.
    pub content: String,
}

/// Execute batch symbol edits from JSON input.
pub async fn execute_batch_symbols(json_input: &str, preview: bool, backup: bool) -> Result<()> {
    let edits: Vec<SymbolEdit> =
        serde_json::from_str(json_input).context("Failed to parse JSON symbol edits")?;

    if edits.is_empty() {
        anyhow::bail!("No symbol edits provided");
    }

    // Find project root
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    if !check_daemon_running(&config.agent_dir) {
        anyhow::bail!("Daemon not running. Start with 'mimir daemon start'.");
    }

    let mut client = DaemonClient::connect(&config.agent_dir).await?;
    let options = FindOptions::default();

    // Resolve all symbols and build edit operations
    let mut ops: Vec<EditOp> = Vec::new();
    for edit in &edits {
        let result = client.find_symbol(&edit.symbol, options.clone()).await?;

        if result.symbols.is_empty() {
            anyhow::bail!("Symbol '{}' not found", edit.symbol);
        }

        // Select best match: prioritize struct/impl/function over enum_variant/field
        let sym = select_best_symbol(&result.symbols, &edit.symbol);
        if sym.file_path.as_os_str().is_empty() {
            anyhow::bail!("Symbol '{}' has no file path", edit.symbol);
        }

        let file_path = root.join(&sym.file_path);
        println!(
            "  {} → {}:{}-{}",
            edit.symbol,
            sym.file_path.display(),
            sym.start_line,
            sym.end_line
        );

        ops.push(EditOp::replace_lines(
            &file_path,
            sym.start_line,
            sym.end_line,
            &edit.content,
        ));
    }

    // Create editor
    let editor = if backup {
        Editor::new().with_backups()
    } else {
        Editor::new()
    };

    if preview {
        println!("\nPreviewing {} symbol edits:\n", ops.len());
        for op in &ops {
            let diff = editor.preview(op).await?;
            println!("--- {} ---", op.file_path().display());
            println!("{}", diff.unified);
        }
    } else {
        println!("\nApplying {} symbol edits:\n", ops.len());
        let results = editor.apply_batch(&ops).await?;
        for result in results {
            println!(
                "  {} ({:+} bytes, {:+} lines)",
                result.file_path.display(),
                result.bytes_changed,
                result.lines_changed
            );
        }

        // Auto-sync
        if config.sync.sync_on_edit {
            let (_graph, _ids, _file_ids, stats) = sync_project(&config, false).await?;
            println!("\n  Graph synced: {} symbols", stats.symbols_count);
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
