//! Reference extraction for Rust.

use std::collections::HashSet;

use mimir_parse_core::ParsedTree;
use mimir_parse_core::extracted::{CallForm, ExtractedReference};
use mimir_types::{Confidence, FileId, Range, ReferenceKind, Span};
use tree_sitter::Node;

use crate::{format_reads, imports, uses};

/// Extract references from a Rust parse tree.
pub fn extract_references(tree: &ParsedTree, _file_id: FileId) -> Vec<ExtractedReference> {
    let mut references = Vec::new();
    let source = tree.source_bytes();
    let root = tree.root_node();

    extract_from_node(&root, source, &mut references, None, &HashSet::new());

    references
}

fn extract_from_node(
    node: &Node,
    source: &[u8],
    references: &mut Vec<ExtractedReference>,
    container: Option<&str>,
    bound: &HashSet<String>,
) {
    let mut container = container;
    let mut bound = bound;
    let name;
    let bound_here;
    match node.kind() {
        "call_expression" => {
            if let Some(reference) = extract_call(node, source, container) {
                references.push(reference);
            }
        }
        "use_declaration" => {
            references.extend(imports::extract_use(node, source, container));
            return;
        }
        "function_item" => {
            // Track function as container for nested references
            name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok());
            container = name;
            bound_here = bound_names(node, source);
            bound = &bound_here;
        }
        "identifier" | "scoped_identifier" | "generic_function" => {
            if let Some(reference) = extract_function_value(node, source, container, bound) {
                references.push(reference);
            }
            if let Some(reference) = extract_constant_read(node, source, container) {
                references.push(reference);
            }
            references.extend(uses::use_at(node, source, container));
        }
        "type_identifier" | "field_identifier" | "shorthand_field_identifier" => {
            references.extend(uses::use_at(node, source, container));
        }
        // Tokens that are not code of this function: a macro's own rules, and attributes.
        "macro_definition" | "attribute_item" | "inner_attribute_item" => return,
        "macro_invocation" => {
            let name = node
                .child_by_field_name("macro")
                .and_then(|n| n.utf8_text(source).ok());
            let mut cursor = node.walk();
            let tree = node
                .children(&mut cursor)
                .find(|n| n.kind() == "token_tree");
            if let (Some(name), Some(tree)) = (name, tree) {
                format_reads::extract(name, &tree, source, references, container);
            }
        }
        "token_tree" => {
            extract_from_tokens(node, source, references, container);
            return;
        }
        _ => {}
    }

    // A cursor, as asking for a child by its index walks the siblings before it.
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        extract_from_node(&child, source, references, container, bound);
    }
}

fn extract_call(node: &Node, source: &[u8], container: Option<&str>) -> Option<ExtractedReference> {
    let mut func = node.child_by_field_name("function")?;
    // `name::<T>()` is a call of `name`.
    if func.kind() == "generic_function" {
        func = func.child_by_field_name("function")?;
    }

    let (name_node, qualifier, form, confidence) = match func.kind() {
        "identifier" => (func, None, CallForm::Plain, Confidence::Medium),
        "field_expression" => (
            func.child_by_field_name("field")?,
            func.child_by_field_name("value"),
            CallForm::Method,
            // Low confidence since we don't know the type
            Confidence::Low,
        ),
        "scoped_identifier" => (
            func.child_by_field_name("name")?,
            func.child_by_field_name("path"),
            CallForm::Scoped,
            Confidence::High,
        ),
        _ => return None,
    };

    let name = name_node.utf8_text(source).ok()?.to_string();
    let qualifier = qualifier.and_then(|q| q.utf8_text(source).ok());
    Some(call_reference(
        ReferenceKind::Call,
        name,
        name_node,
        form,
        confidence,
        qualifier,
        container,
    ))
}

