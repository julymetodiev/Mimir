//! What the symbols of an index mean: SCIP's symbol syntax, and which symbol of the graph each is.

use std::collections::HashMap;
use std::sync::Arc;

use mimir_types::{FileId, Symbol, SymbolKind};

use crate::lookup::type_name;
use crate::port::Occurrence;

pub const ROLE_DEFINITION: u64 = 1;
pub const ROLE_IMPORT: u64 = 2;

/// Symbols of the graph by SCIP symbol. A symbol can name several: a package's test binaries are
/// crates of their own, yet `main` or a helper in each gets the same symbol.
#[derive(Default)]
pub struct Definitions<'a> {
    by_symbol: HashMap<&'a str, Vec<Arc<Symbol>>>,
}

impl<'a> Definitions<'a> {
    pub fn add(&mut self, symbol: &'a str, definition: &Arc<Symbol>) {
        self.by_symbol
            .entry(symbol)
            .or_default()
            .push(definition.clone());
    }

    /// What a symbol means in a file: the one defined there, or else the only one there is.
    pub fn get(&self, symbol: &str, file: FileId) -> Option<&Arc<Symbol>> {
        let found = self.by_symbol.get(symbol)?;
        found
            .iter()
            .find(|s| s.file_id == file)
            .or(match found.as_slice() {
                [one] => Some(one),
                _ => None,
            })
    }

    /// Every definition of a symbol.
    pub fn all(&self, symbol: &str) -> &[Arc<Symbol>] {
        self.by_symbol.get(symbol).map_or(&[], Vec::as_slice)
    }

    /// Add the definitions in a file by where they are, with the name written there.
    pub fn add_by_position<'t>(
        &mut self,
        occurrences: &'a [Occurrence],
        lines: &[&str],
        wanted: fn(&str) -> bool,
        at: impl Fn(u32, &str) -> Option<&'t Arc<Symbol>>,
    ) {
        for o in defined(occurrences, wanted) {
            let name = lines
                .get(o.line as usize)
                .and_then(|l| l.get(o.column as usize..o.end_column as usize));
            if let Some(symbol) = name.and_then(|name| at(o.line + 1, name)) {
                self.add(o.symbol.as_str(), symbol);
            }
        }
    }

    /// Add the definitions in a file edited since the index: the positions are old, so each is found
    /// again by its name, what it belongs to, and its place among those. Where owners are told apart
    /// and none fits there is no definition, rather than another member of that name. `keep` says
    /// which of those found are wanted here, after the place is counted.
    pub fn add_by_order<'t>(
        &mut self,
        occurrences: &'a [Occurrence],
        wanted: fn(&str) -> bool,
        named: impl Fn(&str) -> Vec<&'t Arc<Symbol>>,
        keep: impl Fn(&Symbol) -> bool,
    ) {
        let mut own: Vec<_> = defined(occurrences, wanted).collect();
        own.sort_by_key(|o| (o.line, o.column));
        let mut by_name: HashMap<&str, Vec<&'a Occurrence>> = HashMap::new();
        for o in own {
            by_name.entry(symbol_name(&o.symbol)).or_default().push(o);
        }
        for (name, own) in by_name {
            let found = named(name);
            let owners: Vec<_> = found.iter().map(|s| owner_in_graph(s)).collect();
            let written: Vec<_> = own.iter().map(|o| owner_in_index(&o.symbol)).collect();
            let one_owner =
                written.windows(2).all(|w| w[0] == w[1]) && owners.windows(2).all(|w| w[0] == w[1]);
            let mut seen: HashMap<&[&str], usize> = HashMap::new();
            for (i, (o, owner)) in own.iter().zip(&written).enumerate() {
                let symbol = if one_owner {
                    found.get(i).copied()
                } else {
                    let n = seen.entry(owner).or_default();
                    let symbol = found
                        .iter()
                        .zip(&owners)
                        .filter(|(_, of)| *of == owner)
                        .nth(*n)
                        .map(|(symbol, _)| *symbol);
                    *n += 1;
                    symbol
                };
                if let Some(symbol) = symbol.filter(|s| keep(s)) {
                    self.add(o.symbol.as_str(), symbol);
                }
            }
        }
    }
}

fn defined(
    occurrences: &[Occurrence],
    wanted: fn(&str) -> bool,
) -> impl Iterator<Item = &Occurrence> {
    occurrences
        .iter()
        .filter(move |o| o.roles & ROLE_DEFINITION != 0 && wanted(&o.symbol))
}

/// The name in a SCIP symbol: `run` for `a 0.1.0 impl#[T]run().`.
pub fn symbol_name(symbol: &str) -> &str {
    let (descriptor, start) = name_at(symbol);
    &descriptor[start..]
}

/// A symbol without its suffix, and where the name starts in that.
fn name_at(symbol: &str) -> (&str, usize) {
    let descriptor = symbol
        .strip_suffix("().")
        .or_else(|| symbol.strip_suffix('.'))
        .or_else(|| symbol.strip_suffix('#'))
        .unwrap_or(symbol);
    let start = descriptor
        .rfind(['/', '#', ']', ' '])
        .map_or(0, |at| at + 1);
    (descriptor, start)
}

