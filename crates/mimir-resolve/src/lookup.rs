//! The symbol lookups both resolvers need.

use std::collections::HashMap;
use std::sync::Arc;

use mimir_graph::CodeGraph;
use mimir_types::{FileId, Symbol, SymbolKind};

use crate::ProjectFile;

/// Whether a symbol is a function or method.
pub(crate) fn is_callable(symbol: &Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
}

/// The type or trait a method belongs to: `Foo` for a method of `impl<T> Tr for a::Foo<T>`.
pub(crate) fn owner_of(symbol: &Symbol) -> Option<&str> {
    if symbol.kind != SymbolKind::Method {
        return None;
    }
    let segments = symbol.qualified_name.segments();
    let parent = segments.get(segments.len().checked_sub(2)?)?;
    let target = match parent.strip_prefix("impl ") {
        Some(rest) => rest.rsplit(" for ").next().unwrap_or(rest),
        None => parent,
    };
    Some(type_name(target))
}

/// The trait a method implements: `Tr` for a method of `impl a::Tr<X> for Foo`.
pub(crate) fn trait_of(symbol: &Symbol) -> Option<&str> {
    if symbol.kind != SymbolKind::Method {
        return None;
    }
    let segments = symbol.qualified_name.segments();
    let parent = segments.get(segments.len().checked_sub(2)?)?;
    let (implemented, _) = parent.strip_prefix("impl ")?.rsplit_once(" for ")?;
    Some(type_name(implemented))
}

/// Whether a method implements a trait with type arguments, as in `impl From<A> for Foo`.
/// Such a trait can be implemented again where syntax cannot see it: in `std`, or by a derive.
pub(crate) fn implements_generic_trait(symbol: &Symbol) -> bool {
    let segments = symbol.qualified_name.segments();
    segments
        .len()
        .checked_sub(2)
        .and_then(|i| segments[i].strip_prefix("impl "))
        .and_then(|parent| parent.rsplit_once(" for "))
        .is_some_and(|(implemented, _)| implemented.contains('<'))
}

/// The bare name of a type path: `Foo` for `&a::b::Foo<T>`.
pub(crate) fn type_name(path: &str) -> &str {
    let base = path.split('<').next().unwrap_or(path).trim_end();
    let base = base.rsplit("::").next().unwrap_or(base);
    base.trim_start_matches(['&', ' ']).trim_start_matches("mut ").trim()
}

/// The callable symbols of a project, indexed by file, name and owner.
pub(crate) struct SymbolTable {
    by_file: HashMap<FileId, Vec<Arc<Symbol>>>,
    by_name: HashMap<String, Vec<Arc<Symbol>>>,
    by_owner: HashMap<(String, String), Vec<Arc<Symbol>>>,
    named: HashMap<String, Vec<Arc<Symbol>>>,
    all_by_file: HashMap<FileId, Vec<Arc<Symbol>>>,
}

impl SymbolTable {
    pub(crate) fn new(graph: &CodeGraph, files: &[ProjectFile]) -> Self {
        let mut table = Self {
            by_file: HashMap::new(),
            by_name: HashMap::new(),
            by_owner: HashMap::new(),
            named: HashMap::new(),
            all_by_file: HashMap::new(),
        };
        for file in files {
            for symbol in graph.symbols_in_file(file.file_id) {
                table.all_by_file.entry(file.file_id).or_default().push(symbol.clone());
                table.named.entry(symbol.name.clone()).or_default().push(symbol.clone());
                if !is_callable(&symbol) {
                    continue;
                }
                table.by_file.entry(file.file_id).or_default().push(symbol.clone());
                table.by_name.entry(symbol.name.clone()).or_default().push(symbol.clone());
                if let Some(owner) = owner_of(&symbol) {
                    table
                        .by_owner
                        .entry((owner.to_string(), symbol.name.clone()))
                        .or_default()
                        .push(symbol.clone());
                }
            }
        }
        table
    }

