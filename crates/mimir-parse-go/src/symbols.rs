//! Symbols of Go: what a package declares at its top level, the methods of its types and interfaces,
//! and the fields of its structs.

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
    };
    let root = tree.root_node();
    let mut cursor = root.walk();
    for item in root.named_children(&mut cursor) {
        walk.item(&item);
    }
    walk.found.push(module_symbol(tree, Language::Go));
    walk.found
}

struct Walk<'a> {
    source: &'a [u8],
    found: Vec<ExtractedSymbol>,
}

impl<'a> Walk<'a> {
    fn add(&mut self, name: &str, kind: SymbolKind, whole: &Node<'a>, parent: Option<&str>) {
        let exported = name.starts_with(char::is_uppercase);
        let mut symbol = ExtractedSymbol::new(
            name.to_string(),
            kind,
            Language::Go,
            range_of(whole),
            span_of(whole),
        )
        .with_visibility(if exported {
            Visibility::Public
        } else {
            Visibility::Private
        });
        if let Some(parent) = parent {
            symbol =
                symbol
                    .with_parent(parent.to_string())
                    .with_qualified_name(QualifiedName::new(vec![
                        parent.to_string(),
                        name.to_string(),
                    ]));
        }
        self.found.push(symbol);
    }

    fn name_of(&self, node: &Node<'a>) -> Option<&'a str> {
        text(&node.child_by_field_name("name")?, self.source)
    }

    fn item(&mut self, node: &Node<'a>) {
        match node.kind() {
            "function_declaration" => {
                if let Some(name) = self.name_of(node) {
                    self.add(name, SymbolKind::Function, node, None);
                }
            }
            "method_declaration" => self.method(node),
            "type_declaration" => {
                let mut cursor = node.walk();
                for spec in node.named_children(&mut cursor) {
                    self.type_spec(&spec);
                }
            }
            "const_declaration" | "var_declaration" => self.values(node, 0),
            _ => {}
        }
    }

    fn method(&mut self, node: &Node<'a>) {
        let receiver = node
            .child_by_field_name("receiver")
            .and_then(|list| list.named_child(0))
            .and_then(|parameter| parameter.child_by_field_name("type"))
            .and_then(|kind| self.type_name(&kind, 0));
        if let (Some(name), Some(receiver)) = (self.name_of(node), receiver) {
            self.add(name, SymbolKind::Method, node, Some(receiver));
        }
    }

    /// The name a type is written with, without `*`, a package or type arguments.
    fn type_name(&self, node: &Node<'a>, depth: usize) -> Option<&'a str> {
        if depth >= MAX_DEPTH {
            return None;
        }
        match node.kind() {
            "type_identifier" => text(node, self.source),
            "qualified_type" => text(&node.child_by_field_name("name")?, self.source),
            "generic_type" => self.type_name(&node.child_by_field_name("type")?, depth + 1),
            "pointer_type" | "parenthesized_type" => {
                self.type_name(&node.named_child(0)?, depth + 1)
            }
            _ => None,
        }
    }

    fn type_spec(&mut self, spec: &Node<'a>) {
        let Some(name) = self.name_of(spec) else {
            return;
        };
        let body = spec.child_by_field_name("type");
        let kind = match (spec.kind(), body.as_ref().map(Node::kind)) {
            ("type_spec", Some("struct_type")) => SymbolKind::Struct,
            ("type_spec", Some("interface_type")) => SymbolKind::Interface,
            ("type_spec" | "type_alias", _) => SymbolKind::TypeAlias,
            _ => return,
        };
        self.add(name, kind, spec, None);
        match (kind, body) {
            (SymbolKind::Struct, Some(body)) => self.fields(&body, name),
            (SymbolKind::Interface, Some(body)) => self.interface_methods(&body, name),
            _ => {}
        }
    }

    fn fields(&mut self, structure: &Node<'a>, owner: &str) {
        let mut cursor = structure.walk();
        for list in structure.named_children(&mut cursor) {
            let mut cursor = list.walk();
            for field in list.named_children(&mut cursor) {
                if field.kind() != "field_declaration" {
                    continue;
                }
                let mut names = field.walk();
                let named: Vec<&str> = field
                    .children_by_field_name("name", &mut names)
                    .filter_map(|name| text(&name, self.source))
                    .collect();
                if named.is_empty() {
                    // An embedded type is a field of its name.
                    let embedded = field
                        .child_by_field_name("type")
                        .and_then(|kind| self.type_name(&kind, 0));
                    if let Some(name) = embedded {
                        self.add(name, SymbolKind::Field, &field, Some(owner));
                    }
                }
                for name in named {
                    self.add(name, SymbolKind::Field, &field, Some(owner));
                }
            }
        }
    }

    fn interface_methods(&mut self, interface: &Node<'a>, owner: &str) {
        let mut cursor = interface.walk();
        for method in interface.named_children(&mut cursor) {
            if method.kind() == "method_elem"
                && let Some(name) = self.name_of(&method)
            {
                self.add(name, SymbolKind::Method, &method, Some(owner));
            }
        }
    }

    /// `const` and `var`, one at a time or in a list of them. A variable bound to a function is one.
    fn values(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for spec in node.named_children(&mut cursor) {
            match spec.kind() {
                "const_spec" => self.value_spec(&spec, SymbolKind::Constant),
                "var_spec" => self.value_spec(&spec, SymbolKind::Variable),
                "var_spec_list" => self.values(&spec, depth + 1),
                _ => {}
            }
        }
    }

    fn value_spec(&mut self, spec: &Node<'a>, kind: SymbolKind) {
        let values: Vec<Node<'a>> = spec
            .child_by_field_name("value")
            .map(|list| {
                let mut cursor = list.walk();
                list.named_children(&mut cursor).collect()
            })
            .unwrap_or_default();
        let mut cursor = spec.walk();
        let names: Vec<Node<'a>> = spec.children_by_field_name("name", &mut cursor).collect();
        for (at, name) in names.iter().enumerate() {
            let Some(name) = text(name, self.source) else {
                continue;
            };
            if name == "_" {
                continue;
            }
            let holds_function = kind == SymbolKind::Variable
                && values.get(at).is_some_and(|v| v.kind() == "func_literal");
            let kind = if holds_function {
                SymbolKind::Function
            } else {
                kind
            };
            self.add(name, kind, spec, None);
        }
    }
}

#[cfg(test)]
mod tests;
