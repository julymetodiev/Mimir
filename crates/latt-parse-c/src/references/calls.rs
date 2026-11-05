//! What a name means by how it is written: what is called, what is built, and a member.

use latt_parse_core::extracted::CallForm;
use latt_parse_core::nodes::{MAX_DEPTH, text};
use latt_types::ReferenceKind;
use tree_sitter::Node;

use crate::declarators::last_name;
use crate::references::Walk;

impl<'a> Walk<'a> {
    /// `a::b` not called: a type by its name, and otherwise only the answer says.
    pub fn qualified(&mut self, node: &Node<'a>, depth: usize) {
        let Some(name) = last_name(*node, depth) else {
            return self.children(node, depth);
        };
        let scope = node
            .child_by_field_name("scope")
            .and_then(|scope| text(&scope, self.source));
        match name.kind() {
            "type_identifier" => self.add(ReferenceKind::TypeUse, &name, None, scope),
            "template_type" => self.visit(&name, depth + 1),
            _ => self.add(
                ReferenceKind::FunctionValue,
                &name,
                Some(CallForm::Scoped),
                scope,
            ),
        }
    }

    /// What is called: `f`, `a.f`, `a->f`, `a::f`, `f<int>`, or anything else, of which no name can be
    /// said.
    pub fn callee(&mut self, callee: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        match callee.kind() {
            "identifier" => {
                let bound = text(callee, self.source).is_some_and(|name| self.is_bound(name));
                let form = if bound {
                    CallForm::Bound
                } else {
                    CallForm::Plain
                };
                self.add(ReferenceKind::Call, callee, Some(form), None);
            }
            "field_expression" => {
                let argument = callee.child_by_field_name("argument");
                let receiver = argument
                    .as_ref()
                    .filter(|a| a.kind() == "identifier")
                    .and_then(|a| text(a, self.source));
                if let Some(field) = callee.child_by_field_name("field") {
                    let name = if field.kind() == "template_method" {
                        field.child_by_field_name("name").unwrap_or(field)
                    } else {
                        field
                    };
                    self.add(ReferenceKind::Call, &name, Some(CallForm::Method), receiver);
                }
                if let Some(argument) = argument {
                    self.visit(&argument, depth + 1);
                }
            }
            "qualified_identifier" => {
                let scope = callee
                    .child_by_field_name("scope")
                    .and_then(|scope| text(&scope, self.source));
                let name = last_name(*callee, depth);
                match name {
                    Some(name) if name.kind() == "template_function" => {
                        if let Some(inner) = name.child_by_field_name("name") {
                            self.add(ReferenceKind::Call, &inner, Some(CallForm::Scoped), scope);
                        }
                        self.field(&name, "arguments", depth);
                    }
                    Some(name) => {
                        self.add(ReferenceKind::Call, &name, Some(CallForm::Scoped), scope);
                    }
                    None => {}
                }
            }
            "template_function" => {
                if let Some(name) = callee.child_by_field_name("name") {
                    self.callee(&name, depth + 1);
                }
                self.field(callee, "arguments", depth);
            }
            "parenthesized_expression" => {
                if let Some(inner) = callee.named_child(0) {
                    self.callee(&inner, depth + 1);
                }
            }
            _ => self.visit(callee, depth + 1),
        }
    }

    pub fn new_expression(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(kind) = node.child_by_field_name("type") {
            let name = if kind.kind() == "qualified_identifier" {
                last_name(kind, depth)
            } else if kind.kind() == "template_type" {
                kind.child_by_field_name("name")
            } else {
                Some(kind)
            };
            if let Some(name) = name.filter(|n| n.kind() == "type_identifier") {
                self.add(ReferenceKind::Call, &name, None, None);
            }
            self.visit(&kind, depth + 1);
        }
        self.field(node, "placement", depth);
        self.field(node, "arguments", depth);
    }

    /// `x.f` or `x->f` not called. What it is, only the answer says.
    pub fn member(&mut self, node: &Node<'a>, depth: usize, write: bool) {
        let argument = node.child_by_field_name("argument");
        if let Some(field) = node.child_by_field_name("field") {
            let kind = if write {
                ReferenceKind::FieldWrite
            } else {
                ReferenceKind::FieldUse
            };
            let receiver = argument
                .as_ref()
                .filter(|a| a.kind() == "identifier")
                .and_then(|a| text(a, self.source));
            self.add(kind, &field, None, receiver);
        }
        if let Some(argument) = argument {
            self.visit(&argument, depth + 1);
        }
    }
}
