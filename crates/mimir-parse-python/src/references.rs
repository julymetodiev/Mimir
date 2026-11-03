//! References of Python, at the name, where a language server is asked.

use std::collections::HashSet;

use mimir_parse_core::ParsedTree;
use mimir_parse_core::extracted::{CallForm, ExtractedReference};
use mimir_parse_core::nodes::{MAX_DEPTH, range_of, span_of, text};
use mimir_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

mod forms;
mod names;

use names::collect_bound;

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

    fn plain_form(&self, node: &Node<'a>) -> CallForm {
        let bound = text(node, self.source)
            .is_some_and(|name| self.bound.iter().any(|names| names.contains(name)));
        if bound {
            CallForm::Bound
        } else {
            CallForm::Plain
        }
    }

    /// `@size.setter`, which declares a method and neither calls nor reads the one named `size`.
    fn is_accessor(&self, decorator: &Node<'a>) -> bool {
        decorator.kind() == "attribute"
            && decorator
                .child_by_field_name("attribute")
                .and_then(|name| text(&name, self.source))
                .is_some_and(|name| matches!(name, "setter" | "getter" | "deleter"))
    }

    /// What a receiver is, when it is short enough to say.
    fn receiver(&self, object: &Node<'a>) -> Option<&'a str> {
        (object.kind() == "identifier")
            .then(|| text(object, self.source))
            .flatten()
    }

    /// A call of `callee`, which is `f`, `a.f` or anything else, of which no name can be said.
    fn callee(&mut self, callee: &Node<'a>, depth: usize) {
        match callee.kind() {
            "identifier" => {
                let form = self.plain_form(callee);
                self.add(ReferenceKind::Call, callee, Some(form), None);
            }
            "attribute" => {
                let object = callee.child_by_field_name("object");
                if let Some(attribute) = callee.child_by_field_name("attribute") {
                    let receiver = object.as_ref().and_then(|o| self.receiver(o));
                    self.add(
                        ReferenceKind::Call,
                        &attribute,
                        Some(CallForm::Method),
                        receiver,
                    );
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
            "comment" | "global_statement" | "nonlocal_statement" => {}
            "call" => {
                if let Some(function) = node.child_by_field_name("function") {
                    self.callee(&function, depth);
                }
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    self.visit(&arguments, depth + 1);
                }
            }
            "decorator" => {
                let mut cursor = node.walk();
                let inner: Vec<_> = node.named_children(&mut cursor).collect();
                for child in inner {
                    if child.kind() == "call" {
                        self.visit(&child, depth + 1);
                    } else if child.kind() != "comment" && !self.is_accessor(&child) {
                        self.callee(&child, depth);
                    }
                }
            }
            "keyword_argument" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(&value, depth + 1);
                }
            }
            "import_statement" => self.import(node),
            "import_from_statement" => self.import_from(node),
            "assignment" | "augmented_assignment" => self.assignment(node, depth),
            "named_expression" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(&value, depth + 1);
                }
            }
            "for_statement" | "for_in_clause" => self.for_loop(node, depth),
            "as_pattern" => {
                if let Some(value) = node.named_child(0) {
                    self.visit(&value, depth + 1);
                }
            }
            "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression" => self.comprehension(node, depth),
            "attribute" => self.attribute(node, depth, false),
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
            "type" => self.types(node, depth),
            "type_alias_statement" => {
                if let Some(right) = node.child_by_field_name("right") {
                    self.types(&right, depth + 1);
                }
            }
            "class_definition" => self.class(node, depth),
            "function_definition" | "lambda" => self.function(node, depth),
            _ => self.children(node, depth),
        }
    }

    fn attribute(&mut self, node: &Node<'a>, depth: usize, write: bool) {
        let object = node.child_by_field_name("object");
        if let Some(attribute) = node.child_by_field_name("attribute") {
            let receiver = object.as_ref().and_then(|o| self.receiver(o));
            let kind = if write {
                ReferenceKind::FieldWrite
            } else {
                ReferenceKind::FieldUse
            };
            self.add(kind, &attribute, None, receiver);
        }
        if let Some(object) = object {
            self.visit(&object, depth + 1);
        }
    }

    /// What is written to: a name binds, and only what it is computed from is a reference.
    fn target(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        match node.kind() {
            "identifier" => {}
            "attribute" => self.attribute(node, depth, true),
            "subscript" => self.children(node, depth),
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.target(&child, depth + 1);
                }
            }
        }
    }

    fn assignment(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(left) = node.child_by_field_name("left") {
            self.target(&left, depth + 1);
            // `a.x += 1` reads it as well.
            if node.kind() == "augmented_assignment"
                && left.kind() == "attribute"
                && let Some(attribute) = left.child_by_field_name("attribute")
            {
                self.add(ReferenceKind::FieldUse, &attribute, None, None);
            }
        }
        if let Some(annotation) = node.child_by_field_name("type") {
            self.types(&annotation, depth + 1);
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.visit(&right, depth + 1);
        }
    }

    fn for_loop(&mut self, node: &Node<'a>, depth: usize) {
        let left = node.child_by_field_name("left");
        if let Some(left) = &left {
            self.target(left, depth + 1);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if Some(child) != left {
                self.visit(&child, depth + 1);
            }
        }
    }

    /// `f(x) for x in y`: what it binds is bound in all of it.
    fn comprehension(&mut self, node: &Node<'a>, depth: usize) {
        let mut names = HashSet::new();
        collect_bound(node, self.source, &mut names, 0);
        self.bound.push(names);
        self.children(node, depth);
        self.bound.pop();
    }

    /// A type: every name in it is a use, and a string in it has not been tried.
    fn types(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        match node.kind() {
            "identifier" => self.add(ReferenceKind::TypeUse, node, None, None),
            "attribute" => {
                if let Some(attribute) = node.child_by_field_name("attribute") {
                    self.add(ReferenceKind::TypeUse, &attribute, None, None);
                }
            }
            "string" | "comment" => {}
            "call" => self.visit(node, depth),
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.types(&child, depth + 1);
                }
            }
        }
    }

    fn function(&mut self, node: &Node<'a>, depth: usize) {
        let mut names = HashSet::new();
        let parameters = node.child_by_field_name("parameters");
        let body = node.child_by_field_name("body");
        if let Some(parameters) = &parameters {
            collect_bound(parameters, self.source, &mut names, 0);
        }
        if let Some(body) = &body {
            collect_bound(body, self.source, &mut names, 0);
        }
        // A default value and a type are worked out outside the function.
        if let Some(parameters) = &parameters {
            self.parameters(parameters, depth + 1);
        }
        if let Some(returned) = node.child_by_field_name("return_type") {
            self.types(&returned, depth + 1);
        }
        self.bound.push(names);
        if let Some(body) = &body {
            self.visit(body, depth + 1);
        }
        self.bound.pop();
    }

    /// Only the types and the default values of parameters are references.
    fn parameters(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if let Some(annotation) = child.child_by_field_name("type") {
                self.types(&annotation, depth + 1);
            }
            if let Some(value) = child.child_by_field_name("value") {
                self.visit(&value, depth + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests;
