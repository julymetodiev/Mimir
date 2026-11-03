//! Resolution from syntax alone.
//!
//! Without types `x.get()` can name any `get`, so a call is linked only when its form
//! allows one target. What is left out is a miss the caller can see in [`crate::LinkReport`];
//! a guess would be a wrong answer nobody can see.

use std::collections::HashSet;
use std::sync::Arc;

use mimir_graph::EdgeKind;
use mimir_parse_core::extracted::CallForm;
use mimir_types::{Confidence, FileId, ReferenceKind, Symbol};

use crate::lookup::{SymbolTable, implements_generic_trait, owner_of, trait_of, type_name};
use crate::{ConstantUse, EdgeSet, ProjectFile};

/// Add call and function value edges for every file not in `covered`.
pub(crate) fn resolve_calls(
    table: &SymbolTable,
    files: &[ProjectFile],
    covered: &HashSet<FileId>,
    edges: &mut EdgeSet,
) {
    for file in files.iter().filter(|f| !covered.contains(&f.file_id)) {
        for reference in &file.references {
            // A value can sit outside any function, as in `const HANDLERS: &[fn()] = &[run];`.
            let (from, kind) = match reference.kind {
                ReferenceKind::Call => {
                    let (line, column) = (reference.range.start_line, reference.range.start_col);
                    (
                        table.callable_at(file.file_id, line, column),
                        EdgeKind::Calls,
                    )
                }
                ReferenceKind::FunctionValue => (
                    table.any_at(file.file_id, reference.span.start),
                    EdgeKind::UsesAsValue,
                ),
                _ => continue,
            };
            let (Some(from), Some(form)) = (from, reference.call_form) else {
                continue;
            };
            let qualifier = reference.context.as_deref();
            if let Some(target) =
                resolve_call(table, file.file_id, from, form, &reference.name, qualifier)
            {
                edges.add(from.id, target.id, kind, reference.span, Confidence::High);
            }
        }
    }
}

fn resolve_call(
    table: &SymbolTable,
    file: FileId,
    caller: &Symbol,
    form: CallForm,
    name: &str,
    qualifier: Option<&str>,
) -> Option<Arc<Symbol>> {
    match form {
        CallForm::Plain | CallForm::Shorthand => free_function(table, file, name),
        CallForm::Scoped => {
            let path = qualifier.unwrap_or("");
            let owner = match type_name(path) {
                "Self" => owner_of(caller)?,
                other => other,
            };
            if owner.starts_with(char::is_uppercase) {
                // `Vec::new` must not land on a project's `new` when `Vec` has no methods here.
                return match table.methods_of(owner, name) {
                    [one] if !implements_generic_trait(one) => Some(one.clone()),
                    _ => None,
                };
            }
            if ["std", "core", "alloc"].contains(&path.split("::").next().unwrap_or("")) {
                return None;
            }
            // A module path can lead to any crate, so the name must be the only one.
            match table.callables(name) {
                [one] if owner_of(one).is_none() => Some(one.clone()),
                _ => None,
            }
        }
        CallForm::Method => {
            if qualifier != Some("self") {
                return None;
            }
            match table.methods_of(owner_of(caller)?, name) {
                [one] => Some(one.clone()),
                _ => None,
            }
        }
    }
}

/// Add edges for functions in `Struct { name }`, in files of a semantic index: its occurrence
/// there is the field, so the index says nothing of the function.
pub(crate) fn resolve_shorthand_values(
    table: &SymbolTable,
    files: &[ProjectFile],
    covered: &HashSet<FileId>,
    edges: &mut EdgeSet,
) {
    for file in files.iter().filter(|f| covered.contains(&f.file_id)) {
        for reference in file.references.iter().filter(|r| {
            r.kind == ReferenceKind::FunctionValue && r.call_form == Some(CallForm::Shorthand)
        }) {
            let from = table.any_at(file.file_id, reference.span.start);
            if let (Some(from), Some(target)) =
                (from, free_function(table, file.file_id, &reference.name))
            {
                edges.add(
                    from.id,
                    target.id,
                    EdgeKind::UsesAsValue,
                    reference.span,
                    Confidence::High,
                );
            }
        }
    }
}

/// The only free function with this name: in this file first, then in the project.
fn free_function(table: &SymbolTable, file: FileId, name: &str) -> Option<Arc<Symbol>> {
    let free: Vec<_> = table
        .callables(name)
        .iter()
        .filter(|s| owner_of(s).is_none())
        .collect();
    let local: Vec<_> = free.iter().filter(|s| s.file_id == file).collect();
    match (local.as_slice(), free.as_slice()) {
        ([one], _) => Some((**one).clone()),
        ([], [one]) => Some((*one).clone()),
        _ => None,
    }
}

/// Collect uses of constants in files not in `covered`, by a name with one declaration.
pub(crate) fn resolve_constant_uses(
    table: &SymbolTable,
    files: &[ProjectFile],
    covered: &HashSet<FileId>,
    uses: &mut Vec<ConstantUse>,
) {
    for file in files.iter().filter(|f| !covered.contains(&f.file_id)) {
        for reference in file
            .references
            .iter()
            .filter(|r| r.kind == ReferenceKind::Read)
        {
            let from = table.any_at(file.file_id, reference.span.start);
            if let (Some(from), Some(constant)) =
                (from, table.only_constant(file.file_id, &reference.name))
            {
                uses.push(ConstantUse {
                    from: from.id,
                    constant: constant.id,
                    span: reference.span,
                    confidence: Confidence::High,
                });
            }
        }
    }
}

/// Link methods of `impl Tr for T` to the method of `Tr`, for files not in `covered`.
/// Two traits with one name leave the method unlinked.
pub(crate) fn resolve_implementations(
    table: &SymbolTable,
    files: &[ProjectFile],
    covered: &HashSet<FileId>,
    edges: &mut EdgeSet,
) {
    for file in files.iter().filter(|f| !covered.contains(&f.file_id)) {
        for (method, trait_name) in table.trait_impls_in(file.file_id) {
            let declared: Vec<_> = table
                .methods_of(trait_name, &method.name)
                .iter()
                .filter(|s| trait_of(s).is_none())
                .collect();
            if let [declaration] = declared.as_slice() {
                edges.add(
                    method.id,
                    declaration.id,
                    EdgeKind::Implements,
                    method.span,
                    Confidence::High,
                );
            }
        }
    }
}

/// Add import edges for `use crate::`, `self::` and `super::` paths that name one symbol.
/// Any other path may lead outside the project, where a same-named symbol would be wrong.
pub(crate) fn resolve_imports(table: &SymbolTable, files: &[ProjectFile], edges: &mut EdgeSet) {
    for file in files {
        for reference in file
            .references
            .iter()
            .filter(|r| r.kind == ReferenceKind::Import)
        {
            let path = reference.context.as_deref().unwrap_or("");
            if !["crate::", "self::", "super::"]
                .iter()
                .any(|p| path.starts_with(p))
            {
                continue;
            }
            let Some(from) = table.any_at(file.file_id, reference.span.start) else {
                continue;
            };
            if let [target] = table.named(&reference.name) {
                edges.add(
                    from.id,
                    target.id,
                    EdgeKind::Imports,
                    reference.span,
                    Confidence::High,
                );
            }
        }
    }
}
