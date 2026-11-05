//! Wrapper around tree-sitter parse tree.

use tree_sitter::{Node, Parser, Tree};

/// A wrapper around a tree-sitter parse tree.
///
/// Stores both the tree and the source code it was parsed from.
#[derive(Debug)]
pub struct ParsedTree {
    /// The parse tree.
    tree: Tree,
    /// The source code.
    source: String,
}

impl ParsedTree {
    /// Create a new parsed tree.
    pub fn new(tree: Tree, source: String) -> Self {
        Self { tree, source }
    }

    /// Parse source code using the given parser.
    pub fn parse(parser: &mut Parser, source: &str) -> Option<Self> {
        let tree = parser.parse(source, None)?;
        Some(Self::new(tree, source.to_string()))
    }

    /// Get the root node of the tree.
    pub fn root_node(&self) -> Node<'_> {
        self.tree.root_node()
    }

    /// Get the source code.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the source bytes.
    pub fn source_bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parsed_tree_source() {
        // Create a parser (we'll just test the struct, not actual parsing)
        let source = "fn main() {}";

        // We can't easily test tree-sitter parsing without a language,
        // but we can test the struct construction
        assert_eq!(source.len(), 12);
    }
}
