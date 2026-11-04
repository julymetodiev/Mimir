//! Go through tree-sitter. See `docs/GO.md`, section 3.

mod references;
mod symbols;

use std::cell::RefCell;
use std::path::Path;

use mimir_parse_core::extracted::{ExtractedReference, ExtractedSymbol};
use mimir_parse_core::{LanguageAdapter, ParsedTree};
use mimir_types::{FileId, Language, QualifiedName};
use tree_sitter::Parser;

thread_local! {
    /// `None` only when tree-sitter refuses the grammar, and then parsing gives no tree.
    static GO: RefCell<Option<Parser>> = RefCell::new(parser());
}

fn parser() -> Option<Parser> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_go::LANGUAGE.into()).ok()?;
    Some(parser)
}

/// The adapter for `.go`.
#[derive(Default)]
pub struct GoAdapter;

impl LanguageAdapter for GoAdapter {
    fn language(&self) -> Language {
        Language::Go
    }

    fn parse(&self, source: &str) -> Option<ParsedTree> {
        GO.with(|parser| ParsedTree::parse(parser.borrow_mut().as_mut()?, source))
    }

    fn extract_symbols(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedSymbol> {
        symbols::extract_symbols(tree)
    }

    fn extract_references(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedReference> {
        references::extract_references(tree)
    }

    /// A qualified name starts with the package's directory: `inner::Base::Hello`.
    fn qualify(&self, path: &Path, symbols: &mut [ExtractedSymbol]) {
        let directory: Vec<String> = path
            .parent()
            .into_iter()
            .flat_map(Path::components)
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        for symbol in symbols {
            let segments = [directory.as_slice(), symbol.qualified_name.segments()].concat();
            symbol.qualified_name = QualifiedName::new(segments);
        }
    }

    fn extensions(&self) -> &[&str] {
        &["go"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extensions_are_those_of_the_language() {
        for extension in GoAdapter.extensions() {
            assert_eq!(Language::from_extension(extension), Language::Go);
        }
    }

    #[test]
    fn a_qualified_name_starts_with_the_directory() {
        let tree = GoAdapter
            .parse("package inner\ntype Base struct{}\nfunc (b Base) Hello() {}\nfunc F() {}\n")
            .unwrap();
        let mut symbols = GoAdapter.extract_symbols(&tree, FileId::new(1));
        GoAdapter.qualify(Path::new("a/inner/x.go"), &mut symbols);
        let names: Vec<String> = symbols
            .iter()
            .map(|s| s.qualified_name.join("::"))
            .collect();
        assert!(
            names.contains(&"a::inner::Base::Hello".to_string()),
            "{names:?}"
        );
        assert!(names.contains(&"a::inner::F".to_string()), "{names:?}");

        let mut at_the_top = GoAdapter.extract_symbols(&tree, FileId::new(1));
        GoAdapter.qualify(Path::new("x.go"), &mut at_the_top);
        assert!(
            at_the_top
                .iter()
                .any(|s| s.qualified_name.join("::") == "F")
        );
    }
}
