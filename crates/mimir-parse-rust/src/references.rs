//! Reference extraction for Rust.

use mimir_parse_core::extracted::ExtractedReference;
use mimir_parse_core::ParsedTree;
use mimir_types::{Confidence, FileId, Range, ReferenceKind, Span};
use tree_sitter::Node;

/// Extract references from a Rust parse tree.
pub fn extract_references(tree: &ParsedTree, _file_id: FileId) -> Vec<ExtractedReference> {
    let mut references = Vec::new();
    let source = tree.source_bytes();
    let root = tree.root_node();

    extract_from_node(&root, source, &mut references, None);

    references
}

fn extract_from_node(
    node: &Node,
    source: &[u8],
    references: &mut Vec<ExtractedReference>,
    container: Option<&str>,
) {
    match node.kind() {
        "call_expression" => {
            if let Some(reference) = extract_call(node, source, container) {
                references.push(reference);
            }
        }
        "use_declaration" => {
            let imports = extract_use(node, source, container);
            references.extend(imports);
        }
        "function_item" => {
            // Track function as container for nested references
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(|s| s.to_string());

            if let Some(body) = node.child_by_field_name("body") {
                extract_from_node(&body, source, references, name.as_deref());
            }
        }
        _ => {}
    }

    // Recurse into children
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            extract_from_node(&child, source, references, container);
        }
    }
}

fn extract_call(node: &Node, source: &[u8], container: Option<&str>) -> Option<ExtractedReference> {
    let func = node.child_by_field_name("function")?;

    let (name, confidence) = match func.kind() {
        "identifier" => {
            let name = func.utf8_text(source).ok()?.to_string();
            (name, Confidence::Medium)
        }
        "field_expression" => {
            // Method call: obj.method()
            let field = func.child_by_field_name("field")?;
            let name = field.utf8_text(source).ok()?.to_string();
            (name, Confidence::Low) // Low confidence since we don't know the type
        }
        "scoped_identifier" => {
            // Qualified call: Foo::bar()
            let name_node = func.child_by_field_name("name")?;
            let name = name_node.utf8_text(source).ok()?.to_string();

            // Get the path for context
            let path = func.child_by_field_name("path")
                .and_then(|p| p.utf8_text(source).ok())
                .map(|s| s.to_string());

            let mut reference = ExtractedReference::new(
                name,
                ReferenceKind::Call,
                node_to_range(node),
                node_to_span(node),
            )
            .with_confidence(Confidence::High);

            if let Some(path) = path {
                reference = reference.with_context(path);
            }
            if let Some(c) = container {
                reference = reference.with_container(c.to_string());
            }

            return Some(reference);
        }
        _ => return None,
    };

    let mut reference = ExtractedReference::new(
        name,
        ReferenceKind::Call,
        node_to_range(node),
        node_to_span(node),
    )
    .with_confidence(confidence);

    if let Some(c) = container {
        reference = reference.with_container(c.to_string());
    }

    Some(reference)
}

fn extract_use(node: &Node, source: &[u8], container: Option<&str>) -> Vec<ExtractedReference> {
    let mut references = Vec::new();

    if let Some(arg) = node.child_by_field_name("argument") {
        extract_use_path(&arg, source, &mut references, container, None);
    }

    references
}

fn extract_use_path(
    node: &Node,
    source: &[u8],
    references: &mut Vec<ExtractedReference>,
    container: Option<&str>,
    path_prefix: Option<&str>,
) {
    match node.kind() {
        "identifier" => {
            let name = node.utf8_text(source).unwrap_or("").to_string();
            if !name.is_empty() {
                let full_path = if let Some(prefix) = path_prefix {
                    format!("{}::{}", prefix, name)
                } else {
                    name.clone()
                };

                let mut reference = ExtractedReference::new(
                    name,
                    ReferenceKind::Import,
                    node_to_range(node),
                    node_to_span(node),
                )
                .with_confidence(Confidence::Certain)
                .with_context(full_path);

                if let Some(c) = container {
                    reference = reference.with_container(c.to_string());
                }

                references.push(reference);
            }
        }
        "scoped_identifier" => {
            // Get the full path
            let path_text = node.utf8_text(source).unwrap_or("");
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .unwrap_or("");

            if !name.is_empty() {
                let mut reference = ExtractedReference::new(
                    name.to_string(),
                    ReferenceKind::Import,
                    node_to_range(node),
                    node_to_span(node),
                )
                .with_confidence(Confidence::Certain)
                .with_context(path_text.to_string());

                if let Some(c) = container {
                    reference = reference.with_container(c.to_string());
                }

                references.push(reference);
            }
        }
        "use_list" => {
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    extract_use_path(&child, source, references, container, path_prefix);
                }
            }
        }
        "scoped_use_list" => {
            let path = node
                .child_by_field_name("path")
                .and_then(|p| p.utf8_text(source).ok());

            let new_prefix = match (path_prefix, path) {
                (Some(prefix), Some(p)) => Some(format!("{}::{}", prefix, p)),
                (None, Some(p)) => Some(p.to_string()),
                (Some(prefix), None) => Some(prefix.to_string()),
                (None, None) => None,
            };

            if let Some(list) = node.child_by_field_name("list") {
                extract_use_path(&list, source, references, container, new_prefix.as_deref());
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                extract_use_path(&path, source, references, container, path_prefix);
            }
        }
        "use_wildcard" => {
            // For `use foo::*`, we just note the wildcard import
            let path = path_prefix.unwrap_or("*");
            let reference = ExtractedReference::new(
                "*".to_string(),
                ReferenceKind::Import,
                node_to_range(node),
                node_to_span(node),
            )
            .with_confidence(Confidence::Low)
            .with_context(format!("{}::*", path));

            references.push(reference);
        }
        _ => {}
    }
}

fn node_to_range(node: &Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range::new(
        start.row as u32 + 1,
        start.column as u32,
        end.row as u32 + 1,
        end.column as u32,
    )
}

fn node_to_span(node: &Node) -> Span {
    Span::new(node.start_byte() as u32, node.end_byte() as u32)
}
