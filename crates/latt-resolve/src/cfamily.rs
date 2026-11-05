//! Syntax rules of C and C++ (`docs/CPP.md`, section 4): a call of a plain name links to a function in the
//! same file with exactly one definition of that name. Against the index this agrees in C, and in C++ not
//! always, so there it holds only until `clangd` answers. Across files, no edge.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use latt_graph::EdgeKind;
use latt_parse_core::extracted::CallForm;
use latt_types::{Confidence, FileId, ReferenceKind, Symbol};

use crate::ProjectFile;
use crate::definitions::Definitions;
use crate::edges::{ConstantUse, EdgeSet};
use crate::lookup::{SymbolTable, owner_of};
use crate::port::{LiveDocument, SemanticIndex};
use crate::semantic::add_data_use;

/// Add call and function value edges for every file not in `covered`.
pub fn resolve_calls(
    table: &SymbolTable,
    files: &[ProjectFile],
    covered: &HashSet<FileId>,
    edges: &mut EdgeSet,
) {
    for file in files.iter().filter(|f| !covered.contains(&f.file_id)) {
        for reference in file.references.iter() {
            if reference.call_form != Some(CallForm::Plain) {
                continue;
            }
            let (from, kind) = match reference.kind {
                ReferenceKind::Call => (
                    table
                        .callable_at(
                            file.file_id,
                            reference.range.start_line,
                            reference.range.start_col,
                        )
                        .or_else(|| table.any_at(file.file_id, reference.span.start)),
                    EdgeKind::Calls,
                ),
                ReferenceKind::FunctionValue => (
                    table.any_at(file.file_id, reference.span.start),
                    EdgeKind::UsesAsValue,
                ),
                _ => continue,
            };
            let target = only_function(table, file.file_id, &reference.name);
            if let (Some(from), Some(target)) = (from, target) {
                edges.add(from.id, target.id, kind, reference.span, Confidence::High);
            }
        }
    }
}

/// The one function of this name in a file, which no class owns.
fn only_function(table: &SymbolTable, file: FileId, name: &str) -> Option<Arc<Symbol>> {
    let mut found = table
        .callables(name)
        .iter()
        .filter(|s| s.file_id == file && owner_of(s).is_none());
    let first = found.next()?;
    found.next().is_none().then(|| first.clone())
}

/// Add the edges of what `clangd` said of an edited file. A target is exactly what it names, at its own
/// line: nothing near it stands in for it.
pub fn resolve_answers(
    table: &SymbolTable,
    file: &ProjectFile,
    document: &LiveDocument,
    by_path: &HashMap<&Path, &ProjectFile>,
    edges: &mut EdgeSet,
    constant_uses: &mut Vec<ConstantUse>,
) {
    for reference in file.references.iter() {
        let at = (reference.range.start_line, reference.range.start_col);
        // A constructor and its class may share a line and a name, so which it is comes with the answer.
        for target in document.built_at.get(&at).into_iter().flatten() {
            if let Some(defined_in) = by_path.get(&*target.path)
                && let Some(built) =
                    table.definition_at(defined_in.file_id, target.line, &target.name)
            {
                add_use_of_function(table, file, reference, built, edges);
            }
        }
        let Some(targets) = document.all_sites.get(&at) else {
            continue;
        };
        for target in targets {
            let Some(defined_in) = by_path.get(&*target.path) else {
                continue;
            };
            let id = defined_in.file_id;
            let span = reference.span;
            let holder = table.any_at(file.file_id, span.start);
            if let (Some(constant), Some(from)) =
                (table.constant_at(id, target.line, &target.name), holder)
            {
                constant_uses.push(ConstantUse {
                    from: from.id,
                    constant: constant.id,
                    span,
                    confidence: Confidence::Certain,
                });
            } else if let Some(data) = table.data_at(id, target.line, &target.name) {
                if let Some(from) = holder {
                    add_data_use(from, data, span, edges);
                }
            } else if let Some(callable) = table.definition_at(id, target.line, &target.name) {
                add_use_of_function(table, file, reference, callable, edges);
            }
        }
    }
}

fn add_use_of_function(
    table: &SymbolTable,
    file: &ProjectFile,
    reference: &latt_parse_core::ExtractedReference,
    target: &Arc<Symbol>,
    edges: &mut EdgeSet,
) {
    // As an index is read: `new T(1)` is two sites of one place, and what it does is in the text after it.
    let after = file.source.get(reference.span.end as usize..).unwrap_or("");
    let (from, kind) = if is_call(after) {
        let caller = table.callable_at(
            file.file_id,
            reference.range.start_line,
            reference.range.start_col,
        );
        (caller, EdgeKind::Calls)
    } else {
        (
            table.any_at(file.file_id, reference.span.start),
            EdgeKind::UsesAsValue,
        )
    };
    if let Some(from) = from {
        edges.add(
            from.id,
            target.id,
            kind,
            reference.span,
            Confidence::Certain,
        );
    }
}

/// Add an edge for each pair of an override and what it overrides in an index. What overrides what is
/// said by ids, so a pair holds after an edit while both are found. A method added since has none.
pub fn add_index_implementations(
    index: &SemanticIndex,
    definitions: &Definitions<'_>,
    edges: &mut EdgeSet,
) {
    for (overrider, overridden) in &index.implementations {
        for method in definitions.all(overrider) {
            for declared in definitions.all(overridden) {
                if method.id != declared.id {
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
    }
}

/// Whether the text after a function's name makes it a call: `(`, or `<...>(`. A comparison, as in
/// `f < x`, never closes, and so is no call.
pub fn is_call(after: &str) -> bool {
    let after = after.trim_start();
    let Some(arguments) = after.strip_prefix('<') else {
        return after.starts_with('(');
    };
    let mut depth = 1;
    for (at, c) in arguments.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return arguments[at + 1..].trim_start().starts_with('(');
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_a_call_from_a_value() {
        assert!(is_call("(1)"));
        assert!(is_call(" (1)"));
        assert!(is_call("<int>(1)"));
        assert!(is_call("<Box<int>>(1)"));
        assert!(!is_call(";"));
        assert!(!is_call(", other)"));
        assert!(!is_call(" < x;"));
        assert!(!is_call(""));
    }
}
