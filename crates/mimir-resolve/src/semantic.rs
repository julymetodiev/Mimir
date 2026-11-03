//! Turning what a semantic source knows into edges: the occurrences of an index and the answers
//! of a running analyzer. Where they come from is the business of a [`crate::SemanticSource`].

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use mimir_graph::EdgeKind;
use mimir_types::{Confidence, FileId, ReferenceKind, Span, Symbol, SymbolKind};

use crate::ProjectFile;
use crate::definitions::{
    Definitions, ROLE_DEFINITION, ROLE_IMPORT, is_data, is_function, is_term,
};
use crate::edges::{ConstantUse, EdgeSet, files_by_path};
use crate::lookup::{SymbolTable, trait_of};
use crate::port::{LiveDocument, Occurrence, ScipIndex};

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
        let mut data = Definitions::default();
        for (file, lines, occurrences) in &documents {
            if !index.unanswered.contains(&file.path) {
                covered.insert(file.file_id);
            }
            let id = file.file_id;
            if index.is_edited(&file.path) {
                definitions.add_by_order(occurrences, is_function, |name, n| {
                    table.nth_definition(id, name, n)
                });
                constants.add_by_order(occurrences, is_term, |name, n| {
                    table.nth_constant(id, name, n)
                });
                data.add_by_order(occurrences, is_data, |name, n| table.nth_data(id, name, n));
                continue;
            }
            definitions.add_by_position(occurrences, lines, is_function, |line, name| {
                table.definition_at(id, line, name)
            });
            constants.add_by_position(occurrences, lines, is_term, |line, name| {
                table.constant_at(id, line, name)
            });
            data.add_by_position(occurrences, lines, is_data, |line, name| {
                table.data_at(id, line, name)
            });
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
            add_data_uses(table, file, lines, occurrences, &data, &imports, edges);
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

/// Add an edge for each use in a file of a type or field of the project.
fn add_data_uses(
    table: &SymbolTable,
    file: &ProjectFile,
    lines: &[&str],
    occurrences: &[Occurrence],
    data: &Definitions<'_>,
    imports: &[Span],
    edges: &mut EdgeSet,
) {
    for o in occurrences
        .iter()
        .filter(|o| o.roles & (ROLE_DEFINITION | ROLE_IMPORT) == 0 && is_data(&o.symbol))
    {
        let Some(target) = data.get(o.symbol.as_str(), file.file_id) else {
            continue;
        };
        let start = byte_offset(lines, o.line, o.column);
        // `Self` is written only inside the type it stands for, and says nothing about who uses it.
        let is_self = lines
            .get(o.line as usize)
            .and_then(|l| l.get(o.column as usize..o.end_column as usize))
            == Some("Self");
        if is_self || imports.iter().any(|i| i.start <= start && start < i.end) {
            continue;
        }
        if let Some(from) = table.any_at(file.file_id, start) {
            let span = Span::new(start, byte_offset(lines, o.end_line, o.end_column));
            add_data_use(from, target, span, edges);
        }
    }
}

fn add_data_use(from: &Symbol, target: &Symbol, span: Span, edges: &mut EdgeSet) {
    if from.id == target.id {
        return;
    }
    let kind = if target.kind == SymbolKind::Field {
        EdgeKind::UsesField
    } else {
        EdgeKind::UsesType
    };
    edges.add(from.id, target.id, kind, span, Confidence::Certain);
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
        if reference.kind == ReferenceKind::FieldUse {
            let field = document
                .field_sites
                .get(&(line, column))
                .into_iter()
                .flatten()
                .find_map(|t| {
                    let defined_in = by_path.get(t.path.as_path())?.file_id;
                    table
                        .data_at(defined_in, t.line, &t.name)
                        .filter(|s| s.kind == SymbolKind::Field)
                });
            if let (Some(field), Some(from)) =
                (field, table.any_at(file.file_id, reference.span.start))
            {
                add_data_use(from, field, reference.span, edges);
            }
            continue;
        }
        if matches!(reference.kind, ReferenceKind::Read | ReferenceKind::TypeUse) {
            // What a site is, only the answer says: `Kind::A` looks like a constant.
            let answer = document.sites.get(&(line, column)).and_then(|t| {
                let defined_in = by_path.get(t.path.as_path())?.file_id;
                Some((defined_in, t))
            });
            let Some(from) = table.any_at(file.file_id, reference.span.start) else {
                continue;
            };
            let Some((defined_in, target)) = answer else {
                continue;
            };
            if let Some(constant) = table.constant_at(defined_in, target.line, &target.name) {
                constant_uses.push(ConstantUse {
                    from: from.id,
                    constant: constant.id,
                    span: reference.span,
                    confidence: Confidence::Certain,
                });
            } else if let Some(data) = table.data_at(defined_in, target.line, &target.name) {
                add_data_use(from, data, reference.span, edges);
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
}
