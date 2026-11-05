//! The symbol lookups both resolvers need.

use std::collections::HashMap;
use std::sync::Arc;

use mimir_graph::CodeGraph;
use mimir_types::{FileId, Language, Symbol, SymbolKind};

use crate::ProjectFile;

/// Whether a symbol is a function, method or constructor.
pub fn is_callable(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

/// Whether a symbol is a type, trait, alias, variant or field.
pub fn is_data(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Struct
            | SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::EnumVariant
            | SymbolKind::Union
            | SymbolKind::TypeAlias
            | SymbolKind::Trait
            | SymbolKind::Field
    )
}

/// Whether a symbol is a constant or static, or a variable at package or file level.
pub fn is_constant(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Constant
        || (symbol.kind == SymbolKind::Variable
            && matches!(symbol.language, Language::Go | Language::C | Language::Cpp))
}

/// The type or trait a method belongs to: `Foo` for a method of `impl<T> Tr for a::Foo<T>`.
pub fn owner_of(symbol: &Symbol) -> Option<&str> {
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
pub fn trait_of(symbol: &Symbol) -> Option<&str> {
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
pub fn implements_generic_trait(symbol: &Symbol) -> bool {
    let segments = symbol.qualified_name.segments();
    segments
        .len()
        .checked_sub(2)
        .and_then(|i| segments[i].strip_prefix("impl "))
        .and_then(|parent| parent.rsplit_once(" for "))
        .is_some_and(|(implemented, _)| implemented.contains('<'))
}

/// The bare name of a type path: `Foo` for `&a::b::Foo<T>`.
pub fn type_name(path: &str) -> &str {
    let base = path.split('<').next().unwrap_or(path).trim_end();
    let base = base.rsplit("::").next().unwrap_or(base);
    base.trim_start_matches(['&', ' '])
        .trim_start_matches("mut ")
        .trim()
}

/// The callable symbols of a project, indexed by file, name and owner.
pub struct SymbolTable {
    by_file: HashMap<FileId, Vec<Arc<Symbol>>>,
    by_name: HashMap<String, Vec<Arc<Symbol>>>,
    by_owner: HashMap<(String, String), Vec<Arc<Symbol>>>,
    named: HashMap<String, Vec<Arc<Symbol>>>,
    all_by_file: HashMap<FileId, Vec<Arc<Symbol>>>,
}

impl SymbolTable {
    pub fn new(graph: &CodeGraph, files: &[ProjectFile]) -> Self {
        let mut table = Self {
            by_file: HashMap::new(),
            by_name: HashMap::new(),
            by_owner: HashMap::new(),
            named: HashMap::new(),
            all_by_file: HashMap::new(),
        };
        for file in files {
            for symbol in graph.symbols_in_file(file.file_id) {
                table
                    .all_by_file
                    .entry(file.file_id)
                    .or_default()
                    .push(symbol.clone());
                table
                    .named
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(symbol.clone());
                if !is_callable(&symbol) {
                    continue;
                }
                table
                    .by_file
                    .entry(file.file_id)
                    .or_default()
                    .push(symbol.clone());
                table
                    .by_name
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(symbol.clone());
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
    pub fn callable_at(&self, file: FileId, line: u32, column: u32) -> Option<&Arc<Symbol>> {
        self.by_file
            .get(&file)?
            .iter()
            .filter(|s| holds(s, line, column))
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The innermost function or method with this name holding a line.
    pub fn definition_at(&self, file: FileId, line: u32, name: &str) -> Option<&Arc<Symbol>> {
        self.by_file
            .get(&file)?
            .iter()
            .filter(|s| s.name == name && s.range.start_line <= line && line <= s.range.end_line)
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The functions and methods with this name in a file, in the order they are written.
    pub fn definitions_named(&self, file: FileId, name: &str) -> Vec<&Arc<Symbol>> {
        let mut named: Vec<_> = self
            .by_file
            .get(&file)
            .into_iter()
            .flatten()
            .filter(|s| s.name == name)
            .collect();
        named.sort_by_key(|s| s.span.start);
        named
    }

    /// The next function or method with this name from a line on: what an overload's signature
    /// declares, since overloads are one symbol, at the body.
    pub fn definition_after(&self, file: FileId, line: u32, name: &str) -> Option<&Arc<Symbol>> {
        self.by_file
            .get(&file)?
            .iter()
            .filter(|s| s.name == name && s.range.start_line >= line)
            .min_by_key(|s| s.range.start_line)
    }

    /// The methods a class declares in a file.
    pub fn methods_in_owner(&self, file: FileId, owner: &str) -> Vec<Arc<Symbol>> {
        self.by_file
            .get(&file)
            .into_iter()
            .flatten()
            .filter(|s| owner_of(s) == Some(owner))
            .cloned()
            .collect()
    }

    /// The constructor a class declares in a file.
    pub fn constructor_of(&self, file: FileId, class: &str) -> Option<&Arc<Symbol>> {
        self.by_file.get(&file)?.iter().find(|s| {
            s.kind == SymbolKind::Constructor
                && s.qualified_name.segments().first().map(String::as_str) == Some(class)
        })
    }

    /// The constant or static with this name declared on a line.
    pub fn constant_at(&self, file: FileId, line: u32, name: &str) -> Option<&Arc<Symbol>> {
        self.declared_at(file, line, name, is_constant)
    }

    /// The type, trait, variant or field with this name declared on a line.
    pub fn data_at(&self, file: FileId, line: u32, name: &str) -> Option<&Arc<Symbol>> {
        self.declared_at(file, line, name, is_data)
    }

    fn declared_at(
        &self,
        file: FileId,
        line: u32,
        name: &str,
        wanted: fn(&Symbol) -> bool,
    ) -> Option<&Arc<Symbol>> {
        self.all_by_file
            .get(&file)?
            .iter()
            .filter(|s| {
                wanted(s)
                    && s.name == name
                    && s.range.start_line <= line
                    && line <= s.range.end_line
            })
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The constants and statics with this name in a file, in the order they are written.
    pub fn constants_named(&self, file: FileId, name: &str) -> Vec<&Arc<Symbol>> {
        self.declared_named(file, name, is_constant)
    }

    /// The types, traits, variants and fields with this name in a file, in the order written.
    pub fn data_named(&self, file: FileId, name: &str) -> Vec<&Arc<Symbol>> {
        self.declared_named(file, name, is_data)
    }

    fn declared_named(
        &self,
        file: FileId,
        name: &str,
        wanted: fn(&Symbol) -> bool,
    ) -> Vec<&Arc<Symbol>> {
        let mut named: Vec<_> = self
            .all_by_file
            .get(&file)
            .into_iter()
            .flatten()
            .filter(|s| wanted(s) && s.name == name)
            .collect();
        named.sort_by_key(|s| s.span.start);
        named
    }

    /// The type or trait a name without a path means at a byte offset, if syntax can tell: the only
    /// one with this name in the same module of the file, and not inside a function elsewhere. An
    /// import of the same name would not compile beside it, but one in another module could.
    pub fn type_in_scope(&self, file: FileId, offset: u32, name: &str) -> Option<&Arc<Symbol>> {
        let module = self.module_at(file, offset);
        let found: Vec<_> = self
            .named(name)
            .iter()
            .filter(|s| {
                s.file_id == file
                    && is_data(s)
                    && !matches!(s.kind, SymbolKind::Field | SymbolKind::EnumVariant)
                    && self.module_at(file, s.span.start) == module
                    && self
                        .callable_holding(file, s.span.start)
                        .is_none_or(|f| f.span.start <= offset && offset <= f.span.end)
            })
            .collect();
        match found.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }

    /// The constant or static with this name that is an item of the module holding `offset`, not
    /// an associated one and not inside a function.
    pub fn constant_in_module(
        &self,
        file: FileId,
        offset: u32,
        name: &str,
    ) -> Option<&Arc<Symbol>> {
        let module = self.module_at(file, offset);
        let found: Vec<_> = self
            .named(name)
            .iter()
            .filter(|s| {
                s.file_id == file
                    && is_constant(s)
                    && self.module_at(file, s.span.start) == module
                    && self.all_by_file.get(&file).is_some_and(|symbols| {
                        !symbols.iter().any(|holder| {
                            matches!(
                                holder.kind,
                                SymbolKind::Impl
                                    | SymbolKind::Trait
                                    | SymbolKind::Function
                                    | SymbolKind::Method
                            ) && holder.span.start <= s.span.start
                                && s.span.end <= holder.span.end
                        })
                    })
            })
            .collect();
        match found.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }

    fn module_at(&self, file: FileId, offset: u32) -> Option<mimir_types::SymbolId> {
        self.all_by_file
            .get(&file)?
            .iter()
            .filter(|s| {
                s.kind == SymbolKind::Module && s.span.start <= offset && offset <= s.span.end
            })
            .min_by_key(|s| s.span.end - s.span.start)
            .map(|s| s.id)
    }

    pub fn callable_holding(&self, file: FileId, offset: u32) -> Option<&Arc<Symbol>> {
        self.by_file
            .get(&file)?
            .iter()
            .filter(|s| s.span.start <= offset && offset <= s.span.end)
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// A field or variant of a type: the member with this name declared inside it.
    pub fn member_of(&self, owner: &Symbol, name: &str, kind: SymbolKind) -> Option<&Arc<Symbol>> {
        self.named(name).iter().find(|s| {
            s.file_id == owner.file_id
                && s.kind == kind
                && owner.span.start <= s.span.start
                && s.span.end <= owner.span.end
        })
    }

    /// The only constant or static with this name: in this file first, then in the project.
    pub fn only_constant(&self, file: FileId, name: &str, scoped: bool) -> Option<&Arc<Symbol>> {
        let all: Vec<_> = self
            .named(name)
            .iter()
            .filter(|s| s.kind == SymbolKind::Constant)
            .collect();
        let local: Vec<_> = all.iter().filter(|s| s.file_id == file).collect();
        // A path can lead to any crate, so the one in this file is no more likely than another.
        match (local.as_slice(), all.as_slice()) {
            (_, [one]) => Some(*one),
            ([one], _) if !scoped => Some(**one),
            _ => None,
        }
    }

    /// The innermost symbol of any kind holding a byte offset.
    pub fn any_at(&self, file: FileId, offset: u32) -> Option<&Arc<Symbol>> {
        self.all_by_file
            .get(&file)?
            .iter()
            .filter(|s| s.span.start <= offset && offset <= s.span.end)
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The innermost impl block holding a line.
    pub fn impl_at(&self, file: FileId, line: u32) -> Option<&Arc<Symbol>> {
        self.all_by_file
            .get(&file)?
            .iter()
            .filter(|s| {
                s.kind == SymbolKind::Impl && s.range.start_line <= line && line <= s.range.end_line
            })
            .min_by_key(|s| s.span.end - s.span.start)
    }

    /// The methods in a file that implement a trait.
    pub fn trait_impls_in(&self, file: FileId) -> impl Iterator<Item = (&Arc<Symbol>, &str)> {
        self.by_file
            .get(&file)
            .into_iter()
            .flatten()
            .filter_map(|s| trait_of(s).map(|t| (s, t)))
    }

    /// Functions and methods with this name.
    pub fn callables(&self, name: &str) -> &[Arc<Symbol>] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// Methods with this name on a type or trait.
    pub fn methods_of(&self, owner: &str, name: &str) -> &[Arc<Symbol>] {
        self.by_owner
            .get(&(owner.to_string(), name.to_string()))
            .map_or(&[], Vec::as_slice)
    }

    /// Symbols of any kind with this name.
    pub fn named(&self, name: &str) -> &[Arc<Symbol>] {
        self.named.get(name).map_or(&[], Vec::as_slice)
    }
}

fn holds(symbol: &Symbol, line: u32, column: u32) -> bool {
    let r = &symbol.range;
    (r.start_line, r.start_col) <= (line, column) && (line, column) <= (r.end_line, r.end_col)
}
