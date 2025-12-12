//! Symbol extraction for Rust.

use mimir_parse_core::extracted::ExtractedSymbol;
use mimir_parse_core::ParsedTree;
use mimir_types::{FileId, Language, QualifiedName, Range, Span, SymbolKind, Visibility};
use tree_sitter::Node;

/// Extract symbols from a Rust parse tree.
pub fn extract_symbols(tree: &ParsedTree, _file_id: FileId) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let source = tree.source_bytes();
    let root = tree.root_node();

    extract_from_node(&root, source, &mut symbols, None);

    symbols
}

fn extract_from_node(
    node: &Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    parent: Option<&str>,
) {
    match node.kind() {
        "function_item" => {
            if let Some(sym) = extract_function(node, source, parent) {
                symbols.push(sym);
            }
        }
        "struct_item" => {
            if let Some(sym) = extract_struct(node, source, parent) {
                let name = sym.name.clone();
                symbols.push(sym);
                // Extract fields
                extract_struct_fields(node, source, symbols, &name);
            }
        }
        "enum_item" => {
            if let Some(sym) = extract_enum(node, source, parent) {
                let name = sym.name.clone();
                symbols.push(sym);
                // Extract variants
                extract_enum_variants(node, source, symbols, &name);
            }
        }
        "trait_item" => {
            if let Some(sym) = extract_trait(node, source, parent) {
                symbols.push(sym);
            }
        }
        "impl_item" => {
            if let Some(sym) = extract_impl(node, source, parent) {
                let name = sym.name.clone();
                symbols.push(sym);
                // Extract methods in impl
                extract_impl_methods(node, source, symbols, &name);
            }
        }
        "mod_item" => {
            if let Some(sym) = extract_module(node, source, parent) {
                let name = sym.name.clone();
                symbols.push(sym);
                // Recurse into module body
                if let Some(body) = node.child_by_field_name("body") {
                    for i in 0..body.named_child_count() {
                        if let Some(child) = body.named_child(i) {
                            extract_from_node(&child, source, symbols, Some(&name));
                        }
                    }
                }
            }
        }
        _ => {}
    }

    // Recurse into children for top-level items
    if node.kind() == "source_file" {
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                extract_from_node(&child, source, symbols, parent);
            }
        }
    }
}

fn extract_function(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();

    let visibility = extract_visibility(node);
    let range = node_to_range(node);
    let span = node_to_span(node);

    let mut sym = ExtractedSymbol::new(name.clone(), SymbolKind::Function, Language::Rust, range, span)
        .with_visibility(visibility);

    if let Some(p) = parent {
        sym = sym
            .with_parent(p.to_string())
            .with_qualified_name(QualifiedName::new(vec![p.to_string(), name]));
    }

    Some(sym)
}

fn extract_struct(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();

    let visibility = extract_visibility(node);
    let range = node_to_range(node);
    let span = node_to_span(node);

    let mut sym = ExtractedSymbol::new(name.clone(), SymbolKind::Struct, Language::Rust, range, span)
        .with_visibility(visibility);

    if let Some(p) = parent {
        sym = sym
            .with_parent(p.to_string())
            .with_qualified_name(QualifiedName::new(vec![p.to_string(), name]));
    }

    Some(sym)
}

fn extract_struct_fields(node: &Node, source: &[u8], symbols: &mut Vec<ExtractedSymbol>, struct_name: &str) {
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() {
            if let Some(field) = body.named_child(i) {
                if field.kind() == "field_declaration" {
                    if let Some(name_node) = field.child_by_field_name("name") {
                        if let Ok(name) = name_node.utf8_text(source) {
                            let range = node_to_range(&field);
                            let span = node_to_span(&field);
                            let visibility = extract_visibility(&field);

                            let sym = ExtractedSymbol::new(
                                name.to_string(),
                                SymbolKind::Field,
                                Language::Rust,
                                range,
                                span,
                            )
                            .with_visibility(visibility)
                            .with_parent(struct_name.to_string())
                            .with_qualified_name(QualifiedName::new(vec![
                                struct_name.to_string(),
                                name.to_string(),
                            ]));

                            symbols.push(sym);
                        }
                    }
                }
            }
        }
    }
}