/// What a SCIP symbol belongs to, as bare names: `[T, Tr]` for `impl#[T][Tr]run().`, `[T]` for
/// `T#run().`, and none for what is directly in a module.
fn owner_in_index(symbol: &str) -> Vec<&str> {
    let (descriptor, start) = name_at(symbol);
    let before = &descriptor[..start];
    if let Some(at) = before.strip_suffix(']').and_then(|b| b.rfind("impl#[")) {
        let brackets = &before[at + "impl#[".len()..before.len() - 1];
        return brackets.split("][").map(bare_type).collect();
    }
    let Some(owner) = before.strip_suffix('#') else {
        return Vec::new();
    };
    let from = owner.rfind(['/', '#', ' ']).map_or(0, |at| at + 1);
    vec![bare_type(&owner[from..])]
}

/// What a symbol of the graph belongs to, in the same form. A module or a package in its name is not
/// an owner, as it is not in `owner_in_index`.
fn owner_in_graph(symbol: &Symbol) -> Vec<&str> {
    let segments = symbol.qualified_name.segments();
    let Some(parent) = segments
        .len()
        .checked_sub(2)
        .and_then(|at| segments.get(at))
    else {
        return Vec::new();
    };
    if let Some(header) = parent.strip_prefix("impl ") {
        return match header.rsplit_once(" for ") {
            Some((of_trait, of_type)) => vec![bare_type(of_type), bare_type(of_trait)],
            None => vec![bare_type(header)],
        };
    }
    let is_member = matches!(
        symbol.kind,
        SymbolKind::Method | SymbolKind::Field | SymbolKind::Property | SymbolKind::EnumVariant
    );
    if is_member {
        vec![bare_type(parent)]
    } else {
        Vec::new()
    }
}

/// `Foo` for `` `&a::Foo<T>` ``.
fn bare_type(written: &str) -> &str {
    type_name(written.trim_matches('`'))
}

/// In SCIP's symbol syntax a constant, static or field ends in a plain `.`.
pub fn is_term(symbol: &str) -> bool {
    symbol.ends_with('.') && !symbol.ends_with("().") && !symbol.starts_with("local ")
}

/// In SCIP's symbol syntax a function or method ends in `().`.
pub fn is_function(symbol: &str) -> bool {
    symbol.ends_with("().") && !symbol.starts_with("local ")
}

/// In SCIP's symbol syntax a type, trait or alias ends in `#`.
fn is_type(symbol: &str) -> bool {
    symbol.ends_with('#') && !symbol.starts_with("local ")
}

/// A symbol that may be a type or a field. What an `impl` declares has no symbol in the graph:
/// `type Error = E;` there is not the `Error` beside it.
pub fn is_data(symbol: &str) -> bool {
    (is_type(symbol) || is_term(symbol)) && !symbol.contains("impl#[")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_name_in_a_symbol() {
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 run()."), "run");
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/T#run()."), "run");
        assert_eq!(
            symbol_name("rust-analyzer cargo a 0.1.0 m/impl#[T]run()."),
            "run"
        );
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/TABLE."), "TABLE");
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/Foo#"), "Foo");
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/Foo#bar."), "bar");
    }

    #[test]
    fn finds_what_a_symbol_belongs_to() {
        let of = |s| owner_in_index(s);
        assert_eq!(of("gomod m 1 m/A#Name()."), ["A"]);
        assert_eq!(of("gomod m 1 m/A#x."), ["A"]);
        assert_eq!(of("cargo a 0.1.0 m/impl#[A]run()."), ["A"]);
        assert_eq!(of("cargo a 0.1.0 m/impl#[A][Tr]run()."), ["A", "Tr"]);
        assert_eq!(
            of("cargo a 0.1.0 m/impl#[`Foo<T>`][`PartialEq<Self>`]eq()."),
            ["Foo", "PartialEq"]
        );
        assert_eq!(of("cargo a 0.1.0 m/Tr#run()."), ["Tr"]);
        assert!(of("cargo a 0.1.0 m/run().").is_empty());
        assert!(of("cargo a 0.1.0 m/Foo#").is_empty());
    }

    #[test]
    fn locals_are_not_functions() {
        assert!(is_function("rust-analyzer cargo a 0.1.0 foo()."));
        assert!(!is_function("local 3"));
        assert!(!is_function("rust-analyzer cargo a 0.1.0 Foo#"));
    }

    #[test]
    fn tells_types_and_fields_from_the_rest() {
        assert!(is_data("rust-analyzer cargo a 0.1.0 m/Foo#"));
        assert!(is_data("rust-analyzer cargo a 0.1.0 m/Foo#bar."));
        assert!(!is_data("rust-analyzer cargo a 0.1.0 m/Foo#[T]"));
        assert!(!is_data("rust-analyzer cargo a 0.1.0 m/Foo#run()."));
        assert!(!is_data("local 3"));
        assert!(!is_data(
            "rust-analyzer cargo a 0.1.0 m/impl#[Foo][`Tr`]Error#"
        ));
    }
}
