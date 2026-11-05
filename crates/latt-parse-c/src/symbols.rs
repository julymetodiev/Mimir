//! Symbols of C and C++ (`docs/CPP.md`, section 3). A declaration and its definition are one symbol, at
//! the definition, so a function only declared is no symbol unless it is pure virtual, which nothing
//! defines. Macros are no symbols.

use std::collections::HashSet;

use latt_parse_core::ParsedTree;
use latt_parse_core::extracted::ExtractedSymbol;
use latt_parse_core::nodes::{MAX_DEPTH, module_symbol, range_of, span_of, text};
use latt_types::{Language, QualifiedName, SymbolKind, Visibility};
use tree_sitter::Node;

use crate::declarators::{declared, function_declarator, named_by};

/// Extract the symbols of a parse tree.
pub fn extract_symbols(tree: &ParsedTree, language: Language) -> Vec<ExtractedSymbol> {
    let mut walk = Walk {
        source: tree.source_bytes(),
        language,
        found: Vec::new(),
        scope: Vec::new(),
        owners: Vec::new(),
        namespaces: HashSet::new(),
    };
    let root = tree.root_node();
    walk.namespaces(&root, 0);
    walk.children(&root, false, 0);
    walk.found.push(module_symbol(tree, language));
    walk.found
}

struct Walk<'a> {
    source: &'a [u8],
    language: Language,
    found: Vec<ExtractedSymbol>,
    /// The namespaces and classes around here, as written in a qualified name.
    scope: Vec<String>,
    /// The classes around here, by the name a constructor has.
    owners: Vec<String>,
    /// So that `ns::f` written outside `ns` is told from a method of a class.
    namespaces: HashSet<&'a str>,
}

/// What a declarator names: its name, and the scopes written in it.
struct Named<'a> {
    name: String,
    scopes: Vec<&'a str>,
}

impl<'a> Walk<'a> {
    fn namespaces(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        if node.kind() == "namespace_definition"
            && let Some(name) = node.child_by_field_name("name")
        {
            let mut cursor = name.walk();
            let parts: Vec<Node> = name.named_children(&mut cursor).collect();
            for part in parts.iter().chain([&name]) {
                if part.named_child_count() == 0
                    && let Some(text) = text(part, self.source)
                {
                    self.namespaces.insert(text);
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if !matches!(child.kind(), "function_definition" | "comment") {
                self.namespaces(&child, depth + 1);
            }
        }
    }

    fn children(&mut self, node: &Node<'a>, templated: bool, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.item(&child, templated, depth + 1);
        }
    }

    fn item(&mut self, node: &Node<'a>, templated: bool, depth: usize) {
        match node.kind() {
            "function_definition" => self.function(node, templated),
            "declaration" => self.declaration(node, depth),
            "field_declaration" => self.field(node, depth),
            "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
                self.record(node, None, templated, depth);
            }
            "type_definition" => self.typedef(node, depth),
            "alias_declaration" => {
                if let Some(name) = self.name_field(node) {
                    self.add(name, SymbolKind::TypeAlias, node, &[], None, true);
                }
            }
            "namespace_definition" => self.namespace(node, depth),
            "template_declaration" => self.children(node, true, depth),
            "linkage_specification"
            | "declaration_list"
            | "field_declaration_list"
            | "translation_unit"
            | "preproc_if"
            | "preproc_ifdef"
            | "preproc_elif"
            | "preproc_elifdef"
            | "preproc_else"
            | "ERROR" => self.children(node, templated, depth),
            _ => {}
        }
    }

