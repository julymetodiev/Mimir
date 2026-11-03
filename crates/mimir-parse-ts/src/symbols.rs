//! Symbols of TypeScript and JavaScript: declarations, functions bound to a name, and functions
//! assigned to a property, as in `app.use = function () {}`.

use std::collections::HashSet;

use mimir_parse_core::ParsedTree;
use mimir_parse_core::extracted::ExtractedSymbol;
use mimir_types::{Language, QualifiedName, SymbolKind, Visibility};
use tree_sitter::Node;

use crate::{MAX_DEPTH, is_function, range_of, span_of, text};

/// Extract the symbols of a parse tree.
pub fn extract_symbols(tree: &ParsedTree, language: Language) -> Vec<ExtractedSymbol> {
    let mut walk = Walk {
        source: tree.source_bytes(),
        language,
        found: Vec::new(),
        exported_later: HashSet::new(),
    };
    walk.children(&tree.root_node(), &Scope::module(), 0);
    walk.finish()
}

/// Where a walk is.
#[derive(Clone)]
struct Scope<'a> {
    parent: Option<&'a str>,
    /// At the top of a module or namespace, where a constant is a symbol.
    top: bool,
    exported: bool,
    /// The `export` around a declaration, which is part of it.
    outer: Option<Node<'a>>,
}

impl<'a> Scope<'a> {
    fn module() -> Self {
        Self {
            parent: None,
            top: true,
            exported: false,
            outer: None,
        }
    }

    fn inside(&self, parent: Option<&'a str>) -> Self {
        Self {
            parent,
            top: false,
            exported: false,
            outer: None,
        }
    }
}

struct Found {
    symbol: ExtractedSymbol,
    /// An overload or a declaration without a body.
    signature: bool,
}

struct Walk<'a> {
    source: &'a [u8],
    language: Language,
    found: Vec<Found>,
    /// Names in `export { a }`.
    exported_later: HashSet<String>,
}

