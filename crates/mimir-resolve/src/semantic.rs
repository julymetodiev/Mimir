//! Turning what a semantic source knows into edges: the occurrences of an index and the answers
//! of a running analyzer. Where they come from is the business of a [`crate::SemanticSource`].

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use mimir_graph::EdgeKind;
use mimir_types::{Confidence, FileId, ReferenceKind, Span, Symbol};

use crate::ProjectFile;
use crate::edges::{ConstantUse, EdgeSet, files_by_path};
use crate::lookup::{SymbolTable, trait_of};
use crate::port::{LiveDocument, Occurrence, ScipIndex};

const ROLE_DEFINITION: u64 = 1;
const ROLE_IMPORT: u64 = 2;

/// Add an edge for each call and each function named as a value in the indexes.
/// Returns the files they cover.
pub fn resolve(
    table: &SymbolTable,
    files: &[ProjectFile],
    indexes: &[ScipIndex],
    edges: &mut EdgeSet,
    constant_uses: &mut Vec<ConstantUse>,
) -> HashSet<FileId> {
    let by_path = files_by_path(files);
    let mut covered = HashSet::new();
    for index in indexes {
        let documents: Vec<(&ProjectFile, Vec<&str>, &Vec<Occurrence>)> = index
            .documents
            .iter()
            .filter_map(|(path, occurrences)| {
                let file = by_path.get(index.root.join(path).as_path())?;
                Some((*file, file.source.split('\n').collect(), occurrences))
            })
            .collect();

        let mut definitions = Definitions::default();
        let mut constants = Definitions::default();
        for (file, lines, occurrences) in &documents {
            if !index.unanswered.contains(&file.path) {
                covered.insert(file.file_id);
            }
            if index.is_edited(&file.path) {
                // The positions are old, so find each function again by name and order.
                let mut seen: HashMap<&str, usize> = HashMap::new();
                let mut own: Vec<_> = occurrences
                    .iter()
                    .filter(|o| o.roles & ROLE_DEFINITION != 0 && is_function(&o.symbol))
                    .collect();
                own.sort_by_key(|o| (o.line, o.column));
                for o in own {
                    let name = symbol_name(&o.symbol);
                    let n = seen.entry(name).or_default();
                    if let Some(symbol) = table.nth_definition(file.file_id, name, *n) {
                        definitions.add(o.symbol.as_str(), symbol);
                    }
                    *n += 1;
                }
                let mut seen: HashMap<&str, usize> = HashMap::new();
                let mut own: Vec<_> = occurrences
                    .iter()
                    .filter(|o| o.roles & ROLE_DEFINITION != 0 && is_term(&o.symbol))
                    .collect();
                own.sort_by_key(|o| (o.line, o.column));
                for o in own {
                    let name = symbol_name(&o.symbol);
                    let n = seen.entry(name).or_default();
                    if let Some(symbol) = table.nth_constant(file.file_id, name, *n) {
                        constants.add(o.symbol.as_str(), symbol);
                    }
                    *n += 1;
                }
                continue;
            }
            for o in occurrences
                .iter()
                .filter(|o| o.roles & ROLE_DEFINITION != 0 && is_term(&o.symbol))
            {
                let name = lines
                    .get(o.line as usize)
                    .and_then(|l| l.get(o.column as usize..o.end_column as usize));
                if let Some(symbol) =
                    name.and_then(|name| table.constant_at(file.file_id, o.line + 1, name))
                {
                    constants.add(o.symbol.as_str(), symbol);
                }
            }
            for o in occurrences
                .iter()
                .filter(|o| o.roles & ROLE_DEFINITION != 0 && is_function(&o.symbol))
            {
                let Some(name) = lines
                    .get(o.line as usize)
                    .and_then(|l| l.get(o.column as usize..o.end_column as usize))
                else {
                    continue;
                };
                if let Some(symbol) = table.definition_at(file.file_id, o.line + 1, name) {
                    definitions.add(o.symbol.as_str(), symbol);
                }
            }
        }

        for (file, lines, occurrences) in documents
            .iter()
            .filter(|(file, ..)| !index.is_edited(&file.path))
        {
            // rust-analyzer does not set the import role on the paths in a `use`.
            let imports: Vec<Span> = file
                .references
                .iter()
                .filter(|r| r.kind == ReferenceKind::Import)
                .map(|r| r.span)
                .collect();
            // A failed macro leaves a parameter resolved to the function of the same name.
            let locals: HashSet<u32> = file
                .references
                .iter()
                .filter(|r| r.kind == ReferenceKind::LocalRead)
                .map(|r| r.span.start)
                .collect();
            for o in occurrences.iter().filter(|o| {
                o.roles & (ROLE_DEFINITION | ROLE_IMPORT) == 0 && is_function(&o.symbol)
            }) {
                let Some(target) = definitions.get(o.symbol.as_str(), file.file_id) else {
                    continue;
                };
                let Some(after) = lines
                    .get(o.end_line as usize)
                    .and_then(|l| l.get(o.end_column as usize..))
                else {
                    continue;
                };
                let start = byte_offset(lines, o.line, o.column);
                let span = Span::new(start, byte_offset(lines, o.end_line, o.end_column));
                // An operator's occurrence is its token: `a[i]` is `index`, `a == b` is `eq`.
                let named = lines
                    .get(o.line as usize)
                    .and_then(|l| l.get(o.column as usize..))
                    .is_some_and(|t| t.starts_with(|c: char| c.is_alphabetic() || c == '_'));
                let (from, kind) = if is_call(after) {
                    (
                        table.callable_at(file.file_id, o.line + 1, o.column),
                        EdgeKind::Calls,
                    )
                } else if !named
                    || imports.iter().any(|i| i.start <= start && start < i.end)
                    || locals.contains(&start)
                {
                    continue;
                } else {
                    (table.any_at(file.file_id, start), EdgeKind::UsesAsValue)
                };
                if let Some(from) = from {
                    edges.add(from.id, target.id, kind, span, Confidence::Certain);
                }
            }
            for o in occurrences
                .iter()
                .filter(|o| o.roles & (ROLE_DEFINITION | ROLE_IMPORT) == 0 && is_term(&o.symbol))
            {
                let Some(constant) = constants.get(o.symbol.as_str(), file.file_id) else {
                    continue;
                };
                let start = byte_offset(lines, o.line, o.column);
                if imports.iter().any(|i| i.start <= start && start < i.end) {
                    continue;
                }
                if let Some(from) = table.any_at(file.file_id, start) {
                    constant_uses.push(ConstantUse {
                        from: from.id,
                        constant: constant.id,
                        span: Span::new(start, byte_offset(lines, o.end_line, o.end_column)),
                        confidence: Confidence::Certain,
                    });
                }
            }
        }

        for (file, lines, occurrences) in documents
            .iter()
            .filter(|(file, ..)| !index.is_edited(&file.path))
        {
            for (method, trait_name) in table.trait_impls_in(file.file_id) {
                let declaration = table
                    .impl_at(file.file_id, method.range.start_line)
                    .and_then(|block| {
                        declared_method(
                            block,
                            method,
                            trait_name,
                            lines,
                            occurrences,
                            &definitions,
                            file.file_id,
                        )
                    });
                if let Some(declaration) = declaration {
                    edges.add(
                        method.id,
                        declaration.id,
                        EdgeKind::Implements,
                        method.span,
                        Confidence::Certain,
                    );
                }
            }
        }

        for (path, document) in &index.live {
            let Some(file) = by_path.get(path.as_path()) else {
                continue;
            };
            covered.insert(file.file_id);
            resolve_live(table, file, document, &by_path, edges, constant_uses);
        }
    }
    covered
}

