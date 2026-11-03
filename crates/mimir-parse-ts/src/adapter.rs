//! The adapters: one grammar each, since `.tsx` and `.ts` read `<T>x` differently.

use std::cell::RefCell;

use mimir_parse_core::extracted::{ExtractedReference, ExtractedSymbol};
use mimir_parse_core::{LanguageAdapter, ParsedTree};
use mimir_types::{FileId, Language};
use tree_sitter::Parser;

use crate::references::extract_references;
use crate::symbols::extract_symbols;

thread_local! {
    /// `None` only when tree-sitter refuses a grammar, and then parsing gives no tree.
    static TYPESCRIPT: RefCell<Option<Parser>> =
        RefCell::new(parser(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()));
    static TSX: RefCell<Option<Parser>> =
        RefCell::new(parser(tree_sitter_typescript::LANGUAGE_TSX.into()));
    static JAVASCRIPT: RefCell<Option<Parser>> =
        RefCell::new(parser(tree_sitter_javascript::LANGUAGE.into()));
}

fn parser(language: tree_sitter::Language) -> Option<Parser> {
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    Some(parser)
}

/// One of the three grammars of TypeScript and JavaScript.
pub struct ScriptAdapter {
    language: Language,
}

impl ScriptAdapter {
    /// `.ts`, `.mts` and `.cts`.
    pub fn typescript() -> Self {
        Self {
            language: Language::TypeScript,
        }
    }

    /// `.tsx`.
    pub fn tsx() -> Self {
        Self {
            language: Language::Tsx,
        }
    }

    /// `.js`, `.jsx`, `.mjs` and `.cjs`.
    pub fn javascript() -> Self {
        Self {
            language: Language::JavaScript,
        }
    }
}

impl LanguageAdapter for ScriptAdapter {
    fn language(&self) -> Language {
        self.language
    }

    fn parse(&self, source: &str) -> Option<ParsedTree> {
        let with = |parser: &RefCell<Option<Parser>>| {
            ParsedTree::parse(parser.borrow_mut().as_mut()?, source)
        };
        match self.language {
            Language::Tsx => TSX.with(with),
            Language::JavaScript => JAVASCRIPT.with(with),
            _ => TYPESCRIPT.with(with),
        }
    }

    fn extract_symbols(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedSymbol> {
        extract_symbols(tree, self.language)
    }

    fn extract_references(&self, tree: &ParsedTree, _: FileId) -> Vec<ExtractedReference> {
        extract_references(tree)
    }

    fn extensions(&self) -> &[&str] {
        match self.language {
            Language::Tsx => &["tsx"],
            Language::JavaScript => &["js", "jsx", "mjs", "cjs"],
            _ => &["ts", "mts", "cts"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_grammar_reads_what_only_it_can() {
        let jsx = "const a = <div />;\n";
        let assertion = "const a = <T>b;\n";
        let clean = |adapter: &ScriptAdapter, source: &str| {
            !adapter.parse(source).unwrap().root_node().has_error()
        };
        assert!(clean(&ScriptAdapter::tsx(), jsx));
        assert!(clean(&ScriptAdapter::javascript(), jsx));
        assert!(clean(&ScriptAdapter::typescript(), assertion));
        assert!(!clean(&ScriptAdapter::typescript(), jsx));
    }

    #[test]
    fn the_extensions_are_those_of_the_language() {
        for adapter in [
            ScriptAdapter::typescript(),
            ScriptAdapter::tsx(),
            ScriptAdapter::javascript(),
        ] {
            for extension in adapter.extensions() {
                assert_eq!(Language::from_extension(extension), adapter.language());
            }
        }
    }
}
