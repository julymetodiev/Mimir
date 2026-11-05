//! Turning what a client asked to change into operations on files.

use latt_edit::EditOp;
use latt_protocol::EditChange;
use latt_types::{ContentHash, Span, Symbol, SymbolKind};
use std::path::Path;

use crate::error::GraphError;

/// The operation for a change to a symbol. Line numbers count from the symbol's first line, and the
/// hash makes the edit fail if the file is no longer what the symbol's lines were read from.
pub fn on_symbol(
    path: &Path,
    symbol: &Symbol,
    hash: ContentHash,
    text: &str,
    change: &EditChange,
) -> Result<EditOp, GraphError> {
    let op = match change {
        EditChange::WholeSymbol { content } => EditOp::replace_lines(
            path,
            symbol.range.start_line,
            symbol.range.end_line,
            content,
        ),
        EditChange::AfterSymbol { content } => EditOp::insert(
            path,
            end_of_line(text, symbol.span.end),
            after(symbol, content),
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

/// Where the line holding a byte position ends, before its newline.
fn end_of_line(text: &str, at: u32) -> u32 {
    let rest = text.get(at as usize..).unwrap_or("");
    at + rest.find('\n').unwrap_or(rest.len()) as u32
}

/// New code as it goes after a symbol: a line apart from an item, right below a field or a variant.
fn after(symbol: &Symbol, content: &str) -> String {
    let gap = match symbol.kind {
        SymbolKind::Field | SymbolKind::EnumVariant => "\n",
        _ => "\n\n",
    };
    format!("{gap}{}", content.trim_end_matches('\n'))
}

/// The operation for a change to a file.
pub fn on_file(path: &Path, change: &EditChange) -> Result<EditOp, GraphError> {
    Ok(match change {
        EditChange::WholeSymbol { .. } | EditChange::AfterSymbol { .. } => {
            return Err(GraphError::NotASymbol);
        }
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
        EditChange::Text { old, new, all } => EditOp::replace_text(path, old, new, *all),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_end_of_line_passes_what_follows_on_the_line() {
        let text = "fn a() {} // note\nfn b() {}";
        assert_eq!(end_of_line(text, 9), 17);
        assert_eq!(end_of_line(text, 26), 27);
        assert_eq!(end_of_line(text, 99), 99);
    }
}
