//! Which symbol of the graph the symbols of an index mean, read from the documents of that index.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use mimir_types::FileId;

use crate::definitions::{Definitions, is_data, is_function, is_term};
use crate::lines::Lines;
use crate::lookup::{SymbolTable, is_callable, is_data as is_graph_data};
use crate::port::{Occurrence, SemanticIndex};
use crate::{ProjectFile, golang};

/// A file of an index, its lines, and its occurrences.
pub type Document<'a> = (&'a ProjectFile, Lines<'a>, &'a Vec<Occurrence>);

pub fn documents_of<'a>(
    index: &'a SemanticIndex,
    by_path: &HashMap<&Path, &'a ProjectFile>,
) -> Vec<Document<'a>> {
    index
        .documents
        .iter()
        .filter_map(|(path, occurrences)| {
            let file = by_path.get(index.root.join(path).as_path())?;
            Some((*file, Lines::new(&file.source), occurrences))
        })
        .collect()
}

/// Which symbol of the graph the symbols of an index mean.
#[derive(Default)]
pub struct Known<'a> {
    pub definitions: Definitions<'a>,
    pub constants: Definitions<'a>,
    pub data: Definitions<'a>,
}

impl<'a> Known<'a> {
    pub fn add(
        &mut self,
        table: &SymbolTable,
        index: &SemanticIndex,
        documents: &[Document<'a>],
        covered: &mut HashSet<FileId>,
    ) {
        for (file, lines, occurrences) in documents {
            if !index.unanswered.contains(&file.path) {
                covered.insert(file.file_id);
            }
            let id = file.file_id;
            if index.is_edited(&file.path) {
                self.definitions.add_by_order(
                    occurrences,
                    is_function,
                    |name| table.definitions_named(id, name),
                    |_| true,
                );
                self.constants.add_by_order(
                    occurrences,
                    is_term,
                    |name| table.constants_named(id, name),
                    |_| true,
                );
                self.data.add_by_order(
                    occurrences,
                    is_data,
                    |name| table.data_named(id, name),
                    |_| true,
                );
                continue;
            }
            self.definitions
                .add_by_position(occurrences, lines, is_function, |line, name| {
                    table.definition_at(id, line, name)
                });
            self.constants
                .add_by_position(occurrences, lines, is_term, |line, name| {
                    table.constant_at(id, line, name)
                });
            self.data
                .add_by_position(occurrences, lines, is_data, |line, name| {
                    table.data_at(id, line, name)
                });
        }
    }

    /// The same for Go, where an interface's method looks like a field in an index, so in an edited
    /// file each form of a symbol has its own order.
    pub fn add_go(
        &mut self,
        table: &SymbolTable,
        index: &SemanticIndex,
        documents: &[Document<'a>],
        covered: &mut HashSet<FileId>,
    ) {
        for (file, lines, occurrences) in documents {
            if !index.unanswered.contains(&file.path) {
                covered.insert(file.file_id);
            }
            let id = file.file_id;
            if index.is_edited(&file.path) {
                self.definitions.add_by_order(
                    occurrences,
                    is_function,
                    |name| golang::functions_named(table, id, name),
                    |_| true,
                );
                self.definitions.add_by_order(
                    occurrences,
                    golang::is_member_term,
                    |name| golang::members_named(table, id, name),
                    is_callable,
                );
                self.definitions.add_by_order(
                    occurrences,
                    golang::is_package_term,
                    |name| golang::bound_functions_named(table, id, name),
                    |_| true,
                );
                self.constants.add_by_order(
                    occurrences,
                    golang::is_package_term,
                    |name| table.constants_named(id, name),
                    |_| true,
                );
                self.data.add_by_order(
                    occurrences,
                    golang::is_member_term,
                    |name| golang::members_named(table, id, name),
                    is_graph_data,
                );
                self.data.add_by_order(
                    occurrences,
                    golang::is_type,
                    |name| golang::types_named(table, id, name),
                    |_| true,
                );
                continue;
            }
            self.definitions.add_by_position(
                occurrences,
                lines,
                golang::may_be_callable,
                |line, name| table.definition_at(id, line, name),
            );
            self.constants
                .add_by_position(occurrences, lines, is_term, |line, name| {
                    table.constant_at(id, line, name)
                });
            self.data
                .add_by_position(occurrences, lines, is_data, |line, name| {
                    table.data_at(id, line, name)
                });
        }
    }
}
