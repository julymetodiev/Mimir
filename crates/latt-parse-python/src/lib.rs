//! Python through tree-sitter. See `docs/PYTHON.md`, section 3.

mod references;
mod symbols;

use std::cell::RefCell;

use latt_parse_core::extracted::{ExtractedReference, ExtractedSymbol};
use latt_parse_core::{LanguageAdapter, ParsedTree};
use latt_types::{FileId, Language};
use tree_sitter::Parser;

thread_local! {
    /// `None` only when tree-sitter refuses the grammar, and then parsing gives no tree.
    static PYTHON: RefCell<Option<Parser>> = RefCell::new(parser());
}

fn parser() -> Option<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .ok()?;
    Some(parser)
}

/// The adapter for `.py`, `.pyi` and `.pyw`.
#[derive(Default)]
pub struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn language(&self) -> Language {
        Language::Python
    }

    fn parse(&self, source: &str) -> Option<ParsedTree> {
        PYTHON.with(|parser| ParsedTree::parse(parser.borrow_mut().as_mut()?, source))
    }

    fn extract_symbols(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedSymbol> {
        symbols::extract_symbols(tree)
    }

    fn extract_references(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedReference> {
        references::extract_references(tree)
    }

    fn extensions(&self) -> &[&str] {
        &["py", "pyi", "pyw"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extensions_are_those_of_the_language() {
        for extension in PythonAdapter.extensions() {
            assert_eq!(Language::from_extension(extension), Language::Python);
        }
    }
}
