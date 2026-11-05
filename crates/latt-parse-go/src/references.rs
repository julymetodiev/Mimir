//! References of Go, at the name, where a language server is asked. A name is read through the scopes
//! around it, since a name a function binds is no `FunctionValue` and gives no `Calls`.

use std::collections::HashSet;

use latt_parse_core::ParsedTree;
use latt_parse_core::extracted::{CallForm, ExtractedReference};
use latt_parse_core::nodes::{MAX_DEPTH, range_of, span_of, text};
use latt_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

mod imports;

/// Extract the references of a parse tree.
pub fn extract_references(tree: &ParsedTree) -> Vec<ExtractedReference> {
    let source = tree.source_bytes();
    let root = tree.root_node();
    let mut walk = Walk {
        source,
        found: Vec::new(),
        scopes: Vec::new(),
        imported: HashSet::new(),
    };
    walk.imports(&root);
    walk.children(&root, 0);
    walk.found
}

struct Walk<'a> {
    source: &'a [u8],
    found: Vec<ExtractedReference>,
    /// The names bound in each scope around here.
    scopes: Vec<HashSet<&'a str>>,
    /// The names a file's imports bind.
    imported: HashSet<String>,
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

    fn bind(&mut self, node: &Node<'a>) {
        if let (Some(name), Some(scope)) = (text(node, self.source), self.scopes.last_mut()) {
            scope.insert(name);
        }
    }

    /// Bind each identifier of a list of them, or the one itself.
    fn bind_all(&mut self, node: &Node<'a>) {
        if node.kind() == "identifier" {
            self.bind(node);
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "identifier" {
                self.bind(&child);
            }
        }
    }

    /// Whether a name here is a package, which is no value and no call of its own.
    fn is_package(&self, node: &Node<'a>) -> Option<&'a str> {
        if node.kind() != "identifier" {
            return None;
        }
        text(node, self.source).filter(|name| !self.is_bound(name) && self.imported.contains(*name))
    }

    /// What a receiver is, when it is short enough to say.
    fn receiver(&self, operand: &Node<'a>) -> Option<&'a str> {
        (operand.kind() == "identifier")
            .then(|| text(operand, self.source))
            .flatten()
    }

    fn visit(&mut self, node: &Node<'a>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        match node.kind() {
            "comment" | "package_clause" | "import_declaration" => {}
            "function_declaration" | "func_literal" => self.function(node, depth),
            "method_declaration" => {
                self.declared_method(node);
                self.function(node, depth);
            }
            "method_elem" => {
                self.declared_method(node);
                self.scoped(node, depth);
            }
            "block"
            | "if_statement"
            | "for_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "select_statement"
            | "expression_case"
            | "default_case"
            | "type_case"
            | "communication_case"
            | "function_type" => self.scoped_statement(node, depth),
            "parameter_declaration" | "variadic_parameter_declaration" => {
                self.value_spec(node, depth)
            }
            "var_spec" | "const_spec" => self.value_spec(node, depth),
            "type_parameter_declaration" => {
                let mut cursor = node.walk();
                for name in node.children_by_field_name("name", &mut cursor) {
                    self.bind(&name);
                }
                if let Some(kind) = node.child_by_field_name("type") {
                    self.visit(&kind, depth + 1);
                }
            }
            "type_spec" | "type_alias" => self.type_spec(node, depth),
            "field_declaration" => {
                if let Some(kind) = node.child_by_field_name("type") {
                    self.visit(&kind, depth + 1);
                }
            }
            "type_identifier" => {
                if text(node, self.source).is_some_and(|name| !self.is_bound(name)) {
                    self.add(ReferenceKind::TypeUse, node, None, None);
                }
            }
            "qualified_type" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let package = node
                        .child_by_field_name("package")
                        .and_then(|package| text(&package, self.source));
                    self.add(ReferenceKind::TypeUse, &name, None, package);
                }
            }
            "call_expression" => self.call(node, depth),
            // `f[int](1)`, which the grammar reads as a conversion, and it may be one.
            "type_conversion_expression" => self.generic_call(node, depth),
            "selector_expression" => self.selector(node, depth, false),
            "short_var_declaration" => {
                if let Some(right) = node.child_by_field_name("right") {
                    self.visit(&right, depth + 1);
                }
                if let Some(left) = node.child_by_field_name("left") {
                    self.bind_all(&left);
                }
            }
            "assignment_statement" => self.assignment(node, depth),
            "range_clause" => self.range_clause(node, depth),
            "keyed_element" => self.keyed_element(node, depth),
            "identifier" => {
                let plain = text(node, self.source).is_some_and(|name| {
                    name != "_" && !self.is_bound(name) && !self.imported.contains(name)
                });
                if plain {
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

    fn declared_method(&mut self, node: &Node<'a>) {
        if let Some(name) = node.child_by_field_name("name") {
            self.add(ReferenceKind::MethodDeclaration, &name, None, None);
        }
    }

    /// A statement or a type that opens a scope, and what its head binds.
    fn scoped_statement(&mut self, node: &Node<'a>, depth: usize) {
        if node.kind() != "type_switch_statement" {
            return self.scoped(node, depth);
        }
        self.scopes.push(HashSet::new());
        // `switch v := x.(type)`: `v` is bound in the cases, after what it is worked out from.
        if let Some(initializer) = node.child_by_field_name("initializer") {
            self.visit(&initializer, depth + 1);
        }
        if let Some(value) = node.child_by_field_name("value") {
            self.visit(&value, depth + 1);
        }
        if let Some(alias) = node.child_by_field_name("alias") {
            self.bind_all(&alias);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "type_case" | "default_case") {
                self.visit(&child, depth + 1);
            }
        }
        self.scopes.pop();
    }

    /// A function: what it binds is in its own scope, and its name is no reference.
    fn function(&mut self, node: &Node<'a>, depth: usize) {
        self.scopes.push(HashSet::new());
        for field in [
            "receiver",
            "type_parameters",
            "parameters",
            "result",
            "body",
        ] {
            if let Some(part) = node.child_by_field_name(field) {
                self.visit(&part, depth + 1);
            }
        }
        self.scopes.pop();
    }

    /// `name type = value`: what it is worked out from comes before the name is bound.
    fn value_spec(&mut self, node: &Node<'a>, depth: usize) {
        for field in ["type", "value"] {
            if let Some(part) = node.child_by_field_name(field) {
                self.visit(&part, depth + 1);
            }
        }
        let mut cursor = node.walk();
        for name in node.children_by_field_name("name", &mut cursor) {
            self.bind(&name);
        }
    }

    fn type_spec(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(name) = node.child_by_field_name("name") {
            self.bind(&name);
        }
        self.scopes.push(HashSet::new());
        for field in ["type_parameters", "type"] {
            if let Some(part) = node.child_by_field_name(field) {
                self.visit(&part, depth + 1);
            }
        }
        self.scopes.pop();
    }

    fn call(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(function) = node.child_by_field_name("function") {
            self.callee(&function, depth);
        }
        for field in ["type_arguments", "arguments"] {
            if let Some(part) = node.child_by_field_name(field) {
                self.visit(&part, depth + 1);
            }
        }
    }

    fn generic_call(&mut self, node: &Node<'a>, depth: usize) {
        match node.child_by_field_name("type") {
            Some(generic) if generic.kind() == "generic_type" => {
                if let Some(called) = generic.child_by_field_name("type") {
                    self.callee_type(&called, depth + 1);
                }
                if let Some(arguments) = generic.child_by_field_name("type_arguments") {
                    self.visit(&arguments, depth + 1);
                }
            }
            Some(other) => self.visit(&other, depth + 1),
            None => {}
        }
        if let Some(operand) = node.child_by_field_name("operand") {
            self.visit(&operand, depth + 1);
        }
    }

    /// What is called: `f`, `a.f`, `f[int]`, or anything else, of which no name can be said.
    fn callee(&mut self, callee: &Node<'a>, depth: usize) {
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
            "selector_expression" => {
                let operand = callee.child_by_field_name("operand");
                let package = operand.as_ref().and_then(|o| self.is_package(o));
                if let Some(field) = callee.child_by_field_name("field") {
                    if let Some(package) = package {
                        self.add(
                            ReferenceKind::Call,
                            &field,
                            Some(CallForm::Scoped),
                            Some(package),
                        );
                    } else {
                        let receiver = operand.as_ref().and_then(|o| self.receiver(o));
                        self.add(
                            ReferenceKind::Call,
                            &field,
                            Some(CallForm::Method),
                            receiver,
                        );
                    }
                }
                if let (Some(operand), None) = (operand, package) {
                    self.visit(&operand, depth + 1);
                }
            }
            "index_expression" => {
                if let Some(operand) = callee.child_by_field_name("operand") {
                    self.callee(&operand, depth + 1);
                }
                if let Some(index) = callee.child_by_field_name("index") {
                    self.visit(&index, depth + 1);
                }
            }
            "type_instantiation_expression" => {
                let mut cursor = callee.walk();
                let parts: Vec<Node<'a>> = callee.named_children(&mut cursor).collect();
                if let Some((called, arguments)) = parts.split_first() {
                    self.callee_type(called, depth + 1);
                    for argument in arguments {
                        self.visit(argument, depth + 1);
                    }
                }
            }
            "parenthesized_expression" => {
                if let Some(inner) = callee.named_child(0) {
                    self.callee(&inner, depth + 1);
                }
            }
            _ => self.visit(callee, depth + 1),
        }
    }

    /// What `f[int]()` calls, which the grammar reads as a type.
    fn callee_type(&mut self, called: &Node<'a>, depth: usize) {
        match called.kind() {
            "type_identifier" => {
                let bound = text(called, self.source).is_some_and(|name| self.is_bound(name));
                let form = if bound {
                    CallForm::Bound
                } else {
                    CallForm::Plain
                };
                self.add(ReferenceKind::Call, called, Some(form), None);
            }
            "qualified_type" => {
                let package = called
                    .child_by_field_name("package")
                    .and_then(|package| text(&package, self.source));
                if let Some(name) = called.child_by_field_name("name") {
                    self.add(ReferenceKind::Call, &name, Some(CallForm::Scoped), package);
                }
            }
            _ => self.visit(called, depth),
        }
    }

    /// `x.f` not called: what it is, only the answer says, as it may be a method or `pkg.Name`.
    fn selector(&mut self, node: &Node<'a>, depth: usize, write: bool) {
        let operand = node.child_by_field_name("operand");
        let package = operand.as_ref().and_then(|o| self.is_package(o));
        if let Some(field) = node.child_by_field_name("field") {
            let kind = if write {
                ReferenceKind::FieldWrite
            } else {
                ReferenceKind::FieldUse
            };
            let context = package.or_else(|| operand.as_ref().and_then(|o| self.receiver(o)));
            let form = package.map(|_| CallForm::Scoped);
            self.add(kind, &field, form, context);
        }
        if let (Some(operand), None) = (operand, package) {
            self.visit(&operand, depth + 1);
        }
    }

    fn assignment(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(right) = node.child_by_field_name("right") {
            self.visit(&right, depth + 1);
        }
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        let mut cursor = left.walk();
        for target in left.named_children(&mut cursor) {
            if target.kind() == "selector_expression" {
                self.selector(&target, depth + 1, true);
            } else {
                self.visit(&target, depth + 1);
            }
        }
    }

    /// `for k, v := range x` binds, and `for k, v = range x` does not.
    fn range_clause(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(right) = node.child_by_field_name("right") {
            self.visit(&right, depth + 1);
        }
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        let mut cursor = node.walk();
        let binds = node.children(&mut cursor).any(|child| child.kind() == ":=");
        if binds {
            self.bind_all(&left);
        } else {
            self.visit(&left, depth + 1);
        }
    }

    /// `T{x: 1}`: a name before a colon may be a field, or in a map a value. Only the answer says.
    fn keyed_element(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(key) = node.child_by_field_name("key") {
            let name = if key.kind() == "identifier" {
                Some(key)
            } else {
                key.named_child(0)
                    .filter(|inner| inner.kind() == "identifier")
            };
            match name {
                Some(name) => self.add(ReferenceKind::FieldUse, &name, None, None),
                None => self.visit(&key, depth + 1),
            }
        }
        if let Some(value) = node.child_by_field_name("value") {
            self.visit(&value, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests;