fn extract_enum(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();

    let visibility = extract_visibility(node);
    let range = node_to_range(node);
    let span = node_to_span(node);

    let mut sym = ExtractedSymbol::new(name.clone(), SymbolKind::Enum, Language::Rust, range, span)
        .with_visibility(visibility);

    if let Some(p) = parent {
        sym = sym
            .with_parent(p.to_string())
            .with_qualified_name(QualifiedName::new(vec![p.to_string(), name]));
    }

    Some(sym)
}

fn extract_enum_variants(node: &Node, source: &[u8], symbols: &mut Vec<ExtractedSymbol>, enum_name: &str) {
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() {
            if let Some(variant) = body.named_child(i) {
                if variant.kind() == "enum_variant" {
                    if let Some(name_node) = variant.child_by_field_name("name") {
                        if let Ok(name) = name_node.utf8_text(source) {
                            let range = node_to_range(&variant);
                            let span = node_to_span(&variant);

                            let sym = ExtractedSymbol::new(
                                name.to_string(),
                                SymbolKind::EnumVariant,
                                Language::Rust,
                                range,
                                span,
                            )
                            .with_parent(enum_name.to_string())
                            .with_qualified_name(QualifiedName::new(vec![
                                enum_name.to_string(),
                                name.to_string(),
                            ]));

                            symbols.push(sym);
                        }
                    }
                }
            }
        }
    }
}

fn extract_trait(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();

    let visibility = extract_visibility(node);
    let range = node_to_range(node);
    let span = node_to_span(node);

    let mut sym = ExtractedSymbol::new(name.clone(), SymbolKind::Trait, Language::Rust, range, span)
        .with_visibility(visibility);

    if let Some(p) = parent {
        sym = sym
            .with_parent(p.to_string())
            .with_qualified_name(QualifiedName::new(vec![p.to_string(), name]));
    }

    Some(sym)
}

fn extract_impl(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let type_node = node.child_by_field_name("type")?;
    let name = type_node.utf8_text(source).ok()?.to_string();

    let range = node_to_range(node);
    let span = node_to_span(node);

    // Check if this is a trait impl
    let trait_name = node
        .child_by_field_name("trait")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string());

    let impl_name = if let Some(trait_ref) = &trait_name {
        format!("<{} as {}>", name, trait_ref)
    } else {
        format!("<{}>", name)
    };

    let mut sym = ExtractedSymbol::new(impl_name.clone(), SymbolKind::Impl, Language::Rust, range, span);

    if let Some(p) = parent {
        sym = sym.with_parent(p.to_string());
    }

    Some(sym)
}

fn extract_impl_methods(node: &Node, source: &[u8], symbols: &mut Vec<ExtractedSymbol>, impl_name: &str) {
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() {
            if let Some(child) = body.named_child(i) {
                if child.kind() == "function_item" {
                    if let Some(mut sym) = extract_function(&child, source, Some(impl_name)) {
                        sym.kind = SymbolKind::Method;
                        symbols.push(sym);
                    }
                }
            }
        }
    }
}

fn extract_module(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();

    let visibility = extract_visibility(node);
    let range = node_to_range(node);
    let span = node_to_span(node);

    let mut sym = ExtractedSymbol::new(name.clone(), SymbolKind::Module, Language::Rust, range, span)
        .with_visibility(visibility);

    if let Some(p) = parent {
        sym = sym
            .with_parent(p.to_string())
            .with_qualified_name(QualifiedName::new(vec![p.to_string(), name]));
    }

    Some(sym)
}

fn has_visibility_modifier(node: &Node) -> Visibility {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "visibility_modifier" {
                // Check for pub(crate) by looking at child nodes
                for j in 0..child.child_count() {
                    if let Some(inner) = child.child(j) {
                        if inner.kind() == "crate" {
                            return Visibility::Internal;
                        }
                    }
                }
                return Visibility::Public;
            }
        }
    }
    Visibility::Private
}

fn extract_visibility(node: &Node) -> Visibility {
    has_visibility_modifier(node)
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