    fn name_field(&self, node: &Node<'a>) -> Option<&'a str> {
        text(&node.child_by_field_name("name")?, self.source)
    }

    /// Add a symbol named with what is around it and `written`, the scopes in its own declarator.
    fn add(
        &mut self,
        name: &str,
        kind: SymbolKind,
        whole: &Node<'a>,
        written: &[String],
        owner: Option<&str>,
        public: bool,
    ) {
        let segments: Vec<String> = self
            .scope
            .iter()
            .chain(written)
            .cloned()
            .chain([name.to_string()])
            .collect();
        let mut symbol = ExtractedSymbol::new(
            name.to_string(),
            kind,
            self.language,
            range_of(whole),
            span_of(whole),
        )
        .with_qualified_name(QualifiedName::new(segments))
        .with_visibility(if public {
            Visibility::Public
        } else {
            Visibility::Private
        });
        if let Some(owner) = owner {
            symbol = symbol.with_parent(owner.to_string());
        }
        self.found.push(symbol);
    }

    fn is_static(&self, node: &Node<'a>) -> bool {
        let mut cursor = node.walk();
        node.children(&mut cursor).any(|child| {
            child.kind() == "storage_class_specifier" && text(&child, self.source) == Some("static")
        })
    }

    /// Whether a declarator is `operator bool()`, or `A::operator bool()`.
    fn is_cast(&self, declarator: &Node<'a>) -> bool {
        let mut at = *declarator;
        for _ in 0..MAX_DEPTH {
            match at.kind() {
                "operator_cast" => return true,
                "qualified_identifier" => match at.child_by_field_name("name") {
                    Some(name) => at = name,
                    None => return false,
                },
                _ => return false,
            }
        }
        false
    }

    /// What a function's declarator names, and whether it is one.
    fn function_name(&self, declarator: Node<'a>) -> Option<Named<'a>> {
        let mut at = match function_declarator(declarator, 0) {
            Some(function) => declared(function.child_by_field_name("declarator")?, 0),
            // `operator bool()` has no function declarator of its own.
            None if self.is_cast(&declarator) => declarator,
            None => return None,
        };
        let mut scopes = Vec::new();
        for _ in 0..MAX_DEPTH {
            if at.kind() != "qualified_identifier" {
                break;
            }
            if let Some(scope) = at.child_by_field_name("scope") {
                scopes.push(text(&scope, self.source)?);
            }
            at = at.child_by_field_name("name")?;
        }
        if at.kind() == "template_function" {
            at = at.child_by_field_name("name")?;
        }
        if at.kind() == "operator_cast" {
            let kind = text(&at.child_by_field_name("type")?, self.source)?;
            return Some(Named {
                name: format!("operator {kind}"),
                scopes,
            });
        }
        Some(Named {
            name: text(&at, self.source)?.to_string(),
            scopes,
        })
    }

    fn function(&mut self, node: &Node<'a>, templated: bool) {
        let Some(named) = node
            .child_by_field_name("declarator")
            .and_then(|declarator| self.function_name(declarator))
        else {
            return;
        };
        // A class written in the name goes by its name alone in a template, where `Box<T>` is `Box`.
        let written: Vec<String> = named
            .scopes
            .iter()
            .map(|scope| match (templated, scope.split_once('<')) {
                (true, Some((base, _))) => base.to_string(),
                _ => scope.to_string(),
            })
            .collect();
        let of_class = named
            .scopes
            .last()
            .map(|scope| scope.split('<').next().unwrap_or(scope))
            .filter(|scope| !self.namespaces.contains(scope));
        let owner = of_class
            .map(str::to_string)
            .or_else(|| self.owners.last().cloned());
        let kind = match owner.as_deref() {
            Some(owner) if owner == named.name.as_str() => SymbolKind::Constructor,
            Some(_) => SymbolKind::Method,
            None => SymbolKind::Function,
        };
        let public = !self.is_static(node) || owner.is_some();
        self.add(&named.name, kind, node, &written, owner.as_deref(), public);
    }

    /// A variable at file level, and what a declaration defines as it goes. A function declared and
    /// not defined is no symbol.
    fn declaration(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(kind) = node.child_by_field_name("type") {
            self.item(&kind, false, depth + 1);
        }
        let mut cursor = node.walk();
        let declarators: Vec<Node<'a>> = node
            .children_by_field_name("declarator", &mut cursor)
            .collect();
        let external = {
            let mut cursor = node.walk();
            node.children(&mut cursor).any(|child| {
                child.kind() == "storage_class_specifier"
                    && text(&child, self.source) == Some("extern")
            })
        };
        let constant = {
            let mut cursor = node.walk();
            node.children(&mut cursor).any(|child| {
                child.kind() == "type_qualifier"
                    && matches!(text(&child, self.source), Some("const" | "constexpr"))
            })
        };
        for declarator in declarators {
            // `int (*fp)(void) = f;` is a variable, and `int f(void);` no symbol.
            let at = named_by(declarator, 0);
            if at.kind() != "identifier" || external || function_declarator(declarator, 0).is_some()
            {
                continue;
            }
            if let Some(name) = text(&at, self.source) {
                let kind = if constant {
                    SymbolKind::Constant
                } else {
                    SymbolKind::Variable
                };
                let public = !self.is_static(node);
                self.add(name, kind, node, &[], None, public);
            }
        }
    }

    /// A field or a method in a class. A method only declared here is defined elsewhere, and so is
    /// no symbol here, unless it is pure virtual: then this is all there is.
    fn field(&mut self, node: &Node<'a>, depth: usize) {
        let Some(owner) = self.owners.last().cloned() else {
            return;
        };
        if let Some(kind) = node.child_by_field_name("type") {
            self.item(&kind, false, depth + 1);
        }
        let mut cursor = node.walk();
        let declarators: Vec<Node<'a>> = node
            .children_by_field_name("declarator", &mut cursor)
            .collect();
        for declarator in declarators {
            if let Some(named) = self.function_name(declarator) {
                let pure = node
                    .child_by_field_name("default_value")
                    .is_some_and(|value| text(&value, self.source) == Some("0"));
                if pure {
                    self.add(
                        &named.name,
                        SymbolKind::Method,
                        node,
                        &[],
                        Some(&owner),
                        true,
                    );
                }
                continue;
            }
            // `int (*run)(int)` is a field.
            let at = named_by(declarator, 0);
            if at.kind() == "field_identifier"
                && let Some(name) = text(&at, self.source)
            {
                self.add(name, SymbolKind::Field, node, &[], Some(&owner), true);
            }
        }
    }

    /// A struct, class, union or enum with a body, by the name given or `alias`, the name a `typedef`
    /// gives one that has none.
    fn record(&mut self, node: &Node<'a>, alias: Option<&'a str>, templated: bool, depth: usize) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let named = node.child_by_field_name("name");
        let written = named.as_ref().and_then(|name| text(name, self.source));
        let Some(written) = written.or(alias) else {
            // No name: what is inside belongs to what is around it, and has no name to go by.
            return self.children(&body, templated, depth);
        };
        // `Box<char>` is another symbol than `Box`, and goes by `Box` as its name.
        let name = written.split('<').next().unwrap_or(written).trim();
        let kind = match node.kind() {
            "class_specifier" => SymbolKind::Class,
            "union_specifier" => SymbolKind::Union,
            "enum_specifier" => SymbolKind::Enum,
            _ => SymbolKind::Struct,
        };
        self.add(
            name,
            kind,
            node,
            &[],
            self.owners.last().cloned().as_deref(),
            true,
        );
        if kind == SymbolKind::Enum {
            return self.enumerators(&body, written);
        }
        self.scope.push(written.to_string());
        self.owners.push(name.to_string());
        self.children(&body, templated, depth);
        self.owners.pop();
        self.scope.pop();
    }

    fn enumerators(&mut self, body: &Node<'a>, owner: &str) {
        let mut cursor = body.walk();
        for value in body.named_children(&mut cursor) {
            if value.kind() == "enumerator"
                && let Some(name) = self.name_field(&value)
            {
                self.add(
                    name,
                    SymbolKind::EnumVariant,
                    &value,
                    &[owner.to_string()],
                    Some(owner),
                    true,
                );
            }
        }
    }

    fn typedef(&mut self, node: &Node<'a>, depth: usize) {
        let mut cursor = node.walk();
        let declarators: Vec<Node<'a>> = node
            .children_by_field_name("declarator", &mut cursor)
            .collect();
        let name_of = |declarator: &Node<'a>| {
            let at = declared(*declarator, 0);
            (at.kind() == "type_identifier")
                .then(|| text(&at, self.source))
                .flatten()
        };
        let first = declarators.first().and_then(name_of);
        let kind = node.child_by_field_name("type");
        // `typedef struct { ... } T;`: the struct is `T`.
        let unnamed = kind.as_ref().is_some_and(|kind| {
            kind.child_by_field_name("name").is_none() && kind.child_by_field_name("body").is_some()
        });
        if let Some(kind) = kind {
            self.record(&kind, first.filter(|_| unnamed), false, depth + 1);
        }
        for (at, declarator) in declarators.iter().enumerate() {
            if unnamed && at == 0 {
                continue;
            }
            if let Some(name) = name_of(declarator) {
                self.add(name, SymbolKind::TypeAlias, node, &[], None, true);
            }
        }
    }

    fn namespace(&mut self, node: &Node<'a>, depth: usize) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let Some(name) = self.name_field(node) else {
            // An anonymous one adds nothing to a name.
            return self.children(&body, false, depth);
        };
        self.add(name, SymbolKind::Namespace, node, &[], None, true);
        let pushed = name.split("::").count();
        self.scope.extend(name.split("::").map(str::to_string));
        self.children(&body, false, depth);
        self.scope.truncate(self.scope.len() - pushed);
    }
}

#[cfg(test)]
mod tests;
