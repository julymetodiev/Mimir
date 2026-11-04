//! What is Go's own in resolution (`docs/GO.md`, section 4): what a symbol of its index means, its
//! pairs of implementations, the answers of `textDocument/implementation`, and syntax rules.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mimir_graph::EdgeKind;
use mimir_parse_core::ExtractedReference;
use mimir_parse_core::extracted::CallForm;
use mimir_types::{Confidence, FileId, ReferenceKind, Symbol, SymbolKind};

use crate::ProjectFile;
use crate::definitions::{Definitions, is_function, is_term};
use crate::edges::EdgeSet;
use crate::lookup::{SymbolTable, is_callable, is_data as is_graph_data, owner_of};
use crate::port::{LiveDocument, SemanticIndex};

/// An interface's method ends in `.` where it is defined, the same as a field, so what such a symbol
/// is comes from the symbol of the graph at its definition.
pub fn may_be_callable(symbol: &str) -> bool {
    is_function(symbol) || is_term(symbol)
}

/// A field or an interface's method where it is defined: `T#x.`.
pub fn is_member_term(symbol: &str) -> bool {
    is_term(symbol) && symbol.contains('#')
}

/// A constant, a variable, or a function bound to one: `x.`, with no type.
pub fn is_package_term(symbol: &str) -> bool {
    is_term(symbol) && !symbol.contains('#')
}

/// A type: `T#`.
pub fn is_type(symbol: &str) -> bool {
    symbol.ends_with('#') && !symbol.starts_with("local ")
}

/// Those with this name in a file that are wanted, in the order they are written.
fn named<'t>(
    table: &'t SymbolTable,
    file: FileId,
    name: &str,
    wanted: impl Fn(&Symbol) -> bool,
) -> Vec<&'t Arc<Symbol>> {
    let mut named: Vec<_> = table
        .named(name)
        .iter()
        .filter(|s| s.file_id == file && wanted(s))
        .collect();
    named.sort_by_key(|s| s.span.start);
    named
}

/// Whether a method is one an interface declares, which an index writes as a field where it is
/// defined. An interface's methods are written inside it, so it is in the same file.
fn of_an_interface(table: &SymbolTable, method: &Symbol) -> bool {
    method.kind == SymbolKind::Method
        && owner_of(method).is_some_and(|owner| {
            table
                .named(owner)
                .iter()
                .any(|s| s.kind == SymbolKind::Interface && s.file_id == method.file_id)
        })
}

/// In an edited file, what `f().` or `T#m().` can mean (`docs/GO.md`, section 4): functions and the
/// methods of types, as an interface's are in another form.
pub fn functions_named<'t>(
    table: &'t SymbolTable,
    file: FileId,
    name: &str,
) -> Vec<&'t Arc<Symbol>> {
    named(table, file, name, |s| {
        is_callable(s) && !of_an_interface(table, s)
    })
}

/// In an edited file, what `T#x.` can mean: fields and interfaces' methods.
pub fn members_named<'t>(table: &'t SymbolTable, file: FileId, name: &str) -> Vec<&'t Arc<Symbol>> {
    named(table, file, name, |s| {
        s.kind == SymbolKind::Field || of_an_interface(table, s)
    })
}

/// In an edited file, the functions bound to a variable that `x.` can mean.
pub fn bound_functions_named<'t>(
    table: &'t SymbolTable,
    file: FileId,
    name: &str,
) -> Vec<&'t Arc<Symbol>> {
    named(table, file, name, |s| s.kind == SymbolKind::Function)
}

/// In an edited file, the types that `T#` can mean.
pub fn types_named<'t>(table: &'t SymbolTable, file: FileId, name: &str) -> Vec<&'t Arc<Symbol>> {
    named(table, file, name, |s| {
        is_graph_data(s) && s.kind != SymbolKind::Field
    })
}

/// The other form of an interface's method: `Object#GetName().` where it is used in another module,
/// and `Object#GetName.` where it is defined. A type in Go has no field and method of one name, so
/// this cannot find a wrong target.
pub fn other_form(symbol: &str) -> Option<String> {
    if let Some(method) = symbol.strip_suffix("().") {
        return Some(format!("{method}."));
    }
    let term = symbol.strip_suffix('.')?;
    term.contains('#').then(|| format!("{term}()."))
}