/// Functions by SCIP symbol. A symbol can name several: a package's test binaries are crates
/// of their own, yet `main` or a helper in each gets the same symbol.
#[derive(Default)]
struct Definitions<'a> {
    by_symbol: HashMap<&'a str, Vec<Arc<Symbol>>>,
}

impl<'a> Definitions<'a> {
    fn add(&mut self, symbol: &'a str, definition: &Arc<Symbol>) {
        self.by_symbol
            .entry(symbol)
            .or_default()
            .push(definition.clone());
    }

    /// What a symbol means in a file: the one defined there, or else the only one there is.
    fn get(&self, symbol: &str, file: FileId) -> Option<&Arc<Symbol>> {
        let found = self.by_symbol.get(symbol)?;
        found
            .iter()
            .find(|s| s.file_id == file)
            .or(match found.as_slice() {
                [one] => Some(one),
                _ => None,
            })
    }
}

/// Edges of an edited file, from a running analyzer's answers.
fn resolve_live(
    table: &SymbolTable,
    file: &ProjectFile,
    document: &LiveDocument,
    by_path: &HashMap<&Path, &ProjectFile>,
    edges: &mut EdgeSet,
    constant_uses: &mut Vec<ConstantUse>,
) {
    for reference in &file.references {
        let (line, column) = (reference.range.start_line, reference.range.start_col);
        if reference.kind == ReferenceKind::Read {
            let constant = document.sites.get(&(line, column)).and_then(|t| {
                table.constant_at(by_path.get(t.path.as_path())?.file_id, t.line, &t.name)
            });
            if let (Some(constant), Some(from)) =
                (constant, table.any_at(file.file_id, reference.span.start))
            {
                constant_uses.push(ConstantUse {
                    from: from.id,
                    constant: constant.id,
                    span: reference.span,
                    confidence: Confidence::Certain,
                });
            }
            continue;
        }
        let (from, kind) = match reference.kind {
            ReferenceKind::Call => (
                table.callable_at(file.file_id, line, column),
                EdgeKind::Calls,
            ),
            ReferenceKind::FunctionValue => (
                table.any_at(file.file_id, reference.span.start),
                EdgeKind::UsesAsValue,
            ),
            _ => continue,
        };
        let target = document.sites.get(&(line, column)).and_then(|t| {
            table.definition_at(by_path.get(t.path.as_path())?.file_id, t.line, &t.name)
        });
        if let (Some(from), Some(target)) = (from, target) {
            edges.add(
                from.id,
                target.id,
                kind,
                reference.span,
                Confidence::Certain,
            );
        }
    }
    for (method, _) in table.trait_impls_in(file.file_id) {
        let declared = table
            .impl_at(file.file_id, method.range.start_line)
            .and_then(|block| document.traits.get(&block.range.start_line))
            .and_then(|declared| {
                // Its own name: the impl may use another, from `use a::Tr as Other`.
                let defined_in = by_path.get(declared.path.as_path())?;
                table
                    .methods_of(&declared.name, &method.name)
                    .iter()
                    .find(|s| s.file_id == defined_in.file_id && trait_of(s).is_none())
            });
        if let Some(declared) = declared {
            edges.add(
                method.id,
                declared.id,
                EdgeKind::Implements,
                method.span,
                Confidence::Certain,
            );
        }
    }
}