/// A function named without being called, as in `.map(name)` or `.map(Type::name)`.
/// A local variable looks the same, so a name bound in the function does not count.
fn extract_function_value(
    node: &Node,
    source: &[u8],
    container: Option<&str>,
    bound: &HashSet<String>,
) -> Option<ExtractedReference> {
    if !in_value_position(node) {
        return None;
    }
    let path = if node.kind() == "generic_function" {
        node.child_by_field_name("function")?
    } else {
        *node
    };
    let (name_node, qualifier, form, confidence) = match path.kind() {
        "identifier"
            if node
                .parent()
                .is_some_and(|p| p.kind() == "shorthand_field_initializer") =>
        {
            (path, None, CallForm::Shorthand, Confidence::Medium)
        }
        "identifier" => (path, None, CallForm::Plain, Confidence::Medium),
        "scoped_identifier" => (
            path.child_by_field_name("name")?,
            path.child_by_field_name("path"),
            CallForm::Scoped,
            Confidence::High,
        ),
        _ => return None,
    };
    let name = name_node.utf8_text(source).ok()?;
    // Uppercase is a constant, a variant or a type.
    if name.starts_with(char::is_uppercase) {
        return None;
    }
    let qualifier = qualifier.and_then(|q| q.utf8_text(source).ok());
    let kind = if form != CallForm::Scoped && bound.contains(name) {
        ReferenceKind::LocalRead
    } else {
        ReferenceKind::FunctionValue
    };
    Some(call_reference(
        kind,
        name.to_string(),
        name_node,
        form,
        confidence,
        qualifier,
        container,
    ))
}

/// A use of a constant or static, by its `SCREAMING_CASE` name: it may hold a function.
fn extract_constant_read(
    node: &Node,
    source: &[u8],
    container: Option<&str>,
) -> Option<ExtractedReference> {
    let parent = node.parent()?;
    let name_node = match node.kind() {
        "identifier" => {
            let is_segment = parent.kind() == "scoped_identifier";
            let is_declared = parent
                .child_by_field_name("name")
                .is_some_and(|name| name == *node);
            if is_segment || is_declared {
                return None;
            }
            *node
        }
        "scoped_identifier" => {
            let path = node.child_by_field_name("path")?.utf8_text(source).ok()?;
            let name = node.child_by_field_name("name")?;
            return Some(constant_read(name, source, container)?.with_context(path.to_string()));
        }
        _ => return None,
    };
    constant_read(name_node, source, container)
}

fn constant_read(
    name_node: Node,
    source: &[u8],
    container: Option<&str>,
) -> Option<ExtractedReference> {
    let name = name_node.utf8_text(source).ok()?;
    if !is_constant_name(name) {
        return None;
    }
    // With no path the name may be a local, and a static in a function is no symbol of the graph.
    let has_path = name_node
        .parent()
        .is_some_and(|parent| parent.kind() == "scoped_identifier")
        || name_node
            .prev_sibling()
            .is_some_and(|before| before.kind() == "::");
    if !has_path && format_reads::means_something_nearer(&name_node, name, source) {
        return None;
    }
    let mut reference = ExtractedReference::new(
        name.to_string(),
        ReferenceKind::Read,
        node_to_range(&name_node),
        node_to_span(&name_node),
    )
    .with_confidence(Confidence::Medium);
    if let Some(c) = container {
        reference = reference.with_container(c.to_string());
    }
    Some(reference)
}

pub fn is_constant_name(name: &str) -> bool {
    name.chars().any(char::is_uppercase) && !name.chars().any(char::is_lowercase)
}

/// Whether an expression is used for its value, rather than being a path's segment,
/// the function of a call, or a pattern.
fn in_value_position(node: &Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let field = match parent.kind() {
        // Only a block's tail is a bare expression; a statement is wrapped.
        "arguments"
        | "array_expression"
        | "tuple_expression"
        | "return_expression"
        | "shorthand_field_initializer"
        | "block" => return true,
        "let_declaration"
        | "const_item"
        | "static_item"
        | "field_initializer"
        | "reference_expression"
        | "match_arm"
        | "type_cast_expression" => "value",
        "assignment_expression" => "right",
        "closure_expression" => "body",
        _ => return false,
    };
    parent
        .child_by_field_name(field)
        .is_some_and(|value| value == *node)
}

