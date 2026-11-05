//! The paths of a `use` declaration.

use latt_parse_core::extracted::ExtractedReference;
use latt_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

use crate::references::{node_to_range, node_to_span};

pub fn extract_use(node: &Node, source: &[u8], container: Option<&str>) -> Vec<ExtractedReference> {
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
            for i in 0..node.named_child_count() as u32 {
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
                let first = references.len();
                extract_use_path(&path, source, references, container, path_prefix);
                // The new name is part of the import too.
                for reference in &mut references[first..] {
                    reference.span = node_to_span(node);
                }
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
