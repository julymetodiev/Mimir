//! References of TypeScript and JavaScript, at the name, where a language server is asked.

use std::collections::HashSet;

use mimir_parse_core::ParsedTree;
use mimir_parse_core::extracted::{CallForm, ExtractedReference};
use mimir_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

use crate::{MAX_DEPTH, range_of, span_of, text};

mod forms;
mod names;

use names::{collect_bound, is_declared_name};

/// Extract the references of a parse tree.
pub fn extract_references(tree: &ParsedTree) -> Vec<ExtractedReference> {
    let mut walk = Walk {
        source: tree.source_bytes(),
        found: Vec::new(),
        bound: Vec::new(),
    };
    walk.children(&tree.root_node(), None, 0);
    walk.found
}

struct Walk<'a> {
    source: &'a [u8],
    found: Vec<ExtractedReference>,
    /// The names bound by each function around here.
    bound: Vec<HashSet<&'a str>>,
}

impl<'a> Walk<'a> {
    fn children(&mut self, node: &Node<'a>, container: Option<&'a str>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(&child, container, depth + 1);
        }
    }

    fn add(
        &mut self,
        kind: ReferenceKind,
        at: &Node<'a>,
        container: Option<&'a str>,
        form: Option<CallForm>,
        context: Option<&str>,
    ) {
        let Some(name) = text(at, self.source) else {
            return;
        };
        let mut reference =
            ExtractedReference::new(name.to_string(), kind, range_of(at), span_of(at))
                .with_confidence(Confidence::Low);
        if let Some(container) = container {
            reference = reference.with_container(container.to_string());
        }
        if let Some(form) = form {
            reference = reference.with_call_form(form);
        }
        if let Some(context) = context {
            reference = reference.with_context(context.to_string());
        }
        self.found.push(reference);
    }

    fn is_bound(&self, name: &str) -> bool {
        self.bound.iter().any(|names| names.contains(name))
    }

    fn plain_form(&self, node: &Node<'a>) -> CallForm {
        if text(node, self.source).is_some_and(|name| self.is_bound(name)) {
            CallForm::Bound
        } else {
            CallForm::Plain
        }
    }

    /// Say on the last reference where its receiver's type can be asked for.
    fn mark_receiver(&mut self, object: Option<&Node<'a>>) {
        let name = object.and_then(|object| match object.kind() {
            "identifier" => Some(*object),
            "member_expression" => object.child_by_field_name("property"),
            _ => None,
        });
        let at = name.map(|n| {
            let start = n.start_position();
            (start.row as u32 + 1, start.column as u32)
        });
        if let Some(last) = self.found.last_mut() {
            last.receiver_at = at;
        }
    }

    /// What a receiver is, when it is short enough to say.
    fn receiver(&self, object: &Node<'a>) -> Option<&'a str> {
        matches!(object.kind(), "this" | "identifier" | "super")
            .then(|| text(object, self.source))
            .flatten()
    }

    /// A call of `callee`, which is `f`, `a.f` or anything else, of which no name can be said.
    fn callee(
        &mut self,
        kind: ReferenceKind,
        callee: &Node<'a>,
        container: Option<&'a str>,
        depth: usize,
    ) {
        match callee.kind() {
            "identifier" => {
                let form = self.plain_form(callee);
                self.add(kind, callee, container, Some(form), None);
            }
            "super" => self.add(kind, callee, container, Some(CallForm::Method), None),
            "member_expression" => {
                let object = callee.child_by_field_name("object");
                if let Some(property) = callee.child_by_field_name("property") {
                    let receiver = object.as_ref().and_then(|o| self.receiver(o));
                    self.add(kind, &property, container, Some(CallForm::Method), receiver);
                    self.mark_receiver(object.as_ref());
                }
                if let Some(object) = object {
                    self.visit(&object, container, depth + 1);
                }
            }
            _ => self.visit(callee, container, depth + 1),
        }
    }

    fn visit(&mut self, node: &Node<'a>, container: Option<&'a str>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        match node.kind() {
            "comment" => {}
            "call_expression" => {
                if let Some(function) = node.child_by_field_name("function") {
                    self.callee(ReferenceKind::Call, &function, container, depth);
                }
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    self.visit(&arguments, container, depth + 1);
                }
            }
            "new_expression" => {
                if let Some(constructor) = node.child_by_field_name("constructor") {
                    self.callee(ReferenceKind::Call, &constructor, container, depth);
                }
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    self.visit(&arguments, container, depth + 1);
                }
            }
            "decorator" => {
                let mut cursor = node.walk();
                let inner: Vec<_> = node.named_children(&mut cursor).collect();
                for child in inner {
                    if child.kind() == "call_expression" {
                        self.visit(&child, container, depth + 1);
                    } else {
                        self.callee(ReferenceKind::Call, &child, container, depth);
                    }
                }
            }
            "import_statement" => self.import(node, container),
            "export_statement" => self.export(node, container, depth),
            "variable_declarator" => self.declarator(node, container, depth),
            "jsx_opening_element" | "jsx_self_closing_element" => self.jsx(node, container, depth),
            "jsx_closing_element" => {}
            "member_expression" => self.member(node, container, depth, false),
            "assignment_expression" | "augmented_assignment_expression" => {
                self.assignment(node, container, depth)
            }
            "identifier" => {
                let form = self.plain_form(node);
                self.add(
                    ReferenceKind::FunctionValue,
                    node,
                    container,
                    Some(form),
                    None,
                );
            }
            "shorthand_property_identifier" => {
                self.add(
                    ReferenceKind::FunctionValue,
                    node,
                    container,
                    Some(CallForm::Shorthand),
                    None,
                );
            }
            "type_identifier" => {
                if !is_declared_name(node) {
                    self.add(ReferenceKind::TypeUse, node, container, None, None);
                }
            }
            "nested_type_identifier" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.add(ReferenceKind::TypeUse, &name, container, None, None);
                }
            }
            "extends_clause" | "implements_clause" | "class_heritage" => {
                self.heritage(node, container, depth)
            }
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "function_signature"
            | "method_signature"
            | "abstract_method_signature" => {
                let name = node
                    .child_by_field_name("name")
                    .and_then(|n| text(&n, self.source));
                self.function(node, name.or(container), depth);
            }
            "arrow_function" | "function_expression" | "function" | "generator_function" => {
                self.function(node, container, depth)
            }
            "object_pattern" | "array_pattern" | "rest_pattern" | "formal_parameters" => {
                self.pattern(node, container, depth)
            }
            "required_parameter" | "optional_parameter" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(&value, container, depth + 1);
                }
                self.parameter(node, container, depth)
            }
            "statement_identifier" | "property_identifier" | "private_property_identifier" => {}
            _ => self.children(node, container, depth),
        }
    }

    /// Only what is computed inside a pattern is a reference, and its type.
    fn pattern(&mut self, node: &Node<'a>, container: Option<&'a str>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "identifier" | "shorthand_property_identifier_pattern" => {}
                "assignment_pattern" | "object_assignment_pattern" => {
                    if let Some(right) = child.child_by_field_name("right") {
                        self.visit(&right, container, depth + 1);
                    }
                }
                "type_annotation" | "decorator" | "accessibility_modifier" => {
                    self.visit(&child, container, depth + 1)
                }
                "required_parameter" | "optional_parameter" => {
                    if let Some(value) = child.child_by_field_name("value") {
                        self.visit(&value, container, depth + 1);
                    }
                    self.parameter(&child, container, depth + 1);
                }
                _ => self.pattern(&child, container, depth + 1),
            }
        }
    }

    /// A parameter's pattern and type. Its default value is visited by the caller.
    fn parameter(&mut self, node: &Node<'a>, container: Option<&'a str>, depth: usize) {
        let value = node.child_by_field_name("value");
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if Some(child) == value {
                continue;
            }
            match child.kind() {
                "identifier" => {}
                "type_annotation" | "decorator" => self.visit(&child, container, depth + 1),
                _ => self.pattern(&child, container, depth + 1),
            }
        }
    }

    fn function(&mut self, node: &Node<'a>, container: Option<&'a str>, depth: usize) {
        let mut names = HashSet::new();
        if let Some(parameters) = node
            .child_by_field_name("parameters")
            .or_else(|| node.child_by_field_name("parameter"))
        {
            collect_bound(&parameters, self.source, &mut names, 0);
        }
        if let Some(body) = node.child_by_field_name("body") {
            collect_bound(&body, self.source, &mut names, 0);
        }
        self.bound.push(names);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "formal_parameters" => self.pattern(&child, container, depth + 1),
                "identifier"
                | "property_identifier"
                | "private_property_identifier"
                | "computed_property_name"
                | "type_identifier" => {}
                _ => self.visit(&child, container, depth + 1),
            }
        }
        self.bound.pop();
    }

    /// `const a = require('./a')` is an import.
    fn member(&mut self, node: &Node<'a>, container: Option<&'a str>, depth: usize, write: bool) {
        let object = node.child_by_field_name("object");
        if let Some(property) = node.child_by_field_name("property") {
            let receiver = object.as_ref().and_then(|o| self.receiver(o));
            let kind = if write {
                ReferenceKind::FieldWrite
            } else {
                ReferenceKind::FieldUse
            };
            self.add(kind, &property, container, None, receiver);
            self.mark_receiver(object.as_ref());
        }
        if let Some(object) = object {
            self.visit(&object, container, depth + 1);
        }
    }

    fn assignment(&mut self, node: &Node<'a>, container: Option<&'a str>, depth: usize) {
        if let Some(left) = node.child_by_field_name("left") {
            if left.kind() == "member_expression" {
                self.member(&left, container, depth, true);
                // `a.x += 1` reads it as well.
                if node.kind() == "augmented_assignment_expression"
                    && let Some(property) = left.child_by_field_name("property")
                {
                    self.add(ReferenceKind::FieldUse, &property, container, None, None);
                }
            } else if left.kind() != "identifier" {
                self.visit(&left, container, depth + 1);
            }
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.visit(&right, container, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests;
