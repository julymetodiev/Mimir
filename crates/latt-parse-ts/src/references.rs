//! References of TypeScript and JavaScript, at the name, where a language server is asked.

use std::collections::HashSet;

use latt_parse_core::ParsedTree;
use latt_parse_core::extracted::{CallForm, ExtractedReference};
use latt_parse_core::nodes::{MAX_DEPTH, range_of, span_of, text};
use latt_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

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
    walk.children(&tree.root_node(), 0);
    walk.found
}

struct Walk<'a> {
    source: &'a [u8],
    found: Vec<ExtractedReference>,
    /// The names bound by each function around here.
    bound: Vec<HashSet<&'a str>>,
}

impl<'a> Walk<'a> {
    fn children(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(&child, depth + 1);
        }
    }

    fn add(
        &mut self,
        kind: ReferenceKind,
        at: &Node<'a>,
        form: Option<CallForm>,
        context: Option<&str>,
    ) {
        let Some(name) = text(at, self.source) else {
            return;
        };
        let mut reference =
            ExtractedReference::new(name.to_string(), kind, range_of(at), span_of(at))
                .with_confidence(Confidence::Low);
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
            // Asked at a function's name, the answer is the type it returns.
            "call_expression" => {
                object
                    .child_by_field_name("function")
                    .and_then(|f| match f.kind() {
                        "identifier" => Some(f),
                        "member_expression" => f.child_by_field_name("property"),
                        _ => None,
                    })
            }
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
    fn callee(&mut self, kind: ReferenceKind, callee: &Node<'a>, depth: usize) {
        match callee.kind() {
            "identifier" => {
                let form = self.plain_form(callee);
                self.add(kind, callee, Some(form), None);
            }
            "super" => self.add(kind, callee, Some(CallForm::Method), None),
            "member_expression" => {
                let object = callee.child_by_field_name("object");
                if let Some(property) = callee.child_by_field_name("property") {
                    let receiver = object.as_ref().and_then(|o| self.receiver(o));
                    self.add(kind, &property, Some(CallForm::Method), receiver);
                    self.mark_receiver(object.as_ref());
                }
                if let Some(object) = object {
                    self.visit(&object, depth + 1);
                }
            }
            _ => self.visit(callee, depth + 1),
        }
    }

    fn visit(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        match node.kind() {
            "comment" => {}
            "call_expression" => {
                if let Some(function) = node.child_by_field_name("function") {
                    self.callee(ReferenceKind::Call, &function, depth);
                }
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    self.visit(&arguments, depth + 1);
                }
            }
            "new_expression" => {
                if let Some(constructor) = node.child_by_field_name("constructor") {
                    self.callee(ReferenceKind::Call, &constructor, depth);
                }
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    self.visit(&arguments, depth + 1);
                }
            }
            "decorator" => {
                let mut cursor = node.walk();
                let inner: Vec<_> = node.named_children(&mut cursor).collect();
                for child in inner {
                    if child.kind() == "call_expression" {
                        self.visit(&child, depth + 1);
                    } else {
                        self.callee(ReferenceKind::Call, &child, depth);
                    }
                }
            }
            "import_statement" => self.import(node),
            "export_statement" => self.export(node, depth),
            "variable_declarator" => self.declarator(node, depth),
            "jsx_opening_element" | "jsx_self_closing_element" => self.jsx(node, depth),
            "jsx_closing_element" => {}
            "member_expression" => self.member(node, depth, false),
            "assignment_expression" | "augmented_assignment_expression" => {
                self.assignment(node, depth)
            }
            // A name the function binds is nearly always its own, and the most common thing in
            // code: a function it hides further in is what is lost by leaving them out.
            "identifier" => {
                if self.plain_form(node) == CallForm::Plain {
                    self.add(
                        ReferenceKind::FunctionValue,
                        node,
                        Some(CallForm::Plain),
                        None,
                    );
                }
            }
            "shorthand_property_identifier" => {
                self.add(
                    ReferenceKind::FunctionValue,
                    node,
                    Some(CallForm::Shorthand),
                    None,
                );
            }
            "type_identifier" => {
                if !is_declared_name(node) {
                    self.add(ReferenceKind::TypeUse, node, None, None);
                }
            }
            "nested_type_identifier" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.add(ReferenceKind::TypeUse, &name, None, None);
                }
            }
            "extends_clause" | "implements_clause" | "class_heritage" => self.heritage(node, depth),
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "function_signature"
            | "method_signature"
            | "abstract_method_signature" => {
                self.function(node, depth);
            }
            "arrow_function" | "function_expression" | "function" | "generator_function" => {
                self.function(node, depth)
            }
            "object_pattern" | "array_pattern" | "rest_pattern" | "formal_parameters" => {
                self.pattern(node, depth)
            }
            "required_parameter" | "optional_parameter" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(&value, depth + 1);
                }
                self.parameter(node, depth)
            }
            "statement_identifier" | "property_identifier" | "private_property_identifier" => {}
            _ => self.children(node, depth),
        }
    }

    /// Only what is computed inside a pattern is a reference, and its type.
    fn pattern(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "identifier" | "shorthand_property_identifier_pattern" => {}
                "assignment_pattern" | "object_assignment_pattern" => {
                    if let Some(right) = child.child_by_field_name("right") {
                        self.visit(&right, depth + 1);
                    }
                }
                "type_annotation" | "decorator" | "accessibility_modifier" => {
                    self.visit(&child, depth + 1)
                }
                "required_parameter" | "optional_parameter" => {
                    if let Some(value) = child.child_by_field_name("value") {
                        self.visit(&value, depth + 1);
                    }
                    self.parameter(&child, depth + 1);
                }
                _ => self.pattern(&child, depth + 1),
            }
        }
    }

    /// A parameter's pattern and type. Its default value is visited by the caller.
    fn parameter(&mut self, node: &Node<'a>, depth: usize) {
        let value = node.child_by_field_name("value");
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if Some(child) == value {
                continue;
            }
            match child.kind() {
                "identifier" => {}
                "type_annotation" | "decorator" => self.visit(&child, depth + 1),
                _ => self.pattern(&child, depth + 1),
            }
        }
    }

    fn function(&mut self, node: &Node<'a>, depth: usize) {
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
                "formal_parameters" => self.pattern(&child, depth + 1),
                "identifier"
                | "property_identifier"
                | "private_property_identifier"
                | "computed_property_name"
                | "type_identifier" => {}
                _ => self.visit(&child, depth + 1),
            }
        }
        self.bound.pop();
    }

    /// `const a = require('./a')` is an import.
    fn member(&mut self, node: &Node<'a>, depth: usize, write: bool) {
        let object = node.child_by_field_name("object");
        if let Some(property) = node.child_by_field_name("property") {
            let receiver = object.as_ref().and_then(|o| self.receiver(o));
            let kind = if write {
                ReferenceKind::FieldWrite
            } else {
                ReferenceKind::FieldUse
            };
            self.add(kind, &property, None, receiver);
            self.mark_receiver(object.as_ref());
        }
        if let Some(object) = object {
            self.visit(&object, depth + 1);
        }
    }

    fn assignment(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(left) = node.child_by_field_name("left") {
            if left.kind() == "member_expression" {
                self.member(&left, depth, true);
                // `a.x += 1` reads it as well.
                if node.kind() == "augmented_assignment_expression"
                    && let Some(property) = left.child_by_field_name("property")
                {
                    self.add(ReferenceKind::FieldUse, &property, None, None);
                }
            } else if left.kind() != "identifier" {
                self.visit(&left, depth + 1);
            }
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.visit(&right, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests;