impl<'a> Walk<'a> {
    fn children(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.item(&child, scope, depth + 1);
        }
    }

    fn add(
        &mut self,
        name: &str,
        kind: SymbolKind,
        node: &Node<'a>,
        scope: &Scope<'a>,
        signature: bool,
    ) {
        let whole = scope.outer.as_ref().unwrap_or(node);
        let mut symbol = ExtractedSymbol::new(
            name.to_string(),
            kind,
            self.language,
            range_of(whole),
            span_of(whole),
        )
        .with_visibility(self.visibility(node, name, scope));
        if let Some(parent) = scope.parent {
            symbol =
                symbol
                    .with_parent(parent.to_string())
                    .with_qualified_name(QualifiedName::new(vec![
                        parent.to_string(),
                        name.to_string(),
                    ]));
        }
        self.found.push(Found { symbol, signature });
    }

    fn visibility(&self, node: &Node<'a>, name: &str, scope: &Scope<'a>) -> Visibility {
        if name.starts_with('#') {
            return Visibility::Private;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "accessibility_modifier" {
                return match text(&child, self.source) {
                    Some("private") => Visibility::Private,
                    Some("protected") => Visibility::Protected,
                    _ => Visibility::Public,
                };
            }
        }
        if scope.exported || scope.parent.is_some() {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    fn name_of(&self, node: &Node<'a>, field: &str) -> Option<&'a str> {
        text(&node.child_by_field_name(field)?, self.source)
    }

    fn item(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        match node.kind() {
            "export_statement" => self.export(node, scope, depth),
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                if let Some(name) = self.name_of(node, "name") {
                    let signature = node.kind() == "function_signature";
                    self.add(name, SymbolKind::Function, node, scope, signature);
                    self.children(node, &scope.inside(Some(name)).without_parent(), depth);
                }
            }
            "class_declaration" | "abstract_class_declaration" | "class" => {
                if let Some(name) = self.name_of(node, "name") {
                    self.add(name, SymbolKind::Class, node, scope, false);
                    self.children(node, &scope.inside(Some(name)), depth);
                } else {
                    self.children(node, &scope.inside(scope.parent), depth);
                }
            }
            "interface_declaration" => {
                if let Some(name) = self.name_of(node, "name") {
                    self.add(name, SymbolKind::Interface, node, scope, false);
                    self.children(node, &scope.inside(Some(name)), depth);
                }
            }
            "type_alias_declaration" => {
                if let Some(name) = self.name_of(node, "name") {
                    self.add(name, SymbolKind::TypeAlias, node, scope, false);
                }
            }
            "enum_declaration" => {
                if let Some(name) = self.name_of(node, "name") {
                    self.add(name, SymbolKind::Enum, node, scope, false);
                    self.children(node, &scope.inside(Some(name)), depth);
                }
            }
            "enum_assignment" => {
                if let Some(name) = self.name_of(node, "name") {
                    self.add(name, SymbolKind::EnumVariant, node, scope, false);
                }
            }
            "property_identifier" if scope.parent.is_some() => {
                // Only an enum's body has a bare name as a member.
                if node.parent().is_some_and(|p| p.kind() == "enum_body")
                    && let Some(name) = text(node, self.source)
                {
                    self.add(name, SymbolKind::EnumVariant, node, scope, false);
                }
            }
            "internal_module" | "module" => {
                if let Some(name) = self.name_of(node, "name") {
                    self.add(name, SymbolKind::Namespace, node, scope, false);
                    let mut inner = scope.inside(Some(name));
                    inner.top = true;
                    self.children(node, &inner, depth);
                }
            }
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                if let Some(name) = self.name_of(node, "name") {
                    let kind = if name == "constructor" {
                        SymbolKind::Constructor
                    } else {
                        SymbolKind::Method
                    };
                    let signature = node.kind() != "method_definition";
                    self.add(name, kind, node, scope, signature);
                    self.children(node, &scope.inside(Some(name)).without_parent(), depth);
                }
            }
            "public_field_definition" | "field_definition" | "property_signature" => {
                self.field(node, scope, depth)
            }
            "lexical_declaration" | "variable_declaration" => self.variables(node, scope, depth),
            "assignment_expression" => self.assignment(node, scope, depth),
            "pair" => self.pair(node, scope, depth),
            "export_clause" => {
                let mut cursor = node.walk();
                for specifier in node.named_children(&mut cursor) {
                    if let Some(name) = self.name_of(&specifier, "name") {
                        self.exported_later.insert(name.to_string());
                    }
                }
            }
            _ => self.children(node, &scope.inside(scope.parent).keeping_top(scope), depth),
        }
    }

    fn export(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        let mut inner = scope.clone();
        inner.exported = true;
        inner.outer = Some(*node);
        if let Some(declaration) = node.child_by_field_name("declaration") {
            self.item(&declaration, &inner, depth + 1);
        } else if let Some(value) = node.child_by_field_name("value") {
            if is_function(&value) {
                self.add("default", SymbolKind::Function, &value, &inner, false);
                self.children(&value, &scope.inside(None), depth);
            } else {
                self.item(&value, &inner, depth + 1);
            }
        } else if node.child_by_field_name("source").is_none() {
            self.children(node, scope, depth);
        }
    }

    fn field(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        let name = self
            .name_of(node, "name")
            .or_else(|| self.name_of(node, "property"));
        let Some(name) = name else {
            return;
        };
        let value = node.child_by_field_name("value");
        let kind = if value.as_ref().is_some_and(is_function) {
            SymbolKind::Method
        } else {
            SymbolKind::Field
        };
        self.add(name, kind, node, scope, false);
        if let Some(value) = value {
            self.children(&value, &scope.inside(Some(name)).without_parent(), depth);
        }
    }

    fn variables(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        let constant = text(node, self.source).is_some_and(|t| t.starts_with("const"));
        let mut cursor = node.walk();
        let declarators: Vec<_> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "variable_declarator")
            .collect();
        for declarator in &declarators {
            let mut here = scope.clone();
            // With one name the declaration is its whole text, keyword included.
            let whole = if declarators.len() == 1 {
                node
            } else {
                declarator
            };
            let name = declarator
                .child_by_field_name("name")
                .filter(|n| n.kind() == "identifier")
                .and_then(|n| text(&n, self.source));
            let value = declarator.child_by_field_name("value");
            let Some(name) = name else {
                if let Some(value) = value {
                    self.children(&value, &scope.inside(scope.parent), depth);
                }
                continue;
            };
            here.outer = scope.outer.or(Some(*whole));
            match &value {
                Some(v) if is_function(v) => {
                    self.add(name, SymbolKind::Function, whole, &here, false);
                    self.children(v, &scope.inside(Some(name)).without_parent(), depth);
                }
                Some(v) if v.kind() == "class" => {
                    self.add(name, SymbolKind::Class, whole, &here, false);
                    self.children(v, &scope.inside(Some(name)), depth);
                }
                _ => {
                    if scope.top {
                        let kind = if constant {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        };
                        self.add(name, kind, whole, &here, false);
                    }
                    if let Some(v) = value {
                        let parent = if scope.top { Some(name) } else { scope.parent };
                        self.children(&v, &scope.inside(parent), depth);
                    }
                }
            }
        }
    }

    /// `Foo.prototype.run = function () {}`, `exports.run = () => {}`, and what they hold.
    fn assignment(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        if is_function(&right) && left.kind() == "member_expression" {
            let object = left
                .child_by_field_name("object")
                .and_then(|o| text(&o, self.source));
            let property = left
                .child_by_field_name("property")
                .and_then(|p| text(&p, self.source));
            if let (Some(object), Some(property)) = (object, property) {
                let (name, parent, kind) = match (object, property) {
                    ("module", "exports") => {
                        (self.name_of(&right, "name"), None, SymbolKind::Function)
                    }
                    ("exports" | "module.exports", _) => {
                        (Some(property), None, SymbolKind::Function)
                    }
                    ("this", _) => (Some(property), scope.parent, SymbolKind::Method),
                    _ => (
                        Some(property),
                        Some(object.strip_suffix(".prototype").unwrap_or(object)),
                        SymbolKind::Method,
                    ),
                };
                if let Some(name) = name {
                    let mut here = scope.inside(parent);
                    here.exported = true;
                    self.add(name, kind, node, &here, false);
                    self.children(&right, &scope.inside(Some(name)).without_parent(), depth);
                    return;
                }
            }
        }
        self.children(&right, &scope.inside(scope.parent), depth);
    }

    /// `run: () => {}` in an object a constant holds.
    fn pair(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        let Some(value) = node.child_by_field_name("value") else {
            return;
        };
        let key = node
            .child_by_field_name("key")
            .filter(|k| k.kind() == "property_identifier");
        match key.and_then(|k| text(&k, self.source)) {
            Some(name) if is_function(&value) && scope.parent.is_some() => {
                self.add(name, SymbolKind::Method, node, scope, false);
                self.children(&value, &scope.inside(Some(name)).without_parent(), depth);
            }
            _ => self.item(&value, scope, depth + 1),
        }
    }

    /// One symbol for an overloaded name, at what has a body, or else the first signature.
    fn finish(self) -> Vec<ExtractedSymbol> {
        let key = |s: &ExtractedSymbol| (s.parent.clone(), s.name.clone());
        let with_body: HashSet<_> = self
            .found
            .iter()
            .filter(|f| !f.signature)
            .map(|f| key(&f.symbol))
            .collect();
        let mut seen = HashSet::new();
        let exported_later = self.exported_later;
        self.found
            .into_iter()
            .filter(|f| {
                !f.signature
                    || (!with_body.contains(&key(&f.symbol)) && seen.insert(key(&f.symbol)))
            })
            .map(|f| {
                let mut symbol = f.symbol;
                if symbol.parent.is_none() && exported_later.contains(&symbol.name) {
                    symbol.visibility = Visibility::Public;
                }
                symbol
            })
            .collect()
    }
}

impl<'a> Scope<'a> {
    /// Inside a function nothing has a parent of its own: `run` in `run` is not a member.
    fn without_parent(mut self) -> Self {
        self.parent = None;
        self
    }

    /// A block at the top of a namespace, or `declare`, stays at the top.
    fn keeping_top(mut self, outer: &Scope<'a>) -> Self {
        self.top = outer.top;
        self.exported = outer.exported;
        self
    }
}

#[cfg(test)]
mod tests;
