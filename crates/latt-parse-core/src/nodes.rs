//! What every parser asks of a node, and the symbol every file has.

use latt_types::{Language, Range, Span, SymbolKind};
use tree_sitter::Node;

use crate::{ExtractedSymbol, ParsedTree};

/// How deep a walk goes. A long chain, such as `a + b + c` thousands of times in generated code,
/// nests as deep as it is long, and the stack must hold.
pub const MAX_DEPTH: usize = 400;

/// The name of a file's own symbol, which no name in code can be.
pub const MODULE_NAME: &str = "<module>";

pub fn range_of(node: &Node) -> Range {
    let (start, end) = (node.start_position(), node.end_position());
    Range::new(
        start.row as u32 + 1,
        start.column as u32,
        end.row as u32 + 1,
        end.column as u32,
    )
}

pub fn span_of(node: &Node) -> Span {
    Span::new(node.start_byte() as u32, node.end_byte() as u32)
}

pub fn text<'a>(node: &Node, source: &'a [u8]) -> Option<&'a str> {
    node.utf8_text(source).ok()
}

/// What runs when a file is loaded: the caller of code outside any function. A parser is not told
/// the file's path, and the file is where a symbol is shown to be anyway.
pub fn module_symbol(tree: &ParsedTree, language: Language) -> ExtractedSymbol {
    let end = tree.root_node().end_position();
    ExtractedSymbol::new(
        MODULE_NAME.to_string(),
        SymbolKind::Module,
        language,
        Range::new(1, 0, end.row as u32 + 1, end.column as u32),
        Span::new(0, tree.source_bytes().len() as u32),
    )
}