    /// The innermost function or method holding a position (1-based line, 0-based column).
    pub(crate) fn callable_at(&self, file: FileId, line: u32, column: u32) -> Option<&Arc<Symbol>> {
        self.by_file
            .get(&file)?
            .iter()
            .filter(|s| holds(s, line, column))
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The innermost function or method with this name holding a line.
    pub(crate) fn definition_at(&self, file: FileId, line: u32, name: &str) -> Option<&Arc<Symbol>> {
        self.by_file
            .get(&file)?
            .iter()
            .filter(|s| s.name == name && s.range.start_line <= line && line <= s.range.end_line)
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The `n`th function or method with this name in a file, in the order they are written.
    pub(crate) fn nth_definition(&self, file: FileId, name: &str, n: usize) -> Option<&Arc<Symbol>> {
        let mut named: Vec<_> = self.by_file.get(&file)?.iter().filter(|s| s.name == name).collect();
        named.sort_by_key(|s| s.span.start);
        named.get(n).copied()
    }

    /// The constant or static with this name declared on a line.
    pub(crate) fn constant_at(&self, file: FileId, line: u32, name: &str) -> Option<&Arc<Symbol>> {
        self.all_by_file.get(&file)?.iter().find(|s| {
            s.kind == SymbolKind::Constant && s.name == name && s.range.start_line <= line && line <= s.range.end_line
        })
    }

    /// The `n`th constant or static with this name in a file, in the order they are written.
    pub(crate) fn nth_constant(&self, file: FileId, name: &str, n: usize) -> Option<&Arc<Symbol>> {
        let mut named: Vec<_> = self
            .all_by_file
            .get(&file)?
            .iter()
            .filter(|s| s.kind == SymbolKind::Constant && s.name == name)
            .collect();
        named.sort_by_key(|s| s.span.start);
        named.get(n).copied()
    }

    /// The only constant or static with this name: in this file first, then in the project.
    pub(crate) fn only_constant(&self, file: FileId, name: &str) -> Option<&Arc<Symbol>> {
        let all: Vec<_> = self.named(name).iter().filter(|s| s.kind == SymbolKind::Constant).collect();
        let local: Vec<_> = all.iter().filter(|s| s.file_id == file).collect();
        match (local.as_slice(), all.as_slice()) {
            ([one], _) => Some(**one),
            ([], [one]) => Some(*one),
            _ => None,
        }
    }

    /// The innermost symbol of any kind holding a byte offset.
    pub(crate) fn any_at(&self, file: FileId, offset: u32) -> Option<&Arc<Symbol>> {
        self.all_by_file
            .get(&file)?
            .iter()
            .filter(|s| s.span.start <= offset && offset <= s.span.end)
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The innermost impl block holding a line.
    pub(crate) fn impl_at(&self, file: FileId, line: u32) -> Option<&Arc<Symbol>> {
        self.all_by_file
            .get(&file)?
            .iter()
            .filter(|s| s.kind == SymbolKind::Impl && s.range.start_line <= line && line <= s.range.end_line)
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The methods in a file that implement a trait.
    pub(crate) fn trait_impls_in(&self, file: FileId) -> impl Iterator<Item = (&Arc<Symbol>, &str)> {
        self.by_file
            .get(&file)
            .into_iter()
            .flatten()
            .filter_map(|s| trait_of(s).map(|t| (s, t)))
    }

    /// Functions and methods with this name.
    pub(crate) fn callables(&self, name: &str) -> &[Arc<Symbol>] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// Methods with this name on a type or trait.
    pub(crate) fn methods_of(&self, owner: &str, name: &str) -> &[Arc<Symbol>] {
        self.by_owner
            .get(&(owner.to_string(), name.to_string()))
            .map_or(&[], Vec::as_slice)
    }

    /// Symbols of any kind with this name.
    pub(crate) fn named(&self, name: &str) -> &[Arc<Symbol>] {
        self.named.get(name).map_or(&[], Vec::as_slice)
    }
}

fn holds(symbol: &Symbol, line: u32, column: u32) -> bool {
    let r = &symbol.range;
    (r.start_line, r.start_col) <= (line, column) && (line, column) <= (r.end_line, r.end_col)
}
