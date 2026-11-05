//! The names a function or a comprehension binds.

use std::collections::HashSet;

use latt_parse_core::nodes::{MAX_DEPTH, text};
use tree_sitter::Node;

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
            "function_definition" | "class_definition" => {
                names.extend(
                    child
                        .child_by_field_name("name")
                        .and_then(|name| text(&name, source)),
                );
            }
            "lambda" | "comment" | "string" => {}
            "identifier"
                if matches!(
                    node.kind(),
                    "parameters"
                        | "lambda_parameters"
                        | "typed_parameter"
                        | "list_splat_pattern"
                        | "dictionary_splat_pattern"
                ) =>
            {
                names.extend(text(&child, source));
            }
            "default_parameter" | "typed_default_parameter" | "named_expression" => {
                bind(child.child_by_field_name("name"), source, names, depth);
            }
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => {
                bind(child.child_by_field_name("left"), source, names, depth);
                collect_bound(&child, source, names, depth + 1);
            }
            "as_pattern" => {
                bind(child.child_by_field_name("alias"), source, names, depth);
                collect_bound(&child, source, names, depth + 1);
            }
            "import_statement" | "import_from_statement" => imported(&child, source, names),
            _ => collect_bound(&child, source, names, depth + 1),
        }
    }
}

/// Every name in what is assigned to: not `b` of `a.b`, which is an attribute.
fn bind<'a>(
    target: Option<Node<'a>>,
    source: &'a [u8],
    names: &mut HashSet<&'a str>,
    depth: usize,
) {
    let Some(target) = target else {
        return;
    };
    if depth >= MAX_DEPTH {
        return;
    }
    match target.kind() {
        "identifier" => names.extend(text(&target, source)),
        "attribute" | "subscript" => {}
        _ => {
            let mut cursor = target.walk();
            for child in target.named_children(&mut cursor) {
                bind(Some(child), source, names, depth + 1);
            }
        }
    }
}

fn imported<'a>(node: &Node<'a>, source: &'a [u8], names: &mut HashSet<&'a str>) {
    let mut cursor = node.walk();
    for name in node.children_by_field_name("name", &mut cursor) {
        let bound = if name.kind() == "aliased_import" {
            name.child_by_field_name("alias")
        } else {
            name.named_child(0)
        };
        names.extend(bound.and_then(|bound| text(&bound, source)));
    }
}
