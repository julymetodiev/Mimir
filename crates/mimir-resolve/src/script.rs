//! Resolution of TypeScript and JavaScript from syntax alone (`docs/TYPESCRIPT.md`, section 4).
//!
//! A call links to a function of the same file, or to one a relative import names. Anything else
//! needs types: a method above all, and a name from a package.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use mimir_graph::EdgeKind;
use mimir_parse_core::ExtractedReference;
use mimir_parse_core::extracted::CallForm;
use mimir_types::{Confidence, FileId, ReferenceKind, Symbol, SymbolKind, Visibility};

use crate::ProjectFile;
use crate::edges::{ConstantUse, EdgeSet};
use crate::lookup::{SymbolTable, is_callable, owner_of};
use crate::port::{LiveDocument, Target};
use crate::semantic::add_data_use;

/// What an import may be written without its extension as, in the order a compiler looks.
const ENDINGS: [&str; 12] = [
    ".ts",
    ".tsx",
    ".mts",
    ".cts",
    ".js",
    ".jsx",
    ".mjs",
    ".cjs",
    "/index.ts",
    "/index.tsx",
    "/index.js",
    "/index.jsx",
];

/// Add call and function value edges for every file not in `covered`.
pub fn resolve_calls(
    table: &SymbolTable,
    files: &[ProjectFile],
    covered: &HashSet<FileId>,
    edges: &mut EdgeSet,
) {
    let by_path: HashMap<&Path, FileId> = files
        .iter()
        .map(|f| (f.path.as_path(), f.file_id))
        .collect();
    for file in files.iter().filter(|f| !covered.contains(&f.file_id)) {
        for reference in &file.references {
            let (from, kind) = match reference.kind {
                ReferenceKind::Call => (
                    table.callable_at(
                        file.file_id,
                        reference.range.start_line,
                        reference.range.start_col,
                    ),
                    EdgeKind::Calls,
                ),
                ReferenceKind::FunctionValue => (
                    table.any_at(file.file_id, reference.span.start),
                    EdgeKind::UsesAsValue,
                ),
                _ => continue,
            };
            if !matches!(
                reference.call_form,
                Some(CallForm::Plain | CallForm::Shorthand)
            ) {
                continue;
            }
            let Some(from) = from else {
                continue;
            };
            if let Some(target) = target_of(table, file, &by_path, &reference.name) {
                edges.add(from.id, target.id, kind, reference.span, Confidence::High);
            }
        }
    }
}

fn target_of(
    table: &SymbolTable,
    file: &ProjectFile,
    by_path: &HashMap<&Path, FileId>,
    name: &str,
) -> Option<Arc<Symbol>> {
    let free = |file_id: FileId| -> Vec<Arc<Symbol>> {
        table
            .callables(name)
            .iter()
            .filter(|s| s.file_id == file_id && owner_of(s).is_none())
            .cloned()
            .collect()
    };
    let local = free(file.file_id);
    if !local.is_empty() {
        return only(local);
    }
    let path = imported_from(file, name)?;
    let source = resolve_import(&file.path, path, by_path)?;
    only(
        free(source)
            .into_iter()
            .filter(|s| s.visibility == Visibility::Public)
            .collect(),
    )
}

/// Add the edges of a file a language server answered for.
pub fn resolve_answers(
    table: &SymbolTable,
    file: &ProjectFile,
    document: &LiveDocument,
    by_path: &HashMap<&Path, &ProjectFile>,
    edges: &mut EdgeSet,
    constant_uses: &mut Vec<ConstantUse>,
) {
    let mut answer = Answers {
        table,
        by_path,
        edges,
        constant_uses,
    };
    for reference in &file.references {
        let site = (reference.range.start_line, reference.range.start_col);
        let Some(targets) = document.all_sites.get(&site) else {
            continue;
        };
        if reference.kind == ReferenceKind::TypeUse {
            answer.heritage(file, reference, targets);
        }
        answer.site(file, reference, targets);
    }
}

