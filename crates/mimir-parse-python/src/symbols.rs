//! Symbols of Python: functions, classes and what they hold, and what a module or a method assigns.

use std::collections::HashSet;

use mimir_parse_core::ParsedTree;
use mimir_parse_core::extracted::ExtractedSymbol;
use mimir_parse_core::nodes::{MAX_DEPTH, module_symbol, range_of, span_of, text};
use mimir_types::{Language, QualifiedName, SymbolKind, Visibility};
use tree_sitter::Node;

/// Extract the symbols of a parse tree.
pub fn extract_symbols(tree: &ParsedTree) -> Vec<ExtractedSymbol> {
    let mut walk = Walk {
        source: tree.source_bytes(),
        found: Vec::new(),
        fields: HashSet::new(),
    };
    walk.children(&tree.root_node(), &Scope::module(), 0);
    let mut symbols = walk.finish();
    symbols.push(module_symbol(tree, Language::Python));
    symbols
}

/// Where a walk is.
#[derive(Clone, Copy)]
struct Scope<'a> {
    /// Directly in the body of this class.
    class: Option<&'a str>,
    /// In the body of a method of this class, where `self.x = ...` declares a field.
    method_of: Option<&'a str>,
    /// At the top of a module, where a variable is a symbol.
    top: bool,
    /// In the body of a class that inherits from an enum.
    enumeration: bool,
}

impl Scope<'_> {
    fn module() -> Self {
        Self {
            class: None,
            method_of: None,
            top: true,
            enumeration: false,
        }
    }
}

struct Found {
    symbol: ExtractedSymbol,
    /// Under `@overload`.
    signature: bool,
}

