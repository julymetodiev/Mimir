//! Choosing a symbol by name and cutting its lines out of a file's text.

use mimir_types::{Symbol, SymbolKind};
use std::sync::Arc;

/// The symbol a bare name most likely means: a type, then a function; a variant last.
pub fn most_likely<'a>(found: &'a [Arc<Symbol>], name: &str) -> Option<&'a Arc<Symbol>> {
    found
        .iter()
        .filter(|symbol| symbol.name == name)
        .max_by_key(|symbol| match symbol.kind {
            SymbolKind::Struct | SymbolKind::Class => 90,
            SymbolKind::Function => 80,
            SymbolKind::Enum | SymbolKind::Trait | SymbolKind::Interface => 70,
            SymbolKind::EnumVariant => 10,
            _ => 60,
        })
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
