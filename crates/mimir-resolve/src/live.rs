//! Rust's edited files: edges from a running analyzer's answers.

use std::collections::HashMap;
use std::path::Path;

use mimir_graph::EdgeKind;
use mimir_types::{Confidence, ReferenceKind, SymbolKind};

use crate::ProjectFile;
use crate::edges::{ConstantUse, EdgeSet};
use crate::lookup::{SymbolTable, trait_of};
use crate::port::LiveDocument;
use crate::semantic::add_data_use;

/// Add the edges of an edited file.
pub fn resolve_live(
    table: &SymbolTable,
    file: &ProjectFile,
    document: &LiveDocument,
    by_path: &HashMap<&Path, &ProjectFile>,
    edges: &mut EdgeSet,
    constant_uses: &mut Vec<ConstantUse>,
) {
    for reference in file.references.iter() {
        let (line, column) = (reference.range.start_line, reference.range.start_col);
        if reference.kind == ReferenceKind::FieldUse {
            let field = document
                .all_sites
                .get(&(line, column))
                .into_iter()
                .flatten()
                .find_map(|t| {
                    let defined_in = by_path.get(&*t.path)?.file_id;
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
                let defined_in = by_path.get(&*t.path)?.file_id;
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
        let target = document
            .sites
            .get(&(line, column))
            .and_then(|t| table.definition_at(by_path.get(&*t.path)?.file_id, t.line, &t.name));
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
                let defined_in = by_path.get(&*declared.path)?;
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
