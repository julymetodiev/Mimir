//! Rust language adapter implementation.

use mimir_parse_core::extracted::{ExtractedReference, ExtractedSymbol};
use mimir_parse_core::{LanguageAdapter, ParsedTree};
use mimir_types::{FileId, Language};
use std::cell::RefCell;
use tree_sitter::Parser;

use crate::references::extract_references;
use crate::symbols::extract_symbols;

thread_local! {
    /// Thread-local parser instance for lock-free parsing. It is `None` only when tree-sitter
    /// refuses the Rust grammar, and then parsing gives no tree.
    static RUST_PARSER: RefCell<Option<Parser>> = RefCell::new(rust_parser());
}

fn rust_parser() -> Option<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .ok()?;
    Some(parser)
}

/// Rust language adapter using tree-sitter-rust.
/// Uses thread-local storage for lock-free concurrent parsing.
pub struct RustAdapter;

impl RustAdapter {
    /// Create a new Rust adapter.
    pub fn new() -> Self {
        Self
    }
}

impl Default for RustAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for RustAdapter {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn parse(&self, source: &str) -> Option<ParsedTree> {
        RUST_PARSER.with(|parser| {
            let mut parser = parser.borrow_mut();
            ParsedTree::parse(parser.as_mut()?, source)
        })
    }

    fn extract_symbols(&self, tree: &ParsedTree, file_id: FileId) -> Vec<ExtractedSymbol> {
        extract_symbols(tree, file_id)
    }

    fn extract_references(&self, tree: &ParsedTree, file_id: FileId) -> Vec<ExtractedReference> {
        extract_references(tree, file_id)
    }

    fn extensions(&self) -> &[&str] {
        &["rs"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::SymbolKind;

    #[test]
    fn test_parse_simple_function() {
        let adapter = RustAdapter::new();
        let source = "fn main() { println!(\"Hello\"); }";

        assert!(adapter.parse(source).is_some());
    }

    #[test]
    fn test_extract_function() {
        let adapter = RustAdapter::new();
        let source = "pub fn process_data(input: &str) -> Result<(), Error> { Ok(()) }";

        let tree = adapter.parse(source).expect("Failed to parse");
        let symbols = adapter.extract_symbols(&tree, FileId::new(1));

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "process_data");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn test_extract_struct_with_fields() {
        let adapter = RustAdapter::new();
        let source = r#"
pub struct User {
    pub name: String,
    age: u32,
}
"#;

        let tree = adapter.parse(source).expect("Failed to parse");
        let symbols = adapter.extract_symbols(&tree, FileId::new(1));

        // Should have struct + 2 fields
        assert_eq!(symbols.len(), 3);
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "User" && s.kind == SymbolKind::Struct)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "name" && s.kind == SymbolKind::Field)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "age" && s.kind == SymbolKind::Field)
        );
    }

    #[test]
    fn test_extract_enum_with_variants() {
        let adapter = RustAdapter::new();
        let source = r#"
pub enum Status {
    Active,
    Inactive,
    Pending,
}
"#;

        let tree = adapter.parse(source).expect("Failed to parse");
        let symbols = adapter.extract_symbols(&tree, FileId::new(1));

        // Should have enum + 3 variants
        assert_eq!(symbols.len(), 4);
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Status" && s.kind == SymbolKind::Enum)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Active" && s.kind == SymbolKind::EnumVariant)
        );
    }

    #[test]
    fn test_extract_impl_with_methods() {
        let adapter = RustAdapter::new();
        let source = r#"
impl User {
    pub fn new(name: String) -> Self {
        Self { name, age: 0 }
    }

    fn validate(&self) -> bool {
        true
    }
}
"#;

        let tree = adapter.parse(source).expect("Failed to parse");
        let symbols = adapter.extract_symbols(&tree, FileId::new(1));

        // Should have impl + 2 methods
        assert_eq!(symbols.len(), 3);
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Impl));
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "new" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "validate" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_extract_function_calls() {
        let adapter = RustAdapter::new();
        let source = r#"
fn process() {
    helper();
    obj.method();
    Foo::bar();
}
"#;

        let tree = adapter.parse(source).expect("Failed to parse");
        let references = adapter.extract_references(&tree, FileId::new(1));

        // Should find the three calls
        assert!(references.len() >= 3);
        assert!(references.iter().any(|r| r.name == "helper"));
        assert!(references.iter().any(|r| r.name == "method"));
        assert!(references.iter().any(|r| r.name == "bar"));
    }

    /// Asking for a child by its index walks the siblings before it, which over a long file took
    /// tens of seconds. The bound is far above a cursor's few milliseconds, and far below that.
    #[test]
    fn test_a_file_of_many_top_level_items_takes_linear_time() {
        let adapter = RustAdapter::new();
        let source = format!("fn first() {{}}\n{}", "// filler\n".repeat(49_000));
        let started = std::time::Instant::now();

        let tree = adapter.parse(&source).expect("Failed to parse");
        let symbols = adapter.extract_symbols(&tree, FileId::new(1));
        adapter.extract_references(&tree, FileId::new(1));

        assert_eq!(symbols.len(), 1);
        assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
    }

    #[test]
    fn test_extract_use_statements() {
        let adapter = RustAdapter::new();
        let source = r#"
use std::collections::HashMap;
use crate::module::Item;
"#;

        let tree = adapter.parse(source).expect("Failed to parse");
        let references = adapter.extract_references(&tree, FileId::new(1));

        // Should find the imports
        assert!(references.iter().any(|r| r.name == "HashMap"));
        assert!(references.iter().any(|r| r.name == "Item"));
    }

    #[test]
    fn test_trait_definition() {
        let adapter = RustAdapter::new();
        let source = r#"
pub trait Processor {
    fn process(&self);
}
"#;

        let tree = adapter.parse(source).expect("Failed to parse");
        let symbols = adapter.extract_symbols(&tree, FileId::new(1));

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Processor" && s.kind == SymbolKind::Trait)
        );
    }

    #[test]
    fn test_module_definition() {
        let adapter = RustAdapter::new();
        let source = r#"
mod utils {
    pub fn helper() {}
}
"#;

        let tree = adapter.parse(source).expect("Failed to parse");
        let symbols = adapter.extract_symbols(&tree, FileId::new(1));

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "utils" && s.kind == SymbolKind::Module)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "helper" && s.kind == SymbolKind::Function)
        );
    }

    #[test]
    fn test_concurrent_parsing() {
        use std::thread;

        let handles: Vec<_> = (0..4)
            .map(|i| {
                thread::spawn(move || {
                    let adapter = RustAdapter::new();
                    let source = format!("fn test_{}() {{}}", i);
                    let tree = adapter.parse(&source).expect("Failed to parse");
                    let symbols = adapter.extract_symbols(&tree, FileId::new(i as u64));
                    assert_eq!(symbols.len(), 1);
                    assert_eq!(symbols[0].name, format!("test_{}", i));
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("Thread panicked");
        }
    }
}
