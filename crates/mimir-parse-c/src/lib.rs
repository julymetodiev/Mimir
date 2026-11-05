//! C and C++ through tree-sitter. See `docs/CPP.md`, section 3.

mod declarators;
mod macros;
mod references;
mod symbols;

use std::cell::RefCell;

use mimir_parse_core::extracted::{ExtractedReference, ExtractedSymbol};
use mimir_parse_core::{LanguageAdapter, ParsedTree};
use mimir_types::{FileId, Language};
use tree_sitter::Parser;

thread_local! {
    /// `None` only when tree-sitter refuses a grammar, and then parsing gives no tree.
    static C: RefCell<Option<Parser>> = RefCell::new(parser(&tree_sitter_c::LANGUAGE.into()));
    static CPP: RefCell<Option<Parser>> = RefCell::new(parser(&tree_sitter_cpp::LANGUAGE.into()));
}

fn parser(language: &tree_sitter::Language) -> Option<Parser> {
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    Some(parser)
}

/// The adapter for C or for C++. The grammars share the names of what both have, so one walk reads
/// both. A `.h` is read as C++: it may hold either.
pub struct CFamilyAdapter {
    language: Language,
    extensions: &'static [&'static str],
}

impl CFamilyAdapter {
    pub fn c() -> Self {
        Self {
            language: Language::C,
            extensions: &["c"],
        }
    }

    pub fn cpp() -> Self {
        Self {
            language: Language::Cpp,
            extensions: &["cpp", "cc", "cxx", "h", "hpp", "hh", "hxx"],
        }
    }
}

impl LanguageAdapter for CFamilyAdapter {
    fn language(&self) -> Language {
        self.language
    }

    fn parse(&self, source: &str) -> Option<ParsedTree> {
        let blanked = macros::blanked(source);
        let source = blanked.as_deref().unwrap_or(source);
        let parse = |parser: &RefCell<Option<Parser>>| {
            ParsedTree::parse(parser.borrow_mut().as_mut()?, source)
        };
        match self.language {
            Language::C => C.with(parse),
            _ => CPP.with(parse),
        }
    }

    fn extract_symbols(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedSymbol> {
        symbols::extract_symbols(tree, self.language)
    }

    fn extract_references(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedReference> {
        references::extract_references(tree)
    }

    fn extensions(&self) -> &[&str] {
        self.extensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extensions_are_those_of_the_language() {
        for adapter in [CFamilyAdapter::c(), CFamilyAdapter::cpp()] {
            for extension in adapter.extensions() {
                assert_eq!(Language::from_extension(extension), adapter.language());
            }
        }
    }

    #[test]
    fn both_grammars_load() {
        assert!(CFamilyAdapter::c().parse("int f(void);").is_some());
        assert!(CFamilyAdapter::cpp().parse("class A {};").is_some());
    }
}