/// The trait's method for a method of an `impl`. The trait's name in the impl's header is a
/// reference to the exact trait, `crate/path/Tr#`, whose method is `crate/path/Tr#name().`.
/// A trait from outside the project has no definition here, and gets no edge.
fn declared_method<'a>(
    block: &Symbol,
    method: &Symbol,
    trait_name: &str,
    lines: &[&str],
    occurrences: &[Occurrence],
    definitions: &'a Definitions<'_>,
    file: FileId,
) -> Option<&'a Arc<Symbol>> {
    occurrences
        .iter()
        .filter(|o| {
            o.symbol.ends_with('#')
                && o.line + 1 >= block.range.start_line
                && o.line + 1 < method.range.start_line.max(block.range.start_line + 1)
        })
        .filter(|o| {
            lines
                .get(o.line as usize)
                .and_then(|l| l.get(o.column as usize..o.end_column as usize))
                == Some(trait_name)
        })
        .find_map(|o| definitions.get(format!("{}{}().", o.symbol, method.name).as_str(), file))
}

/// Whether the text after a function's name makes it a call: `(`, or `::<...>(`.
fn is_call(after: &str) -> bool {
    let after = after.trim_start();
    let Some(generics) = after.strip_prefix("::<") else {
        return after.starts_with('(');
    };
    let (mut depth, mut previous) = (1, '<');
    for (at, c) in generics.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if previous != '-' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return generics[at + 1..].trim_start().starts_with('(');
        }
        previous = c;
    }
    // Generics running past the end of the line: nearly always a call.
    true
}

fn byte_offset(lines: &[&str], line: u32, column: u32) -> u32 {
    let before: usize = lines.iter().take(line as usize).map(|l| l.len() + 1).sum();
    before as u32 + column
}

/// The name in a function's SCIP symbol: `run` for `a 0.1.0 impl#[T]run().`.
fn symbol_name(symbol: &str) -> &str {
    let descriptor = symbol
        .strip_suffix("().")
        .or_else(|| symbol.strip_suffix('.'))
        .unwrap_or(symbol);
    let start = descriptor
        .rfind(['/', '#', ']', ' '])
        .map_or(0, |at| at + 1);
    &descriptor[start..]
}

/// In SCIP's symbol syntax a constant, static or field ends in a plain `.`.
fn is_term(symbol: &str) -> bool {
    symbol.ends_with('.') && !symbol.ends_with("().") && !symbol.starts_with("local ")
}

/// In SCIP's symbol syntax a function or method ends in `().`.
fn is_function(symbol: &str) -> bool {
    symbol.ends_with("().") && !symbol.starts_with("local ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tells_a_call_from_a_value() {
        assert!(is_call("(1)"));
        assert!(is_call(" (1)"));
        assert!(is_call("::<u32>()"));
        assert!(is_call("::<Vec<u32>>()"));
        assert!(is_call("::<fn() -> u32>()"));
        assert!(!is_call(")"));
        assert!(!is_call(", other)"));
        assert!(!is_call("::<u32>)"));
        assert!(!is_call(""));
    }

    #[test]
    fn finds_the_name_in_a_symbol() {
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 run()."), "run");
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/T#run()."), "run");
        assert_eq!(
            symbol_name("rust-analyzer cargo a 0.1.0 m/impl#[T]run()."),
            "run"
        );
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/TABLE."), "TABLE");
    }

    #[test]
    fn locals_are_not_functions() {
        assert!(is_function("rust-analyzer cargo a 0.1.0 foo()."));
        assert!(!is_function("local 3"));
        assert!(!is_function("rust-analyzer cargo a 0.1.0 Foo#"));
    }
}
