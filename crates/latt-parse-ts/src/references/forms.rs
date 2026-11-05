//! The forms that bring names in or name them in their own way: imports, `require`, JSX, heritage.

use latt_parse_core::extracted::CallForm;
use latt_parse_core::nodes::{MAX_DEPTH, text};
use latt_types::ReferenceKind;
use tree_sitter::Node;

use crate::references::Walk;
use crate::references::names::{module_path, pattern_names, required_path};

impl<'a> Walk<'a> {
    pub fn import(&mut self, node: &Node<'a>) {
        let Some(source) = module_path(node, self.source) else {
            return;
        };
        let mut cursor = node.walk();
        for clause in node.named_children(&mut cursor) {
            if clause.kind() == "import_clause" {
                self.import_names(&clause, source, 0);
            }
        }
    }

    pub fn import_names(&mut self, node: &Node<'a>, source: &str, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                // A default or `* as` import binds a name of its own.
                "identifier" => self.add(
                    ReferenceKind::Import,
                    &child,
                    Some(CallForm::Bound),
                    Some(source),
                ),
                "import_specifier" | "export_specifier" => {
                    if let Some(name) = child.child_by_field_name("name") {
                        let form = child.child_by_field_name("alias").map(|_| CallForm::Bound);
                        self.add(ReferenceKind::Import, &name, form, Some(source));
                    }
                }
                _ => self.import_names(&child, source, depth + 1),
            }
        }
    }

    pub fn export(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(source) = module_path(node, self.source) {
            self.import_names(node, source, 0);
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "export_clause" {
                self.visit(&child, depth + 1);
            }
        }
    }

    pub fn declarator(&mut self, node: &Node<'a>, depth: usize) {
        let value = node.child_by_field_name("value");
        let name = node.child_by_field_name("name");
        if let (Some(value), Some(name)) = (&value, &name)
            && let Some(path) = required_path(value, self.source)
        {
            match name.kind() {
                "identifier" => self.add(
                    ReferenceKind::Import,
                    name,
                    Some(CallForm::Bound),
                    Some(path),
                ),
                _ => {
                    let mut names = Vec::new();
                    pattern_names(name, &mut names, 0);
                    for bound in names {
                        self.add(ReferenceKind::Import, &bound, None, Some(path));
                    }
                }
            }
            return;
        }
        if let Some(name) = &name
            && name.kind() != "identifier"
        {
            self.pattern(name, depth + 1);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if Some(child) != name {
                self.visit(&child, depth + 1);
            }
        }
    }

    pub fn jsx(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(name) = node.child_by_field_name("name") {
            match name.kind() {
                "identifier" => {
                    // A name in lower case is an element of the page, not a component.
                    if text(&name, self.source).is_some_and(|n| n.starts_with(char::is_uppercase)) {
                        let form = self.plain_form(&name);
                        self.add(ReferenceKind::FunctionValue, &name, Some(form), None);
                    }
                }
                "member_expression" | "nested_identifier" => {
                    let property = name.child_by_field_name("property").or_else(|| {
                        name.named_child((name.named_child_count().saturating_sub(1)) as u32)
                    });
                    if let Some(property) = property {
                        self.add(
                            ReferenceKind::FunctionValue,
                            &property,
                            Some(CallForm::Method),
                            None,
                        );
                    }
                }
                _ => {}
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "jsx_attribute" || child.kind() == "jsx_expression" {
                self.children(&child, depth + 1);
            }
        }
    }

    /// `extends A` and `implements B`: the context says which, for the edge `Implements`.
    pub fn heritage(&mut self, node: &Node<'a>, depth: usize) {
        let context = if node.kind() == "implements_clause" {
            "implements"
        } else {
            "extends"
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "identifier" | "type_identifier" => {
                    self.add(ReferenceKind::TypeUse, &child, None, Some(context))
                }
                "extends_clause" | "implements_clause" => self.heritage(&child, depth + 1),
                _ => self.visit(&child, depth + 1),
            }
        }
    }
}
