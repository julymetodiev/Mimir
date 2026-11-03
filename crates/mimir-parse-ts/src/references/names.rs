//! What the walk over references asks of a node: the names it binds and the paths it names.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::{MAX_DEPTH, text};

/// Whether a type's name is where it is declared, not where it is used.
pub fn is_declared_name(node: &Node) -> bool {
    node.parent()
        .and_then(|parent| parent.child_by_field_name("name"))
        .is_some_and(|name| name == *node)
}

/// The text of `from '...'`.
pub fn module_path<'a>(node: &Node, source: &'a [u8]) -> Option<&'a str> {
    string_content(&node.child_by_field_name("source")?, source)
}

pub fn string_content<'a>(string: &Node, source: &'a [u8]) -> Option<&'a str> {
    text(string, source).map(|t| t.trim_matches(['\'', '"', '`']))
}

/// The path of `require('./a')`.
pub fn required_path<'a>(value: &Node, source: &'a [u8]) -> Option<&'a str> {
    if value.kind() != "call_expression" {
        return None;
    }
    let function = value.child_by_field_name("function")?;
    if text(&function, source)? != "require" {
        return None;
    }
    let arguments = value.child_by_field_name("arguments")?;
    let first = arguments.named_child(0)?;
    (first.kind() == "string")
        .then(|| string_content(&first, source))
        .flatten()
}

/// The names a destructuring binds.
pub fn pattern_names<'a>(pattern: &Node<'a>, out: &mut Vec<Node<'a>>, depth: usize) {
    if depth >= MAX_DEPTH {
        return;
    }
    let mut cursor = pattern.walk();
    for child in pattern.named_children(&mut cursor) {
        match child.kind() {
            "shorthand_property_identifier_pattern" | "identifier" => out.push(child),
            "pair_pattern" => {
                if let Some(value) = child.child_by_field_name("value") {
                    if value.kind() == "identifier" {
                        out.push(value);
                    } else {
                        pattern_names(&value, out, depth + 1);
                    }
                }
            }
            _ => pattern_names(&child, out, depth + 1),
        }
    }
}

/// The names a function's parameters or body bind, leaving out what nested functions bind.
pub fn collect_bound<'a>(
    node: &Node<'a>,
    source: &'a [u8],
    names: &mut HashSet<&'a str>,
    depth: usize,
) {
    if depth >= MAX_DEPTH {
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "arrow_function"
            | "function_expression"
            | "function"
            | "generator_function"
            | "method_definition"
            | "class"
            | "class_declaration" => {
                if child.kind() != "arrow_function"
                    && let Some(name) = child.child_by_field_name("name")
                    && let Some(name) = text(&name, source)
                {
                    names.insert(name);
                }
            }
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = child.child_by_field_name("name")
                    && let Some(name) = text(&name, source)
                {
                    names.insert(name);
                }
            }
            "variable_declarator" => {
                if let Some(name) = child.child_by_field_name("name") {
                    if name.kind() == "identifier" {
                        if let Some(name) = text(&name, source) {
                            names.insert(name);
                        }
                    } else {
                        let mut bound = Vec::new();
                        pattern_names(&name, &mut bound, 0);
                        names.extend(bound.iter().filter_map(|n| text(n, source)));
                    }
                }
                if let Some(value) = child.child_by_field_name("value") {
                    collect_bound(&value, source, names, depth + 1);
                }
            }
            "identifier" | "shorthand_property_identifier_pattern"
                if matches!(
                    node.kind(),
                    "formal_parameters"
                        | "required_parameter"
                        | "optional_parameter"
                        | "object_pattern"
                        | "array_pattern"
                        | "rest_pattern"
                        | "assignment_pattern"
                        | "object_assignment_pattern"
                        | "pair_pattern"
                        | "catch_clause"
                ) || node.kind() == "arrow_function" =>
            {
                if let Some(name) = text(&child, source) {
                    names.insert(name);
                }
            }
            _ => collect_bound(&child, source, names, depth + 1),
        }
    }
}
