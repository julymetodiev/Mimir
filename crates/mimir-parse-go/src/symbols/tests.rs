use mimir_parse_core::LanguageAdapter;
use mimir_types::{FileId, SymbolKind, Visibility};

use crate::GoAdapter;

fn symbols(source: &str) -> Vec<(String, SymbolKind)> {
    let tree = GoAdapter.parse(source).unwrap();
    GoAdapter
        .extract_symbols(&tree, FileId::new(1))
        .into_iter()
        .filter(|s| s.kind != SymbolKind::Module)
        .map(|s| (s.qualified_name.join("::"), s.kind))
        .collect()
}

fn has(found: &[(String, SymbolKind)], name: &str, kind: SymbolKind) {
    assert!(
        found.iter().any(|(n, k)| n == name && *k == kind),
        "{name} {kind:?} not in {found:?}"
    );
}

#[test]
fn functions_and_methods_by_their_receiver() {
    let found = symbols(
        "package a\n\
         func init() {}\n\
         func init() {}\n\
         func main() {}\n\
         func Map[T any](x T) T { return x }\n\
         func (c Circle) Area() int { return 0 }\n\
         func (c *Circle) Grow() {}\n\
         func (s *Stack[T]) Push(x T) {}\n",
    );
    assert_eq!(
        found.iter().filter(|(n, _)| n == "init").count(),
        2,
        "a package may have several"
    );
    has(&found, "main", SymbolKind::Function);
    has(&found, "Map", SymbolKind::Function);
    has(&found, "Circle::Area", SymbolKind::Method);
    has(&found, "Circle::Grow", SymbolKind::Method);
    has(&found, "Stack::Push", SymbolKind::Method);
}

#[test]
fn types_and_what_they_hold() {
    let found = symbols(
        "package a\n\
         type Circle struct {\n\
             R, S int\n\
             Base\n\
             *Other\n\
             inner.Plain\n\
             Gen[int]\n\
         }\n\
         type Area interface {\n\
             Area() float64\n\
             Named\n\
         }\n\
         type MyInt int\n\
         type Fn func(int) int\n\
         type Alias = Circle\n\
         type (\n\
             InList struct{ X int }\n\
         )\n",
    );
    has(&found, "Circle", SymbolKind::Struct);
    for field in ["R", "S", "Base", "Other", "Plain", "Gen"] {
        has(&found, &format!("Circle::{field}"), SymbolKind::Field);
    }
    has(&found, "Area", SymbolKind::Interface);
    has(&found, "Area::Area", SymbolKind::Method);
    assert!(!found.iter().any(|(n, _)| n == "Area::Named"));
    has(&found, "MyInt", SymbolKind::TypeAlias);
    has(&found, "Fn", SymbolKind::TypeAlias);
    has(&found, "Alias", SymbolKind::TypeAlias);
    has(&found, "InList", SymbolKind::Struct);
    has(&found, "InList::X", SymbolKind::Field);
}

#[test]
fn constants_and_variables_at_the_top_only() {
    let found = symbols(
        "package a\n\
         const Limit = 1\n\
         const (\n\
             A = iota\n\
             B\n\
         )\n\
         var x, y = 1, func() {}\n\
         var (\n\
             z int\n\
             _ = helper()\n\
         )\n\
         var Handler = func() {}\n\
         var plain = helper\n\
         func f() {\n\
             const local = 1\n\
             var inner = 2\n\
             type T int\n\
             g := func() {}\n\
         }\n",
    );
    has(&found, "Limit", SymbolKind::Constant);
    has(&found, "A", SymbolKind::Constant);
    has(&found, "B", SymbolKind::Constant);
    has(&found, "x", SymbolKind::Variable);
    has(&found, "y", SymbolKind::Function);
    has(&found, "z", SymbolKind::Variable);
    has(&found, "Handler", SymbolKind::Function);
    has(&found, "plain", SymbolKind::Variable);
    for missing in ["local", "inner", "T", "g", "_"] {
        assert!(!found.iter().any(|(n, _)| n == missing), "{missing}");
    }
}

#[test]
fn what_is_exported_is_public() {
    let tree = GoAdapter
        .parse("package a\nfunc Open() {}\nfunc closed() {}\n")
        .unwrap();
    let found = GoAdapter.extract_symbols(&tree, FileId::new(1));
    let visibility = |name: &str| found.iter().find(|s| s.name == name).unwrap().visibility;
    assert_eq!(visibility("Open"), Visibility::Public);
    assert_eq!(visibility("closed"), Visibility::Private);
}
