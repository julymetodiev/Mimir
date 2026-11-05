//! Turning what a semantic source knows into edges: the occurrences of an index and the answers
//! of a running analyzer. Where they come from is the business of a [`crate::SemanticSource`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use latt_graph::EdgeKind;
use latt_types::{Confidence, FileId, Language, ReferenceKind, Span, Symbol, SymbolKind};

use crate::definitions::{
    Definitions, ROLE_DEFINITION, ROLE_IMPORT, is_data, is_function, is_term,
};
use crate::edges::{ConstantUse, EdgeSet, files_by_path};
use crate::known::{Document, Known, documents_of};
use crate::lines::Lines;
use crate::live::resolve_live;
use crate::lookup::SymbolTable;
use crate::port::{Occurrence, SemanticIndex};
use crate::{ProjectFile, cfamily, golang, script};

/// Add an edge for each call and each function named as a value in the indexes.
/// Returns the files they cover.
pub fn resolve(
    table: &SymbolTable,
    files: &[ProjectFile],
    indexes: &[SemanticIndex],
    family: Language,
    edges: &mut EdgeSet,
    constant_uses: &mut Vec<ConstantUse>,
) -> HashSet<FileId> {
    let by_path = files_by_path(files);
    let mut covered = HashSet::new();
    let read: Vec<_> = indexes
        .iter()
        .map(|index| (index, documents_of(index, &by_path)))
        .collect();
    if family == Language::Go {
        // A module's symbols are used from other modules' indexes, without a version.
        let mut known = Known::default();
        let mut edited = HashSet::new();
        for (index, documents) in &read {
            known.add_go(table, index, documents, &mut covered);
            edited.extend(
                documents
                    .iter()
                    .filter(|(file, ..)| index.is_edited(&file.path))
                    .map(|(file, ..)| file.file_id),
            );
        }
        for (index, documents) in &read {
            link_documents(table, index, documents, &known, true, edges, constant_uses);
        }
        golang::add_index_implementations(table, indexes, &known.definitions, &edited, edges);
    } else {
        for (index, documents) in &read {
            let mut known = Known::default();
            known.add(table, index, documents, &mut covered);
            link_documents(table, index, documents, &known, false, edges, constant_uses);
            if family == Language::C {
                cfamily::add_index_implementations(index, &known.definitions, edges);
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
                                &known.definitions,
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
        }
    }
    for index in indexes {
        for (path, document) in &index.answers {
            let Some(file) = by_path.get(path.as_path()) else {
                continue;
            };
            covered.insert(file.file_id);
            match family {
                Language::Rust => {
                    resolve_live(table, file, document, &by_path, edges, constant_uses)
                }
                Language::C => {
                    cfamily::resolve_answers(table, file, document, &by_path, edges, constant_uses)
                }
                _ => script::resolve_answers(table, file, document, &by_path, edges, constant_uses),
            }
            if family == Language::Go {
                golang::add_answered_implementations(table, file, document, &by_path, edges);
            }
        }
    }
    covered
}

/// Add the edges of the occurrences in an index, for the files not edited since it.
fn link_documents(
    table: &SymbolTable,
    index: &SemanticIndex,
    documents: &[Document<'_>],
    known: &Known<'_>,
    go: bool,
    edges: &mut EdgeSet,
    constant_uses: &mut Vec<ConstantUse>,
) {
    let names_callable = if go {
        golang::may_be_callable
    } else {
        is_function
    };
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
        let mut term_definitions: HashMap<&str, Vec<u32>> = HashMap::new();
        for d in occurrences
            .iter()
            .filter(|d| d.roles & ROLE_DEFINITION != 0 && is_term(&d.symbol))
        {
            term_definitions
                .entry(d.symbol.as_str())
                .or_default()
                .push(lines.offset(d.line, d.column));
        }
        for o in occurrences
            .iter()
            .filter(|o| o.roles & (ROLE_DEFINITION | ROLE_IMPORT) == 0 && names_callable(&o.symbol))
        {
            let target = if go {
                golang::definition(&known.definitions, o.symbol.as_str(), file.file_id)
            } else {
                known.definitions.get(o.symbol.as_str(), file.file_id)
            };
            let Some(target) = target else {
                continue;
            };
            let Some(after) = lines
                .get(o.end_line as usize)
                .and_then(|l| l.get(o.end_column as usize..))
            else {
                continue;
            };
            let start = lines.offset(o.line, o.column);
            let span = Span::new(start, lines.offset(o.end_line, o.end_column));
            // An operator's occurrence is its token: `a[i]` is `index`, `a == b` is `eq`.
            let named = lines
                .get(o.line as usize)
                .and_then(|l| l.get(o.column as usize..))
                .is_some_and(|t| t.starts_with(|c: char| c.is_alphabetic() || c == '_'));
            let called = if go {
                golang::is_call(after)
            } else if file.path.extension().is_some_and(is_c_family) {
                cfamily::is_call(after)
            } else {
                is_call(after)
            };
            let (from, kind) = if called {
                let caller = table.callable_at(file.file_id, o.line + 1, o.column);
                (
                    // In Go, code at package level belongs to the variable it initialises.
                    caller.or_else(|| table.any_at(file.file_id, start).filter(|_| go)),
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
            let Some(constant) = known.constants.get(o.symbol.as_str(), file.file_id) else {
                continue;
            };
            let start = lines.offset(o.line, o.column);
            if imports.iter().any(|i| i.start <= start && start < i.end) {
                continue;
            }
            // A static in a function gets the same SCIP symbol as an item of that name, and is
            // no symbol of the graph: what its function reads is not the item.
            let defined_in_this_function =
                table
                    .callable_holding(file.file_id, start)
                    .is_some_and(|function| {
                        term_definitions
                            .get(o.symbol.as_str())
                            .is_some_and(|places| {
                                places
                                    .iter()
                                    .any(|&at| function.span.start <= at && at <= function.span.end)
                            })
                    });
            if defined_in_this_function {
                continue;
            }
            if let Some(from) = table.any_at(file.file_id, start) {
                constant_uses.push(ConstantUse {
                    from: from.id,
                    constant: constant.id,
                    span: Span::new(start, lines.offset(o.end_line, o.end_column)),
                    confidence: Confidence::Certain,
                });
            }
        }
        add_data_uses(
            table,
            file,
            lines,
            occurrences,
            &known.data,
            &imports,
            edges,
        );
    }
}

fn is_c_family(extension: &std::ffi::OsStr) -> bool {
    extension
        .to_str()
        .is_some_and(|e| Language::from_extension(e).family() == Language::C)
}

/// Add an edge for each use in a file of a type or field of the project.
fn add_data_uses(
    table: &SymbolTable,
    file: &ProjectFile,
    lines: &Lines<'_>,
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
        let start = lines.offset(o.line, o.column);
        // `Self` is written only inside the type it stands for, and says nothing about who uses it.
        let is_self = lines
            .get(o.line as usize)
            .and_then(|l| l.get(o.column as usize..o.end_column as usize))
            == Some("Self");
        if is_self || imports.iter().any(|i| i.start <= start && start < i.end) {
            continue;
        }
        if let Some(from) = table.any_at(file.file_id, start) {
            let span = Span::new(start, lines.offset(o.end_line, o.end_column));
            add_data_use(from, target, span, edges);
        }
    }
}

pub fn add_data_use(from: &Symbol, target: &Symbol, span: Span, edges: &mut EdgeSet) {
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