struct Walk<'a> {
    source: &'a [u8],
    found: Vec<Found>,
    /// The fields declared so far: only the first assignment declares one.
    fields: HashSet<(&'a str, &'a str)>,
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
        name: &'a str,
        kind: SymbolKind,
        whole: &Node<'a>,
        parent: Option<&'a str>,
        signature: bool,
    ) {
        let private = name.starts_with('_') && !name.ends_with("__");
        let visibility = if private {
            Visibility::Private
        } else {
            Visibility::Public
        };
        let mut symbol = ExtractedSymbol::new(
            name.to_string(),
            kind,
            Language::Python,
            range_of(whole),
            span_of(whole),
        )
        .with_visibility(visibility);
        if let Some(parent) = parent {
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

    fn name_of(&self, node: &Node<'a>) -> Option<&'a str> {
        text(&node.child_by_field_name("name")?, self.source)
    }

    fn item(&mut self, node: &Node<'a>, scope: &Scope<'a>, depth: usize) {
        match node.kind() {
            "decorated_definition" => {
                if let Some(definition) = node.child_by_field_name("definition") {
                    self.definition(&definition, node, scope, self.is_overload(node), depth);
                }
            }
            "function_definition" | "class_definition" => {
                self.definition(node, node, scope, false, depth)
            }
            "assignment" => self.assignment(node, scope),
            "type_alias_statement" => {
                let name = node
                    .child_by_field_name("left")
                    .and_then(|left| first_identifier(&left, 0))
                    .and_then(|name| text(&name, self.source));
                if let (Some(name), true) = (name, scope.top || scope.class.is_some()) {
                    self.add(name, SymbolKind::TypeAlias, node, scope.class, false);
                }
            }
            "comment" | "string" | "import_statement" | "import_from_statement" => {}
            _ => self.children(node, scope, depth),
        }
    }

    fn is_overload(&self, decorated: &Node<'a>) -> bool {
        let mut cursor = decorated.walk();
        decorated
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "decorator")
            .filter_map(|decorator| text(&decorator, self.source))
            .any(|decorator| matches!(decorator, "@overload" | "@typing.overload"))
    }

    /// A function or a class, with `whole` holding its decorators too.
    fn definition(
        &mut self,
        node: &Node<'a>,
        whole: &Node<'a>,
        scope: &Scope<'a>,
        signature: bool,
        depth: usize,
    ) {
        let Some(name) = self.name_of(node) else {
            return;
        };
        let body = node.child_by_field_name("body");
        if node.kind() == "class_definition" {
            self.add(name, SymbolKind::Class, whole, scope.class, false);
            if let Some(body) = body {
                let inside = Scope {
                    class: Some(name),
                    method_of: None,
                    top: false,
                    enumeration: self.is_enumeration(node),
                };
                self.children(&body, &inside, depth);
            }
            return;
        }
        let kind = match (scope.class, name) {
            (Some(_), "__init__") => SymbolKind::Constructor,
            (Some(_), _) => SymbolKind::Method,
            (None, _) => SymbolKind::Function,
        };
        self.add(name, kind, whole, scope.class, signature);
        if let Some(body) = body {
            let inside = Scope {
                class: None,
                method_of: scope.class,
                top: false,
                enumeration: false,
            };
            self.children(&body, &inside, depth);
        }
    }

    /// Whether a class names an enum among what it inherits, as `Enum` or `enum.IntFlag`.
    fn is_enumeration(&self, class: &Node<'a>) -> bool {
        let Some(bases) = class.child_by_field_name("superclasses") else {
            return false;
        };
        let mut cursor = bases.walk();
        bases
            .named_children(&mut cursor)
            .filter(|base| matches!(base.kind(), "identifier" | "attribute"))
            .filter_map(|base| text(&base, self.source))
            .filter_map(|base| base.rsplit('.').next())
            .any(|base| base.ends_with("Enum") || base.ends_with("Flag"))
    }

    fn assignment(&mut self, node: &Node<'a>, scope: &Scope<'a>) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        let right = node.child_by_field_name("right");
        let is_lambda = right.as_ref().is_some_and(|r| r.kind() == "lambda");
        match left.kind() {
            "identifier" => {
                let Some(name) = text(&left, self.source) else {
                    return;
                };
                if let Some(class) = scope.class {
                    if is_lambda {
                        self.add(name, SymbolKind::Method, node, Some(class), false);
                    } else if scope.enumeration {
                        self.add(name, SymbolKind::EnumVariant, node, Some(class), false);
                    } else if self.fields.insert((class, name)) {
                        self.add(name, SymbolKind::Field, node, Some(class), false);
                    }
                } else if scope.top {
                    let kind = if is_lambda {
                        SymbolKind::Function
                    } else if right.is_some() && is_constant_name(name) {
                        SymbolKind::Constant
                    } else {
                        SymbolKind::Variable
                    };
                    self.add(name, kind, node, None, false);
                }
            }
            "attribute" => {
                let object = left
                    .child_by_field_name("object")
                    .and_then(|o| text(&o, self.source));
                let name = left
                    .child_by_field_name("attribute")
                    .and_then(|a| text(&a, self.source));
                if let (Some(class), Some("self"), Some(name)) = (scope.method_of, object, name)
                    && self.fields.insert((class, name))
                {
                    self.add(name, SymbolKind::Field, node, Some(class), false);
                }
            }
            "pattern_list" | "tuple_pattern" | "list_pattern" if scope.top => {
                let mut cursor = left.walk();
                for part in left.named_children(&mut cursor) {
                    if part.kind() == "identifier"
                        && let Some(name) = text(&part, self.source)
                    {
                        self.add(name, SymbolKind::Variable, node, None, false);
                    }
                }
            }
            _ => {}
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
        self.found
            .into_iter()
            .filter(|f| {
                !f.signature
                    || (!with_body.contains(&key(&f.symbol)) && seen.insert(key(&f.symbol)))
            })
            .map(|f| f.symbol)
            .collect()
    }
}

/// `X` of `X[T]`.
fn first_identifier<'a>(node: &Node<'a>, depth: usize) -> Option<Node<'a>> {
    if node.kind() == "identifier" {
        return Some(*node);
    }
    if depth >= MAX_DEPTH {
        return None;
    }
    first_identifier(&node.named_child(0)?, depth + 1)
}

/// `MAX_SIZE`, by the convention that makes a constant in Python.
fn is_constant_name(name: &str) -> bool {
    name.chars().any(char::is_alphabetic) && !name.chars().any(char::is_lowercase)
}

#[cfg(test)]
mod tests;
