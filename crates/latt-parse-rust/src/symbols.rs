//! Symbol extraction for Rust.

use latt_parse_core::ParsedTree;
use latt_parse_core::extracted::ExtractedSymbol;
use latt_types::{FileId, Language, QualifiedName, Range, Span, SymbolKind, Visibility};
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
            extract_nested_items(node, source, symbols);
        }
        "const_item" | "static_item" => {
            if let Some(sym) = extract_constant(node, source, parent) {
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
        "union_item" => {
            if let Some(mut sym) = extract_struct(node, source, parent) {
                sym.kind = SymbolKind::Union;
                let name = sym.name.clone();
                symbols.push(sym);
                extract_struct_fields(node, source, symbols, &name);
            }
        }
        "type_item" => {
            if let Some(mut sym) = extract_struct(node, source, parent) {
                sym.kind = SymbolKind::TypeAlias;
                symbols.push(sym);
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
                let name = sym.name.clone();
                symbols.push(sym);
                extract_trait_methods(node, source, symbols, &name);
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
                    let mut cursor = body.walk();
                    for child in body.named_children(&mut cursor) {
                        extract_from_node(&child, source, symbols, Some(&name));
                    }
                }
            }
        }
        _ => {}
    }

    // Recurse into children for top-level items
    if node.kind() == "source_file" {
        // A cursor, as asking for a child by its index walks the siblings before it.
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            extract_from_node(&child, source, symbols, parent);
        }
    }
}

/// Items declared inside a function's body: nested functions, and impls with their methods.
fn extract_nested_items(function: &Node, source: &[u8], symbols: &mut Vec<ExtractedSymbol>) {
    let Some(body) = function.child_by_field_name("body") else {
        return;
    };
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        for i in (0..node.named_child_count() as u32).rev() {
            let Some(child) = node.named_child(i) else {
                continue;
            };
            match child.kind() {
                "function_item" | "impl_item" | "trait_item" | "struct_item" | "enum_item"
                | "union_item" | "type_item" | "mod_item" => {
                    extract_from_node(&child, source, symbols, None);
                }
                _ => stack.push(child),
            }
        }
    }
}

/// A trait's methods, with or without a default body.
fn extract_trait_methods(
    node: &Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    trait_name: &str,
) {
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    for i in 0..body.named_child_count() as u32 {
        let Some(child) = body.named_child(i) else {
            continue;
        };
        if matches!(child.kind(), "function_item" | "function_signature_item") {
            if let Some(mut sym) = extract_function(&child, source, Some(trait_name)) {
                sym.kind = SymbolKind::Method;
                symbols.push(sym);
            }
            extract_nested_items(&child, source, symbols);
        } else if child.kind() == "associated_type"
            && let Some(mut sym) = extract_struct(&child, source, Some(trait_name))
        {
            sym.kind = SymbolKind::TypeAlias;
            symbols.push(sym);
        }
    }
}

fn extract_function(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();

    let visibility = extract_visibility(node);
    let range = node_to_range(node);
    let span = node_to_span(node);

    let mut sym = ExtractedSymbol::new(
        name.clone(),
        SymbolKind::Function,
        Language::Rust,
        range,
        span,
    )
    .with_visibility(visibility);

    if let Some(p) = parent {
        sym = sym
            .with_parent(p.to_string())
            .with_qualified_name(QualifiedName::new(vec![p.to_string(), name]));
    }

    Some(sym)
}

fn extract_constant(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name = node
        .child_by_field_name("name")?
        .utf8_text(source)
        .ok()?
        .to_string();
    let mut sym = ExtractedSymbol::new(
        name.clone(),
        SymbolKind::Constant,
        Language::Rust,
        node_to_range(node),
        node_to_span(node),
    )
    .with_visibility(extract_visibility(node));

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

    let mut sym = ExtractedSymbol::new(
        name.clone(),
        SymbolKind::Struct,
        Language::Rust,
        range,
        span,
    )
    .with_visibility(visibility);

    if let Some(p) = parent {
        sym = sym
            .with_parent(p.to_string())
            .with_qualified_name(QualifiedName::new(vec![p.to_string(), name]));
    }

    Some(sym)
}

fn extract_struct_fields(
    node: &Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    struct_name: &str,
) {
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() as u32 {
            if let Some(field) = body.named_child(i)
                && field.kind() == "field_declaration"
                && let Some(name_node) = field.child_by_field_name("name")
                && let Ok(name) = name_node.utf8_text(source)
            {
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

fn extract_enum_variants(
    node: &Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    enum_name: &str,
) {
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() as u32 {
            if let Some(variant) = body.named_child(i)
                && variant.kind() == "enum_variant"
                && let Some(name_node) = variant.child_by_field_name("name")
                && let Ok(name) = name_node.utf8_text(source)
            {
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

fn extract_trait(node: &Node, source: &[u8], parent: Option<&str>) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();

    let visibility = extract_visibility(node);
    let range = node_to_range(node);
    let span = node_to_span(node);

    let mut sym =
        ExtractedSymbol::new(name.clone(), SymbolKind::Trait, Language::Rust, range, span)
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

    // Use "impl Type" format for easier searching
    // For trait impls: "impl Trait for Type"
    let impl_name = if let Some(trait_ref) = &trait_name {
        format!("impl {} for {}", trait_ref, name)
    } else {
        format!("impl {}", name)
    };

    let mut sym = ExtractedSymbol::new(
        impl_name.clone(),
        SymbolKind::Impl,
        Language::Rust,
        range,
        span,
    );

    if let Some(p) = parent {
        sym = sym.with_parent(p.to_string());
    }

    Some(sym)
}

fn extract_impl_methods(
    node: &Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    impl_name: &str,
) {
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() as u32 {
            if let Some(child) = body.named_child(i)
                && child.kind() == "function_item"
            {
                if let Some(mut sym) = extract_function(&child, source, Some(impl_name)) {
                    sym.kind = SymbolKind::Method;
                    symbols.push(sym);
                }
                extract_nested_items(&child, source, symbols);
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

    let mut sym = ExtractedSymbol::new(
        name.clone(),
        SymbolKind::Module,
        Language::Rust,
        range,
        span,
    )
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
        if let Some(child) = node.child(i)
            && child.kind() == "visibility_modifier"
        {
            // Check for pub(crate) by looking at child nodes
            for j in 0..child.child_count() {
                if let Some(inner) = child.child(j)
                    && inner.kind() == "crate"
                {
                    return Visibility::Internal;
                }
            }
            return Visibility::Public;
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