/// What a symbol means in a file, in the form it has or the other one.
pub fn definition<'d>(
    definitions: &'d Definitions<'_>,
    symbol: &str,
    file: FileId,
) -> Option<&'d Arc<Symbol>> {
    definitions
        .get(symbol, file)
        .or_else(|| other_form(symbol).and_then(|other| definitions.get(other.as_str(), file)))
}

/// Whether a method is declared by an interface.
fn of_interface(table: &SymbolTable, method: &Symbol) -> bool {
    table
        .named(owner_of(method).unwrap_or(""))
        .iter()
        .any(|owner| {
            owner.kind == SymbolKind::Interface
                && owner.file_id == method.file_id
                && owner.span.start <= method.span.start
                && method.span.end <= owner.span.end
        })
}

/// Add an edge for each pair of methods in the indexes, and none for a pair of types. A pair with a
/// side in an edited file no longer holds: `implementation` answers for those.
pub fn add_index_implementations(
    table: &SymbolTable,
    indexes: &[SemanticIndex],
    definitions: &Definitions<'_>,
    edited: &HashSet<FileId>,
    edges: &mut EdgeSet,
) {
    let found = |symbol: &str| -> Vec<Arc<Symbol>> {
        let direct = definitions.all(symbol);
        if !direct.is_empty() {
            return direct.to_vec();
        }
        other_form(symbol).map_or(Vec::new(), |other| definitions.all(&other).to_vec())
    };
    for (method, declared) in indexes.iter().flat_map(|index| &index.implementations) {
        for method in found(method) {
            for declared in found(declared) {
                if method.id != declared.id
                    && !edited.contains(&method.file_id)
                    && !edited.contains(&declared.file_id)
                    && of_interface(table, &declared)
                {
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

/// Add an edge for each answer of `textDocument/implementation` in an edited file. On a concrete
/// method it gives the interfaces, and on an interface's method what implements it.
pub fn add_answered_implementations(
    table: &SymbolTable,
    file: &ProjectFile,
    document: &LiveDocument,
    by_path: &HashMap<&Path, &ProjectFile>,
    edges: &mut EdgeSet,
) {
    for (&(line, column), targets) in &document.implementations {
        let Some(asked) = table.callable_at(file.file_id, line, column) else {
            continue;
        };
        for target in targets {
            let found = by_path
                .get(&*target.path)
                .and_then(|defined_in| {
                    table.definition_at(defined_in.file_id, target.line, &target.name)
                })
                .filter(|found| found.id != asked.id);
            let Some(found) = found else {
                continue;
            };
            let (method, declared) = if of_interface(table, found) {
                (asked, found)
            } else {
                (found, asked)
            };
            if of_interface(table, declared) {
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

/// Whether the text after a function's name makes it a call: `(`, or `[...](`.
pub fn is_call(after: &str) -> bool {
    let after = after.trim_start();
    let Some(arguments) = after.strip_prefix('[') else {
        return after.starts_with('(');
    };
    let mut depth = 1;
    for (at, c) in arguments.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return arguments[at + 1..].starts_with('(');
        }
    }
    // Type arguments running past the end of the line: only a call or a value can follow.
    false
}

/// Add call and function value edges for every file not in `covered`, from syntax alone: a function
/// of the same package with exactly one definition of that name, and `pkg.F()` likewise when `pkg` is
/// imported from a directory of the project. No methods.
pub fn resolve_calls(
    table: &SymbolTable,
    files: &[ProjectFile],
    indexes: &[SemanticIndex],
    covered: &HashSet<FileId>,
    edges: &mut EdgeSet,
) {
    let mut packages: HashMap<&Path, Vec<FileId>> = HashMap::new();
    for file in files {
        if let Some(directory) = file.path.parent() {
            packages.entry(directory).or_default().push(file.file_id);
        }
    }
    let sources: HashMap<FileId, &str> = files.iter().map(|f| (f.file_id, &*f.source)).collect();
    let modules: Vec<(&str, &Path)> = indexes
        .iter()
        .filter_map(|index| Some((index.import_path.as_deref()?, index.root.as_path())))
        .collect();
    for file in files.iter().filter(|f| !covered.contains(&f.file_id)) {
        for reference in &file.references {
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
            let directory = match reference.call_form {
                Some(CallForm::Plain) => file.path.parent().map(Path::to_path_buf),
                Some(CallForm::Scoped) => reference
                    .context
                    .as_deref()
                    .and_then(|alias| imported_path(&file.references, alias))
                    .and_then(|import| directory_of(import, &modules)),
                _ => None,
            };
            let target = directory.and_then(|directory| {
                let in_package = packages.get(directory.as_path())?;
                let target = only_function(table, in_package, &reference.name)?;
                // An import that names no package is known by what the package calls itself.
                let named = reference.call_form != Some(CallForm::Scoped)
                    || reference.context.as_deref().is_some_and(|alias| {
                        says_its_name(&file.references, alias)
                            || sources
                                .get(&target.file_id)
                                .is_some_and(|source| declares_package(source, alias))
                    });
                named.then_some(target)
            });
            if let (Some(from), Some(target)) = (from, target) {
                edges.add(from.id, target.id, kind, reference.span, Confidence::High);
            }
        }
    }
}

/// The one function of this name in a package's files.
fn only_function(table: &SymbolTable, package: &[FileId], name: &str) -> Option<Arc<Symbol>> {
    let mut found = table
        .callables(name)
        .iter()
        .filter(|s| package.contains(&s.file_id) && owner_of(s).is_none());
    let first = found.next()?;
    found.next().is_none().then(|| first.clone())
}

/// What an import of this name in a file is the path of.
fn imported_path<'a>(references: &'a [ExtractedReference], name: &str) -> Option<&'a str> {
    references
        .iter()
        .find(|r| r.kind == ReferenceKind::Import && r.name == name)?
        .context
        .as_deref()
}

/// Whether a file's import of this name gave the name itself, as in `import al "x/y"`.
fn says_its_name(references: &[ExtractedReference], name: &str) -> bool {
    references.iter().any(|r| {
        r.kind == ReferenceKind::Import && r.name == name && r.call_form == Some(CallForm::Bound)
    })
}

/// Whether a file's `package` line gives this name.
fn declares_package(source: &str, name: &str) -> bool {
    source.lines().any(|line| {
        line.strip_prefix("package ")
            .and_then(|rest| rest.split_whitespace().next())
            == Some(name)
    })
}

/// The directory of an import, if a module of the project holds it: the longest path that fits.
fn directory_of(import: &str, modules: &[(&str, &Path)]) -> Option<PathBuf> {
    modules
        .iter()
        .filter_map(|(module, root)| {
            let inner = import.strip_prefix(module)?;
            let inner = match inner.strip_prefix('/') {
                Some(inner) => inner,
                None if inner.is_empty() => "",
                None => return None,
            };
            Some((module.len(), root.join(inner)))
        })
        .max_by_key(|(length, _)| *length)
        .map(|(_, directory)| directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_a_call_from_a_value() {
        assert!(is_call("(1)"));
        assert!(is_call("[int](1)"));
        assert!(is_call("[map[string]int](1)"));
        assert!(!is_call(")"));
        assert!(!is_call("[int])"));
        assert!(!is_call("[0]"));
        assert!(!is_call(""));
    }

    #[test]
    fn the_other_form_of_a_symbol_is_only_for_a_member() {
        assert_eq!(
            other_form("a/Object#GetName().").as_deref(),
            Some("a/Object#GetName.")
        );
        assert_eq!(
            other_form("a/Object#GetName.").as_deref(),
            Some("a/Object#GetName().")
        );
        assert_eq!(other_form("a/helper().").as_deref(), Some("a/helper."));
        assert_eq!(other_form("a/LIMIT."), None);
        assert_eq!(other_form("a/Object#"), None);
    }

    #[test]
    fn an_import_finds_the_directory_of_the_longest_module() {
        let modules = [
            ("example.com/a", Path::new("")),
            ("example.com/a/nested", Path::new("deep/nested")),
        ];
        let directory = |import| directory_of(import, &modules);
        assert_eq!(directory("example.com/a"), Some(PathBuf::from("")));
        assert_eq!(
            directory("example.com/a/inner"),
            Some(PathBuf::from("inner"))
        );
        assert_eq!(
            directory("example.com/a/nested/x"),
            Some(PathBuf::from("deep/nested/x"))
        );
        assert_eq!(directory("example.com/ab"), None);
        assert_eq!(directory("encoding/json"), None);
    }
}
