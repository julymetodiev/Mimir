//! Edit command - perform file edits with conflict detection.

use anyhow::{Context, Result};
use mimir_edit::{EditOp, Editor};
use mimir_types::Span;
use std::path::PathBuf;

use crate::app::{find_project_root, Config};

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
            _ => Err(format!("Unknown edit mode: {}. Use: replace, insert, delete, lines", s)),
        }
    }
}

/// Execute the edit command.
pub async fn execute(
    file: PathBuf,
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

    // Resolve file path
    let file_path = if file.is_absolute() {
        file
    } else {
        root.join(&file)
    };

    if !file_path.exists() {
        anyhow::bail!("File not found: {}", file_path.display());
    }

    // Build the edit operation
    let op = match mode {
        EditMode::Replace => {
            let end = end.context("Replace mode requires --end parameter")?;
            let content = content.context("Replace mode requires --content parameter")?;
            EditOp::replace(&file_path, Span::new(start, end), content)
        }
        EditMode::Insert => {
            let content = content.context("Insert mode requires --content parameter")?;
            EditOp::insert(&file_path, start, content)
        }
        EditMode::Delete => {
            let end = end.context("Delete mode requires --end parameter")?;
            EditOp::delete(&file_path, Span::new(start, end))
        }
        EditMode::Lines => {
            let end = end.context("Lines mode requires --end parameter")?;
            let content = content.context("Lines mode requires --content parameter")?;
            EditOp::replace_lines(&file_path, start, end, content)
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
        println!("  Lines changed: {:+}", result.lines_changed);
    }

    Ok(())
}

/// Execute batch edit from JSON input.
pub async fn execute_batch(json_input: &str, preview: bool, backup: bool) -> Result<()> {
    let ops: Vec<EditOp> = serde_json::from_str(json_input)
        .context("Failed to parse JSON edit operations")?;

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
