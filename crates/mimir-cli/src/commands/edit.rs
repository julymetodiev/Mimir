//! Edit commands: the engine does the editing, with conflict detection, so the graph follows.

use anyhow::{Context, Result};
use mimir_client::Client;
use mimir_protocol::{Edit, EditChange, EditReport, EditTarget};
use serde::Deserialize;
use std::path::PathBuf;

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
    /// Add new code after a symbol (requires --symbol).
    After,
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
            "after" => Ok(Self::After),
            _ => Err(format!(
                "Unknown edit mode: {s}. Use: replace, insert, delete, lines, symbol, after"
            )),
        }
    }
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
    let file = request.file.map(std::path::absolute).transpose()?;
    let target = match (request.symbol, file) {
        (Some(symbol), file) => EditTarget::Symbol {
            name: symbol.to_string(),
            file,
        },
        (None, Some(file)) => EditTarget::File(file),
        (None, None) => anyhow::bail!("Either --file or --symbol must be provided"),
    };
    let start = || request.start.context("This mode requires --start");
    let end = || request.end.context("This mode requires --end");
    let content = || {
        request
            .content
            .clone()
            .context("This mode requires --content")
    };
    let change = match request.mode {
        EditMode::Symbol => EditChange::WholeSymbol {
            content: content()?,
        },
        EditMode::After => EditChange::AfterSymbol {
            content: content()?,
        },
        EditMode::Lines => EditChange::Lines {
            start: start()?,
            end: end()?,
            content: content()?,
        },
        EditMode::Replace => EditChange::Replace {
            start: start()?,
            end: end()?,
            content: content()?,
        },
        EditMode::Insert => EditChange::Insert {
            at: start()?,
            content: content()?,
        },
        EditMode::Delete => EditChange::Delete {
            start: start()?,
            end: end()?,
        },
    };
    apply(
        vec![Edit { target, change }],
        request.preview,
        request.backup,
    )
    .await
}

/// One edit of `edit-symbols`.
#[derive(Deserialize)]
struct SymbolEdit {
    symbol: String,
    file: Option<PathBuf>,
    content: String,
}

/// Replace several whole symbols together, from JSON:
/// `[{"symbol": "name", "file": "optional/path.rs", "content": "code"}]`.
pub async fn execute_symbols(json: &str, preview: bool, backup: bool) -> Result<()> {
    let edits: Vec<SymbolEdit> =
        serde_json::from_str(json).context("Failed to parse JSON symbol edits")?;
    if edits.is_empty() {
        anyhow::bail!("No symbol edits provided");
    }
    let edits = edits
        .into_iter()
        .map(|edit| Edit {
            target: EditTarget::Symbol {
                name: edit.symbol,
                file: edit.file,
            },
            change: EditChange::WholeSymbol {
                content: edit.content,
            },
        })
        .collect();
    apply(edits, preview, backup).await
}

async fn apply(edits: Vec<Edit>, preview: bool, backup: bool) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let report = client.edit(edits, preview, backup).await?;
    show(&report);
    Ok(())
}

fn show(report: &EditReport) {
    for file in &report.files {
        match &file.diff {
            Some(diff) => {
                println!("--- {} ---", file.path.display());
                println!("{diff}");
            }
            None => println!(
                "Edited: {} ({:+} bytes, {:+} lines)",
                file.path.display(),
                file.bytes_changed,
                file.lines_changed
            ),
        }
    }
}
