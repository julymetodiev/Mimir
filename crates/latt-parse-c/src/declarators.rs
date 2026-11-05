//! What a declarator names, under what the grammar wraps around a name.

use latt_parse_core::nodes::MAX_DEPTH;
use tree_sitter::Node;

/// A declarator without what wraps its name: `*x`, `&x`, `(x)`, `x[1]`, `x = 1`.
pub fn declared(node: Node<'_>, depth: usize) -> Node<'_> {
    if depth >= MAX_DEPTH {
        return node;
    }
    match node.kind() {
        "pointer_declarator"
        | "reference_declarator"
        | "parenthesized_declarator"
        | "array_declarator"
        | "attributed_declarator"
        | "init_declarator" => inner(node).map_or(node, |inner| declared(inner, depth + 1)),
        _ => node,
    }
}

/// The name of a declarator, a function's too.
pub fn named_by(node: Node<'_>, depth: usize) -> Node<'_> {
    if depth >= MAX_DEPTH {
        return node;
    }
    let at = declared(node, depth);
    if at.kind() == "function_declarator" {
        return inner(at).map_or(at, |inner| named_by(inner, depth + 1));
    }
    at
}

/// A reference declarator gives what it wraps no field, so it is its last child.
fn inner(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("declarator").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).last()
    })
}

/// The function declarator inside a declarator, if it declares a function and not a pointer to one.
pub fn function_declarator(node: Node<'_>, depth: usize) -> Option<Node<'_>> {
    if depth >= MAX_DEPTH {
        return None;
    }
    match node.kind() {
        // `int (*f)(void)` is a variable, and in `int (*f(void))(void)` the function is the one inside.
        "function_declarator" => {
            let around = inner(node)?;
            if around.kind() == "parenthesized_declarator" {
                function_declarator(declared(around, depth + 1), depth + 1)
            } else {
                Some(node)
            }
        }
        "pointer_declarator" | "reference_declarator" | "attributed_declarator" => {
            function_declarator(inner(node)?, depth + 1)
        }
        _ => None,
    }
}

/// The last name of `a::b::c`.
pub fn last_name(node: Node<'_>, depth: usize) -> Option<Node<'_>> {
    if depth >= MAX_DEPTH {
        return None;
    }
    let name = node.child_by_field_name("name")?;
    if name.kind() == "qualified_identifier" {
        last_name(name, depth + 1)
    } else {
        Some(name)
    }
}
