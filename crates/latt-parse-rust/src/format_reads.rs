//! Constants named inside a format string, as in `format!("{NAME}")`. The compiler's index does not
//! report these, so they are found here. Only macros known to take a format string count, and only
//! that string: any other string may hold the same text and mean nothing by it.

use crate::references::{bound_names, is_constant_name, node_to_range, node_to_span};
use latt_parse_core::ExtractedReference;
use latt_types::{Confidence, Range, ReferenceKind, Span};
use tree_sitter::Node;

/// Where a macro's format string is.
enum Place {
    /// After this many arguments.
    After(usize),
    /// The first string that is not the value of `target:` or `field =`, as in `tracing`.
    AfterFields,
}

fn format_string_place(macro_name: &str) -> Option<Place> {
    Some(match macro_name {
        "format" | "format_args" | "print" | "println" | "eprint" | "eprintln" | "panic"
        | "todo" | "unimplemented" | "unreachable" | "bail" => Place::After(0),
        "write" | "writeln" | "assert" | "debug_assert" | "ensure" => Place::After(1),
        "assert_eq" | "assert_ne" | "debug_assert_eq" | "debug_assert_ne" => Place::After(2),
        "trace" | "debug" | "info" | "warn" | "error" => Place::AfterFields,
        _ => return None,
    })
}

/// The last segment of a macro's name: `tracing::info` is `info`.
pub fn last_segment(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

fn is_string(node: &Node) -> bool {
    matches!(node.kind(), "string_literal" | "raw_string_literal")
}

fn format_string<'tree>(place: &Place, tokens: &[Node<'tree>]) -> Option<Node<'tree>> {
    match place {
        Place::After(arguments) => {
            let mut commas = 0;
            let mut first = true;
            for token in tokens {
                if token.kind() == "," {
                    commas += 1;
                    first = true;
                } else if first && commas == *arguments {
                    return is_string(token).then_some(*token);
                } else {
                    first = false;
                }
            }
            None
        }
        Place::AfterFields => tokens.iter().enumerate().find_map(|(i, token)| {
            let value_of_a_field = i > 0 && matches!(tokens[i - 1].kind(), ":" | "=" | "%" | "?");
            (is_string(token) && !value_of_a_field).then_some(*token)
        }),
    }
}

/// The uppercase names a format string captures, with their byte offsets in its text. `{{` and an
/// escape such as `\u{41}` are not captures.
fn captures(text: &str, raw: bool) -> Vec<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' if !raw => {
                at += 2;
                if bytes.get(at - 1) == Some(&b'u') {
                    at += text[at..].find('}').map_or(0, |end| end + 1);
                }
            }
            b'{' if bytes.get(at + 1) == Some(&b'{') => at += 2,
            b'{' => {
                let start = at + 1;
                let length = text[start..]
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(0);
                let name = &text[start..start + length];
                if is_constant_name(name) && matches!(bytes.get(start + length), Some(b'}' | b':'))
                {
                    found.push((start, name));
                }
                at = start + length;
            }
            _ => at += 1,
        }
    }
    found
}

/// Whether a name passed to the macro itself, as in `format!("{NAME}", NAME = 1)`.
fn passed_by_name(tokens: &[Node], name: &str, source: &[u8]) -> bool {
    tokens.windows(2).any(|pair| {
        pair[1].kind() == "=" && pair[0].utf8_text(source).is_ok_and(|text| text == name)
    })
}

/// Whether the name means something nearer than an item: a local, a local constant, or a generic
/// constant parameter. Any such place leaves the capture out.
pub fn means_something_nearer(tree: &Node, name: &str, source: &[u8]) -> bool {
    let named = |node: Node| {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(source).ok())
            == Some(name)
    };
    let mut ancestor = tree.parent();
    while let Some(node) = ancestor {
        if node.kind() == "function_item" && bound_names(&node, source).contains(name) {
            return true;
        }
        let holds = |parent: Node, kinds: &[&str]| {
            let mut cursor = parent.walk();
            let mut children = parent.named_children(&mut cursor);
            children.any(|child| kinds.contains(&child.kind()) && named(child))
        };
        if node.kind() == "block" && holds(node, &["const_item", "static_item"]) {
            return true;
        }
        if let Some(parameters) = node.child_by_field_name("type_parameters")
            && holds(parameters, &["const_parameter"])
        {
            return true;
        }
        ancestor = node.parent();
    }
    false
}

/// A byte offset inside a literal as a line and column, which count over the literal's own lines.
fn range_at(literal: &Node, text: &str, offset: usize, length: usize) -> Range {
    let start = node_to_range(literal);
    let before = &text[..offset];
    let lines = before.matches('\n').count() as u32;
    let column = match before.rfind('\n') {
        Some(newline) => (offset - newline - 1) as u32,
        None => start.start_col + offset as u32,
    };
    let line = start.start_line + lines;
    Range {
        start_line: line,
        start_col: column,
        end_line: line,
        end_col: column + length as u32,
    }
}

/// Add a reference for each constant a macro's format string captures.
pub fn extract(
    macro_name: &str,
    tree: &Node,
    source: &[u8],
    references: &mut Vec<ExtractedReference>,
    container: Option<&str>,
) {
    let Some(place) = format_string_place(last_segment(macro_name)) else {
        return;
    };
    let mut cursor = tree.walk();
    let tokens: Vec<Node> = tree
        .children(&mut cursor)
        .filter(|token| !matches!(token.kind(), "(" | ")" | "[" | "]" | "{" | "}"))
        .collect();
    let Some(literal) = format_string(&place, &tokens) else {
        return;
    };
    let Ok(text) = literal.utf8_text(source) else {
        return;
    };
    let literal_start = node_to_span(&literal).start;
    for (offset, name) in captures(text, literal.kind() == "raw_string_literal") {
        if passed_by_name(&tokens, name, source) || means_something_nearer(tree, name, source) {
            continue;
        }
        let start = literal_start + offset as u32;
        let mut reference = ExtractedReference::new(
            name.to_string(),
            ReferenceKind::FormatRead,
            range_at(&literal, text, offset, name.len()),
            Span::new(start, start + name.len() as u32),
        )
        .with_confidence(Confidence::High);
        if let Some(c) = container {
            reference = reference.with_container(c.to_string());
        }
        references.push(reference);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_captures_only_names_in_braces() {
        let names = |text| {
            captures(text, false)
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("\"{NAME} {OTHER:>4} {} {0} {lower}\""),
            ["NAME", "OTHER"]
        );
        assert_eq!(names("\"{{NOT}} \\u{ABC} \\\\{YES}\""), ["YES"]);
        assert_eq!(names("\"{NOT.field} {MIXED_case}\""), [] as [&str; 0]);
        assert_eq!(captures("r\"\\{NAME}\"", true)[0].1, "NAME");
    }
}
