//! Choosing a symbol by name and cutting its lines out of a file's text.

use mimir_types::{FileId, Symbol, SymbolKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// The symbol a bare name most likely means: one with exactly that name, then an impl, a type, a
/// function; a field and a variant last. Among equals the one most called, as what a name means is
/// most often what is most used, then the first by path and place, so the same question gets the
/// same symbol.
pub fn most_likely<'a>(
    found: &'a [Arc<Symbol>],
    name: &str,
    paths: &HashMap<FileId, PathBuf>,
    callers: impl Fn(&Symbol) -> usize,
) -> Option<&'a Arc<Symbol>> {
    likeliest(found, name).into_iter().min_by_key(|symbol| {
        (
            std::cmp::Reverse(callers(symbol)),
            paths.get(&symbol.file_id),
            symbol.range.start_line,
            symbol.range.start_col,
        )
    })
}

/// The symbols a bare name is equally likely to mean.
pub fn likeliest<'a>(found: &'a [Arc<Symbol>], name: &str) -> Vec<&'a Arc<Symbol>> {
    let rank = |symbol: &Symbol| {
        let rank = match symbol.kind {
            SymbolKind::Impl => 100,
            SymbolKind::Struct | SymbolKind::Class => 90,
            SymbolKind::Function | SymbolKind::Method => 80,
            SymbolKind::Enum | SymbolKind::Trait | SymbolKind::Interface => 70,
            SymbolKind::Field => 20,
            SymbolKind::EnumVariant => 10,
            _ => 60,
        };
        (symbol.name == name, rank)
    };
    let best = found.iter().map(|symbol| rank(symbol)).max();
    found
        .iter()
        .filter(|symbol| Some(rank(symbol)) == best)
        .collect()
}

/// Lines `start..=end` (1-indexed) with context, at most `limit` of the symbol's own when no context
/// is asked. Returns the first line's number, the lines, and whether any were left out.
pub fn cut(
    text: &str,
    start: u32,
    end: u32,
    before: u32,
    after: u32,
    limit: Option<u32>,
) -> (u32, Vec<String>, bool) {
    let all: Vec<&str> = text.lines().collect();
    let first = start.saturating_sub(before).max(1);
    let last = end.saturating_add(after).min(all.len() as u32);
    let shown = limit.map_or(last, |limit| {
        last.min(first.saturating_add(limit).saturating_sub(1))
    });
    let lines = all
        .iter()
        .skip(first as usize - 1)
        .take(shown.saturating_sub(first) as usize + 1)
        .map(|line| (*line).to_string())
        .collect();
    (first, lines, shown < last)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "one\ntwo\nthree\nfour\nfive";

    #[test]
    fn test_cut_with_context() {
        let (first, lines, truncated) = cut(TEXT, 2, 3, 1, 1, None);
        assert_eq!(first, 1);
        assert_eq!(lines, ["one", "two", "three", "four"]);
        assert!(!truncated);
    }

    #[test]
    fn test_cut_stops_at_the_ends_of_the_file() {
        let (first, lines, _) = cut(TEXT, 1, 5, 9, 9, None);
        assert_eq!(first, 1);
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn test_cut_to_a_limit() {
        let (first, lines, truncated) = cut(TEXT, 2, 5, 0, 0, Some(2));
        assert_eq!(first, 2);
        assert_eq!(lines, ["two", "three"]);
        assert!(truncated);
    }
}