struct Answers<'a> {
    table: &'a SymbolTable,
    by_path: &'a HashMap<&'a Path, &'a ProjectFile>,
    edges: &'a mut EdgeSet,
    constant_uses: &'a mut Vec<ConstantUse>,
}

impl Answers<'_> {
    fn site(&mut self, file: &ProjectFile, reference: &ExtractedReference, targets: &[Target]) {
        let span = reference.span;
        let Some(from) = self.table.any_at(file.file_id, span.start).cloned() else {
            return;
        };
        // Answers that are one symbol make a certain edge. Several are possible targets.
        let mut callables = Vec::new();
        for target in targets {
            let Some(defined_in) = self.by_path.get(target.path.as_path()) else {
                continue;
            };
            let id = defined_in.file_id;
            let table = self.table;
            if reference.kind == ReferenceKind::Call
                && let Some(class) = table
                    .data_at(id, target.line, &target.name)
                    .filter(|s| s.kind == SymbolKind::Class)
            {
                let built = table.constructor_of(id, &class.name).unwrap_or(class);
                if !callables.iter().any(|c: &Arc<Symbol>| c.id == built.id) {
                    callables.push(built.clone());
                }
            } else if let Some(callable) = table
                .definition_at(id, target.line, &target.name)
                .filter(|s| s.name == target.name)
                .or_else(|| table.definition_after(id, target.line, &target.name))
                .filter(|s| is_callable(s))
            {
                let wanted = match reference.kind {
                    ReferenceKind::FieldUse => {
                        accessor(callable, defined_in) != Some(Accessor::Set)
                    }
                    ReferenceKind::FieldWrite => {
                        accessor(callable, defined_in) != Some(Accessor::Get)
                    }
                    _ => true,
                };
                if wanted && !callables.iter().any(|c: &Arc<Symbol>| c.id == callable.id) {
                    callables.push(callable.clone());
                }
            } else if let Some(constant) = table.constant_at(id, target.line, &target.name) {
                self.constant_uses.push(ConstantUse {
                    from: from.id,
                    constant: constant.id,
                    span,
                    confidence: Confidence::Certain,
                });
            } else if let Some(data) = table.data_at(id, target.line, &target.name) {
                add_data_use(&from, data, span, self.edges);
            }
        }
        let Some(caller) = self.table.callable_at(
            file.file_id,
            reference.range.start_line,
            reference.range.start_col,
        ) else {
            for callable in &callables {
                self.edge(&from, callable, EdgeKind::UsesAsValue, reference);
            }
            return;
        };
        let certain = callables.len() == 1;
        for callable in &callables {
            let is_field = matches!(
                reference.kind,
                ReferenceKind::FieldUse | ReferenceKind::FieldWrite
            );
            let runs = reference.kind == ReferenceKind::Call
                || (is_field
                    && self
                        .by_path
                        .values()
                        .find(|f| f.file_id == callable.file_id)
                        .is_some_and(|f| accessor(callable, f).is_some()));
            let kind = if runs && certain {
                EdgeKind::Calls
            } else {
                EdgeKind::UsesAsValue
            };
            let holder = if kind == EdgeKind::Calls {
                caller.clone()
            } else {
                from.clone()
            };
            self.edge(&holder, callable, kind, reference);
        }
    }

    fn edge(&mut self, from: &Symbol, to: &Symbol, kind: EdgeKind, reference: &ExtractedReference) {
        self.edges
            .add(from.id, to.id, kind, reference.span, Confidence::Certain);
    }

    /// `class A implements B`: each method of A with a method of that name in B may be what a call
    /// through B runs.
    fn heritage(&mut self, file: &ProjectFile, reference: &ExtractedReference, targets: &[Target]) {
        if !matches!(reference.context.as_deref(), Some("implements" | "extends")) {
            return;
        }
        let Some(class) = self
            .table
            .any_at(file.file_id, reference.span.start)
            .cloned()
        else {
            return;
        };
        for target in targets {
            let Some(defined_in) = self.by_path.get(target.path.as_path()) else {
                continue;
            };
            for method in self.table.methods_in_owner(file.file_id, &class.name) {
                let declared = self
                    .table
                    .methods_of(&target.name, &method.name)
                    .iter()
                    .find(|s| s.file_id == defined_in.file_id)
                    .cloned();
                if let Some(declared) = declared {
                    self.edges.add(
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Accessor {
    Get,
    Set,
}

/// Whether a method's source says `get` or `set`, after any modifiers.
fn accessor(symbol: &Symbol, file: &ProjectFile) -> Option<Accessor> {
    let text = file
        .source
        .get(symbol.span.start as usize..symbol.span.end as usize)?;
    for word in text.split_whitespace().take(5) {
        match word {
            "get" => return Some(Accessor::Get),
            "set" => return Some(Accessor::Set),
            "static" | "public" | "private" | "protected" | "override" | "abstract"
            | "readonly" | "export" | "declare" => {}
            _ => return None,
        }
    }
    None
}

fn only(mut found: Vec<Arc<Symbol>>) -> Option<Arc<Symbol>> {
    if found.len() == 1 { found.pop() } else { None }
}

/// The relative path of the one import that binds this name as it is in the other file.
fn imported_from<'a>(file: &'a ProjectFile, name: &str) -> Option<&'a str> {
    let is_import = |r: &&ExtractedReference| r.kind == ReferenceKind::Import && r.name == name;
    let mut imports = file.references.iter().filter(is_import);
    let import = imports.next()?;
    if imports.next().is_some() || import.call_form == Some(CallForm::Bound) {
        return None;
    }
    import
        .context
        .as_deref()
        .filter(|path| path.starts_with("./") || path.starts_with("../"))
}

/// The file a relative import names.
pub fn resolve_import(
    from: &Path,
    import: &str,
    by_path: &HashMap<&Path, FileId>,
) -> Option<FileId> {
    let base = normal(&from.parent()?.join(import))?;
    if let Some(file) = by_path.get(base.as_path()) {
        return Some(*file);
    }
    let text = base.to_str()?;
    // `./a.js` in TypeScript means `a.ts`.
    let stem = text
        .strip_suffix(".js")
        .or_else(|| text.strip_suffix(".mjs"))
        .or_else(|| text.strip_suffix(".cjs"))
        .or_else(|| text.strip_suffix(".jsx"))
        .unwrap_or(text);
    [text, stem].iter().find_map(|start| {
        ENDINGS
            .iter()
            .find_map(|ending| by_path.get(Path::new(&format!("{start}{ending}"))).copied())
    })
}

/// A path without `.` and `..`, or `None` if it leaves the project.
fn normal(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_import_finds_its_file() {
        let (a, b, c, d) = (
            FileId::new(1),
            FileId::new(2),
            FileId::new(3),
            FileId::new(4),
        );
        let by_path: HashMap<&Path, FileId> = [
            (Path::new("src/a.ts"), a),
            (Path::new("src/lib/index.ts"), b),
            (Path::new("src/c.tsx"), c),
            (Path::new("d.js"), d),
        ]
        .into_iter()
        .collect();
        let from = Path::new("src/x.ts");
        assert_eq!(resolve_import(from, "./a", &by_path), Some(a));
        assert_eq!(resolve_import(from, "./a.js", &by_path), Some(a));
        assert_eq!(resolve_import(from, "./lib", &by_path), Some(b));
        assert_eq!(resolve_import(from, "./c", &by_path), Some(c));
        assert_eq!(resolve_import(from, "../d", &by_path), Some(d));
        assert_eq!(resolve_import(from, "../d.js", &by_path), Some(d));
        assert_eq!(resolve_import(from, "./missing", &by_path), None);
        assert_eq!(resolve_import(from, "../../out", &by_path), None);
    }
}