/// Every name a function binds: parameters, `let`, `for`, closures and match arms.
/// Scopes are ignored, which can only leave a function value out, never add a wrong one.
pub fn bound_names(function: &Node, source: &[u8]) -> HashSet<String> {
    fn collect(node: &Node, source: &[u8], names: &mut HashSet<String>) {
        if matches!(node.kind(), "identifier" | "shorthand_field_identifier")
            && let Ok(name) = node.utf8_text(source)
        {
            names.insert(name.to_string());
        }
        for i in 0..node.named_child_count() as u32 {
            if let Some(child) = node.named_child(i) {
                collect(&child, source, names);
            }
        }
    }
    fn walk(node: &Node, source: &[u8], names: &mut HashSet<String>) {
        if node.kind() == "closure_parameters" {
            collect(node, source, names);
        } else if let Some(pattern) = node.child_by_field_name("pattern") {
            collect(&pattern, source, names);
        }
        for i in 0..node.named_child_count() as u32 {
            if let Some(child) = node.named_child(i).filter(|c| c.kind() != "function_item") {
                walk(&child, source, names);
            }
        }
    }
    let mut names = HashSet::new();
    walk(function, source, &mut names);
    names
}

/// Calls among a macro's tokens, which the grammar leaves unparsed: an identifier followed
/// by a parenthesised group, with what comes before it deciding the form.
fn extract_from_tokens(
    node: &Node,
    source: &[u8],
    references: &mut Vec<ExtractedReference>,
    container: Option<&str>,
) {
    let tokens: Vec<Node> = (0..node.child_count())
        .filter_map(|i| node.child(i))
        .collect();
    uses::uses_in_tokens(&tokens, source, references, container);
    for (i, token) in tokens.iter().enumerate() {
        // `NAME = 1` among a macro's tokens is a name given to a value, not a read of `NAME`.
        let named_here = tokens.get(i + 1).is_some_and(|next| next.kind() == "=");
        if token.kind() == "identifier"
            && !named_here
            && let Some(reference) = constant_read(*token, source, container)
        {
            references.push(reference);
        }
        if token.kind() != "token_tree" {
            continue;
        }
        extract_from_tokens(token, source, references, container);
        if let [.., name, bang] = &tokens[..i]
            && bang.kind() == "!"
            && let Ok(name) = name.utf8_text(source)
        {
            format_reads::extract(name, token, source, references, container);
        }

        let opens_with_paren = token.child(0).is_some_and(|c| c.kind() == "(");
        let Some(name_node) = i.checked_sub(1).map(|j| tokens[j]) else {
            continue;
        };
        if !opens_with_paren || name_node.kind() != "identifier" {
            continue;
        }
        let Ok(name) = name_node.utf8_text(source) else {
            continue;
        };
        let before = |back: usize| i.checked_sub(back).map(|j| tokens[j]);
        let qualifier = before(3).and_then(|n| n.utf8_text(source).ok());
        let (form, confidence, qualifier) = match before(2).map(|n| n.kind()) {
            Some(".") => (CallForm::Method, Confidence::Low, qualifier),
            Some("::") => (CallForm::Scoped, Confidence::High, qualifier),
            _ => (CallForm::Plain, Confidence::Medium, None),
        };
        references.push(call_reference(
            ReferenceKind::Call,
            name.to_string(),
            name_node,
            form,
            confidence,
            qualifier,
            container,
        ));
    }
}

fn call_reference(
    kind: ReferenceKind,
    name: String,
    name_node: Node,
    form: CallForm,
    confidence: Confidence,
    qualifier: Option<&str>,
    container: Option<&str>,
) -> ExtractedReference {
    let mut reference = ExtractedReference::new(
        name,
        kind,
        node_to_range(&name_node),
        node_to_span(&name_node),
    )
    .with_confidence(confidence)
    .with_call_form(form);

    if let Some(q) = qualifier {
        reference = reference.with_context(q.to_string());
    }
    if let Some(c) = container {
        reference = reference.with_container(c.to_string());
    }
    reference
}

pub fn node_to_range(node: &Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range::new(
        start.row as u32 + 1,
        start.column as u32,
        end.row as u32 + 1,
        end.column as u32,
    )
}

pub fn node_to_span(node: &Node) -> Span {
    Span::new(node.start_byte() as u32, node.end_byte() as u32)
}
