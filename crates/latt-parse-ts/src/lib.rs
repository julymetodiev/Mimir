//! TypeScript and JavaScript through tree-sitter. See `docs/TYPESCRIPT.md`, section 3.

mod adapter;
mod references;
mod symbols;

pub use adapter::ScriptAdapter;

use tree_sitter::Node;

/// Whether a value is a function of any form.
fn is_function(node: &Node) -> bool {
    matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "function" | "generator_function"
    )
}
