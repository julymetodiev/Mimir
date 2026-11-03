//! Turning what a client asked to change into operations on files.

use mimir_edit::EditOp;
use mimir_protocol::EditChange;
use mimir_types::{ContentHash, Span, Symbol};
use std::path::Path;

use crate::error::GraphError;

/// The operation for a change to a symbol. Line numbers count from the symbol's first line, and the
/// hash makes the edit fail if the file is no longer what the symbol's lines were read from.
pub fn on_symbol(
    path: &Path,
    symbol: &Symbol,
    hash: ContentHash,
    change: &EditChange,
) -> Result<EditOp, GraphError> {
    let op = match change {
        EditChange::WholeSymbol { content } => EditOp::replace_lines(
            path,
            symbol.range.start_line,
            symbol.range.end_line,
            content,
        ),
        EditChange::Lines {
            start,
            end,
            content,
        } => {
            let offset = symbol.range.start_line.saturating_sub(1);
            EditOp::replace_lines(path, start + offset, end + offset, content)
        }
        other => on_file(path, other)?,
    };
    Ok(op.with_expected_hash(hash))
}

/// The operation for a change to a file.
pub fn on_file(path: &Path, change: &EditChange) -> Result<EditOp, GraphError> {
    Ok(match change {
        EditChange::WholeSymbol { .. } => return Err(GraphError::NotASymbol),
        EditChange::Lines {
            start,
            end,
            content,
        } => EditOp::replace_lines(path, *start, *end, content),
        EditChange::Replace {
            start,
            end,
            content,
        } => EditOp::replace(path, Span::new(*start, *end), content),
        EditChange::Insert { at, content } => EditOp::insert(path, *at, content),
        EditChange::Delete { start, end } => EditOp::delete(path, Span::new(*start, *end)),
    })
}
