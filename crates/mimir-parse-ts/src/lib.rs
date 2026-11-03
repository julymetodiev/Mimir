//! TypeScript and JavaScript through tree-sitter. See `docs/TYPESCRIPT.md`, section 3.

mod adapter;
mod references;
mod symbols;

pub use adapter::ScriptAdapter;

use mimir_types::{Range, Span};
use tree_sitter::Node;

/// How deep a walk goes. A long chain, such as `a + b + c` thousands of times in generated code,
/// nests as deep as it is long, and the stack must hold.
const MAX_DEPTH: usize = 400;

fn range_of(node: &Node) -> Range {
    let (start, end) = (node.start_position(), node.end_position());
    Range::new(
        start.row as u32 + 1,
        start.column as u32,
        end.row as u32 + 1,
        end.column as u32,
    )
}

fn span_of(node: &Node) -> Span {
    Span::new(node.start_byte() as u32, node.end_byte() as u32)
}

fn text<'a>(node: &Node, source: &'a [u8]) -> Option<&'a str> {
    node.utf8_text(source).ok()
}

/// Whether a value is a function of any form.
fn is_function(node: &Node) -> bool {
    matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "function" | "generator_function"
    )
}
