//! Where code names a type or a field. These are sites to ask about: what a name is, only a semantic
//! source knows. A site that turns out to be something else costs one question and adds nothing.

use latt_parse_core::extracted::ExtractedReference;
use latt_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

use crate::references::{is_constant_name, node_to_range, node_to_span};

/// Nodes whose `name` declares what it names, rather than using it.
const DECLARES: [&str; 13] = [
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "type_item",
    "associated_type",
    "enum_variant",
    "field_declaration",
    "function_item",
    "function_signature_item",
    "const_item",
    "static_item",
    "type_parameter",
];

/// The use of a type or field at this node, if it is one.
pub fn use_at(node: &Node, source: &[u8], container: Option<&str>) -> Option<ExtractedReference> {
    let parent = node.parent()?;
    if DECLARES.contains(&parent.kind())
        && parent
            .child_by_field_name("name")
            .is_some_and(|name| name == *node)
    {
        return None;
    }
    let name = node.utf8_text(source).ok()?;
    let (kind, context) = match node.kind() {
        "type_identifier" | "identifier"
            if name != "Self"
                && name.starts_with(char::is_uppercase)
                && (node.kind() == "type_identifier" || !is_constant_name(name)) =>
        {
            (ReferenceKind::TypeUse, qualifier(node, &parent, source))
        }
        "field_identifier" if parent.kind() == "field_expression" => {
            if is_called(&parent) {
                return None;
            }
            let value = parent.child_by_field_name("value")?;
            (
                ReferenceKind::FieldUse,
                value.utf8_text(source).ok().filter(|v| *v == "self"),
            )
        }
        "field_identifier" | "shorthand_field_identifier" => (ReferenceKind::FieldUse, None),
        "identifier" if parent.kind() == "shorthand_field_initializer" => {
            (ReferenceKind::FieldUse, None)
        }
        _ => return None,
    };
    Some(site(*node, name, kind, context, container))
}

/// The uses among a macro's tokens, which the grammar leaves unparsed: a capitalised name, a name
/// after a dot and not called, and a name before a colon.
pub fn uses_in_tokens(
    tokens: &[Node],
    source: &[u8],
    references: &mut Vec<ExtractedReference>,
    container: Option<&str>,
) {
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != "identifier" {
            continue;
        }
        let Ok(name) = token.utf8_text(source) else {
            continue;
        };
        let before = |back: usize| i.checked_sub(back).map(|at| tokens[at]);
        let next = tokens.get(i + 1);
        if name != "Self" && name.starts_with(char::is_uppercase) && !is_constant_name(name) {
            // `Kind::Object` among tokens is a variant, not the struct `Object` beside it.
            let is_scoped = before(1).is_some_and(|n| n.kind() == "::");
            let owner = before(2).filter(|_| is_scoped);
            let is_long = is_scoped && before(3).is_some_and(|n| n.kind() == "::");
            let path = owner.and_then(|n| n.utf8_text(source).ok()).map(|owner| {
                if is_long {
                    format!("::{owner}")
                } else {
                    owner.to_string()
                }
            });
            references.push(site(
                *token,
                name,
                ReferenceKind::TypeUse,
                path.as_deref(),
                container,
            ));
        } else if before(1).is_some_and(|n| n.kind() == ".") {
            let called = next.is_some_and(|n| {
                n.kind() == "token_tree" && n.child(0).is_some_and(|open| open.kind() == "(")
            });
            if !called {
                let receiver = before(2).and_then(|n| n.utf8_text(source).ok());
                let context = receiver.filter(|r| *r == "self");
                references.push(site(
                    *token,
                    name,
                    ReferenceKind::FieldUse,
                    context,
                    container,
                ));
            }
        } else if next.is_some_and(|n| n.kind() == ":") {
            references.push(site(*token, name, ReferenceKind::FieldUse, None, container));
        }
    }
}

/// The path before a name that ends one: `a::Mode` for `Fast` in `a::Mode::Fast`.
fn qualifier<'s>(node: &Node, parent: &Node, source: &'s [u8]) -> Option<&'s str> {
    if !matches!(
        parent.kind(),
        "scoped_identifier" | "scoped_type_identifier"
    ) {
        return None;
    }
    parent
        .child_by_field_name("name")
        .filter(|name| name == node)?;
    parent.child_by_field_name("path")?.utf8_text(source).ok()
}

/// Whether a field expression is the function of a call: then it is a method.
fn is_called(expression: &Node) -> bool {
    expression.parent().is_some_and(|call| {
        call.kind() == "call_expression"
            && call
                .child_by_field_name("function")
                .is_some_and(|function| function == *expression)
    })
}

fn site(
    node: Node,
    name: &str,
    kind: ReferenceKind,
    context: Option<&str>,
    container: Option<&str>,
) -> ExtractedReference {
    let mut reference = ExtractedReference::new(
        name.to_string(),
        kind,
        node_to_range(&node),
        node_to_span(&node),
    )
    .with_confidence(Confidence::Medium);
    if let Some(context) = context {
        reference = reference.with_context(context.to_string());
    }
    if let Some(container) = container {
        reference = reference.with_container(container.to_string());
    }
    reference
}
