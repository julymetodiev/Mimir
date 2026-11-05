//! References of C and C++, at the name, where a language server is asked (`docs/CPP.md`, section 3).
//! A name is read through the scopes around it, since a name a function binds is no `FunctionValue`
//! and gives no `Calls`. An operator is a call the source does not spell, so it gives no reference, and
//! a macro's body is text the grammar does not read.

use std::collections::HashSet;

use latt_parse_core::ParsedTree;
use latt_parse_core::extracted::{CallForm, ExtractedReference};
use latt_parse_core::nodes::{MAX_DEPTH, range_of, span_of, text};
use latt_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

use crate::declarators::{function_declarator, last_name, named_by};

mod calls;

/// Extract the references of a parse tree.
pub fn extract_references(tree: &ParsedTree) -> Vec<ExtractedReference> {
    let mut walk = Walk {
        source: tree.source_bytes(),
        found: Vec::new(),
        scopes: Vec::new(),
    };
    walk.children(&tree.root_node(), 0);
    walk.found
}

pub struct Walk<'a> {
    source: &'a [u8],
    found: Vec<ExtractedReference>,
    /// The names bound in each scope around here.
    scopes: Vec<HashSet<&'a str>>,
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

    fn scoped(&mut self, node: &Node<'a>, depth: usize) {
        self.scopes.push(HashSet::new());
        self.children(node, depth);
        self.scopes.pop();
    }

    fn field(&mut self, node: &Node<'a>, name: &str, depth: usize) {
        if let Some(part) = node.child_by_field_name(name) {
            self.visit(&part, depth + 1);
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
        self.scopes.iter().any(|scope| scope.contains(name))
    }

    /// Bind what a declarator names, in the scope here.
    fn bind(&mut self, declarator: &Node<'a>, depth: usize) {
        let at = named_by(*declarator, depth);
        if let (Some(name), Some(scope)) = (text(&at, self.source), self.scopes.last_mut())
            && matches!(at.kind(), "identifier" | "field_identifier")
        {
            scope.insert(name);
        }
    }

    fn visit(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        match node.kind() {
            "comment"
            | "preproc_def"
            | "preproc_function_def"
            | "preproc_call"
            | "using_declaration"
            | "namespace_alias_definition"
            | "field_identifier"
            | "namespace_identifier"
            | "string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "number_literal" => {}
            "preproc_include" => self.include(node),
            "preproc_if" | "preproc_elif" => {
                let mut cursor = node.walk();
                let condition = node.child_by_field_name("condition").map(|c| c.id());
                for child in node.named_children(&mut cursor) {
                    if Some(child.id()) != condition {
                        self.visit(&child, depth + 1);
                    }
                }
            }
            "preproc_ifdef" | "preproc_elifdef" => {
                let mut cursor = node.walk();
                let name = node.child_by_field_name("name").map(|c| c.id());
                for child in node.named_children(&mut cursor) {
                    if Some(child.id()) != name {
                        self.visit(&child, depth + 1);
                    }
                }
            }
            "function_definition" => self.function(node, depth),
            "lambda_expression" => {
                self.scopes.push(HashSet::new());
                self.field(node, "captures", depth);
                // Its parameters are in its own scope, with the body that reads them.
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    self.field(&declarator, "parameters", depth);
                }
                self.field(node, "body", depth);
                self.scopes.pop();
            }
            "template_declaration" => self.scoped(node, depth),
            "type_parameter_declaration" | "variadic_type_parameter_declaration" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let (true, Some(name), Some(scope)) = (
                        child.kind() == "type_identifier",
                        text(&child, self.source),
                        self.scopes.last_mut(),
                    ) {
                        scope.insert(name);
                    }
                }
            }
            "compound_statement"
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "do_statement"
            | "switch_statement"
            | "case_statement"
            | "catch_clause"
            | "function_declarator"
            | "abstract_function_declarator" => self.scoped(node, depth),
            "for_range_loop" => self.range_loop(node, depth),
            "parameter_declaration" | "optional_parameter_declaration" => {
                self.field(node, "type", depth);
                self.field(node, "default_value", depth);
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    self.bind(&declarator, depth);
                }
            }
            "declaration" => self.declaration(node, depth),
            "field_declaration" => self.field_declaration(node, depth),
            "type_definition" | "alias_declaration" => self.field(node, "type", depth),
            "class_specifier" | "struct_specifier" | "union_specifier" => self.record(node, depth),
            "enum_specifier" => self.field(node, "body", depth),
            "enumerator" => self.field(node, "value", depth),
            "type_identifier" => {
                if text(node, self.source).is_some_and(|name| !self.is_bound(name)) {
                    self.add(ReferenceKind::TypeUse, node, None, None);
                }
            }
            "template_type" => {
                self.field(node, "name", depth);
                self.field(node, "arguments", depth);
            }
            "qualified_identifier" => self.qualified(node, depth),
            "call_expression" => {
                if let Some(function) = node.child_by_field_name("function") {
                    self.callee(&function, depth);
                }
                self.field(node, "arguments", depth);
            }
            "new_expression" => self.new_expression(node, depth),
            "field_expression" => self.member(node, depth, false),
            // `A::A() : field(1)`.
            "field_initializer" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "field_identifier" {
                        self.add(ReferenceKind::FieldWrite, &child, None, None);
                    } else {
                        self.visit(&child, depth + 1);
                    }
                }
            }
            "assignment_expression" => {
                self.field(node, "right", depth);
                match node.child_by_field_name("left") {
                    Some(left) if left.kind() == "field_expression" => {
                        self.member(&left, depth + 1, true);
                    }
                    Some(left) => self.visit(&left, depth + 1),
                    None => {}
                }
            }
            "identifier" => {
                if text(node, self.source).is_some_and(|name| !self.is_bound(name)) {
                    self.add(
                        ReferenceKind::FunctionValue,
                        node,
                        Some(CallForm::Plain),
                        None,
                    );
                }
            }
            _ => self.children(node, depth),
        }
    }

    fn include(&mut self, node: &Node<'a>) {
        let Some(path) = node.child_by_field_name("path") else {
            return;
        };
        let Some(written) = text(&path, self.source) else {
            return;
        };
        let name = written.trim_matches(['"', '<', '>']);
        let mut reference = ExtractedReference::new(
            name.to_string(),
            ReferenceKind::Import,
            range_of(&path),
            span_of(&path),
        )
        .with_confidence(Confidence::Low);
        reference = reference.with_context(name.to_string());
        self.found.push(reference);
    }

    /// A function: what it binds is in its own scope, and its name is no reference, though a method
    /// says where it is, for what overrides it.
    fn function(&mut self, node: &Node<'a>, depth: usize) {
        self.scopes.push(HashSet::new());
        self.field(node, "type", depth);
        if let Some(declarator) = node.child_by_field_name("declarator") {
            self.declarator_parts(&declarator, depth);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "field_initializer_list" {
                self.visit(&child, depth + 1);
            }
        }
        self.field(node, "body", depth);
        self.scopes.pop();
    }

    /// The parameters of a function's declarator, in the scope here: not its name.
    fn declarator_parts(&mut self, declarator: &Node<'a>, depth: usize) {
        let at = function_declarator(*declarator, depth);
        let Some(at) = at else {
            return;
        };
        self.field(&at, "parameters", depth);
        if let Some(name) = at.child_by_field_name("declarator") {
            let name = named_by(name, depth);
            let written = if name.kind() == "qualified_identifier" {
                self.scopes_written(&name, depth);
                last_name(name, depth)
            } else {
                Some(name)
            };
            if let Some(name) = written.filter(|n| {
                matches!(
                    n.kind(),
                    "field_identifier" | "identifier" | "destructor_name"
                )
            }) {
                self.add(ReferenceKind::MethodDeclaration, &name, None, None);
            }
        }
    }

    /// `A::B::f`: the classes in a name are types used, and only the answer says which are.
    fn scopes_written(&mut self, name: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        if let Some(scope) = name.child_by_field_name("scope") {
            let written = if scope.kind() == "template_type" {
                scope.child_by_field_name("name").unwrap_or(scope)
            } else {
                scope
            };
            self.add(ReferenceKind::TypeUse, &written, None, None);
        }
        if let Some(inner) = name
            .child_by_field_name("name")
            .filter(|inner| inner.kind() == "qualified_identifier")
        {
            self.scopes_written(&inner, depth + 1);
        }
    }

    /// `T x = 1`, `T x(1)`, `T f(int)`. What it binds is bound after what it is worked out from.
    fn declaration(&mut self, node: &Node<'a>, depth: usize) {
        self.field(node, "type", depth);
        let mut cursor = node.walk();
        let declarators: Vec<Node<'a>> = node
            .children_by_field_name("declarator", &mut cursor)
            .collect();
        for declarator in declarators {
            if declarator.kind() == "init_declarator" {
                let name = declarator.child_by_field_name("declarator");
                if let Some(value) = declarator.child_by_field_name("value") {
                    // `T x(1)` runs a constructor, which an index gives at the name.
                    if value.kind() == "argument_list"
                        && let Some(name) = name.map(|name| named_by(name, depth))
                    {
                        self.add(ReferenceKind::Call, &name, None, None);
                    }
                    self.visit(&value, depth + 1);
                }
                if let Some(name) = name {
                    self.bind(&name, depth);
                }
            } else if function_declarator(declarator, depth).is_some() {
                // In a function `T x(y)` may be a variable that runs a constructor: only the answer says.
                if !self.scopes.is_empty() {
                    let name = named_by(declarator, depth);
                    self.add(ReferenceKind::Call, &name, None, None);
                }
                self.scopes.push(HashSet::new());
                self.declarator_parts(&declarator, depth);
                self.scopes.pop();
            } else {
                self.bind(&declarator, depth);
            }
        }
    }

    fn field_declaration(&mut self, node: &Node<'a>, depth: usize) {
        self.field(node, "type", depth);
        self.field(node, "default_value", depth);
        let mut cursor = node.walk();
        let declarators: Vec<Node<'a>> = node
            .children_by_field_name("declarator", &mut cursor)
            .collect();
        for declarator in declarators {
            if function_declarator(declarator, depth).is_some() {
                self.scopes.push(HashSet::new());
                self.declarator_parts(&declarator, depth);
                self.scopes.pop();
            }
        }
    }

    /// A class: its bases, and its body. Its own name is no use of it.
    fn record(&mut self, node: &Node<'a>, depth: usize) {
        let has_body = node.child_by_field_name("body").is_some();
        if !has_body {
            self.field(node, "name", depth);
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "base_class_clause" {
                self.visit(&child, depth + 1);
            }
        }
        self.field(node, "body", depth);
    }

    fn range_loop(&mut self, node: &Node<'a>, depth: usize) {
        self.scopes.push(HashSet::new());
        self.field(node, "type", depth);
        self.field(node, "right", depth);
        if let Some(declarator) = node.child_by_field_name("declarator") {
            self.bind(&declarator, depth);
        }
        self.field(node, "body", depth);
        self.scopes.pop();
    }
}

#[cfg(test)]
mod tests;
