use super::*;
use crate::PythonAdapter;
use mimir_parse_core::LanguageAdapter;
use mimir_types::FileId;

fn symbols(source: &str) -> Vec<ExtractedSymbol> {
    let adapter = PythonAdapter;
    let tree = adapter.parse(source).unwrap();
    adapter.extract_symbols(&tree, FileId::new(1))
}

fn named(found: &[ExtractedSymbol], name: &str) -> Vec<(SymbolKind, Vec<String>)> {
    found
        .iter()
        .filter(|s| s.name == name)
        .map(|s| (s.kind, s.qualified_name.segments().to_vec()))
        .collect()
}

fn kind(found: &[ExtractedSymbol], name: &str) -> SymbolKind {
    found.iter().find(|s| s.name == name).unwrap().kind
}

#[test]
fn every_file_has_its_module() {
    let found = symbols("x = 1\n");
    assert_eq!(kind(&found, "<module>"), SymbolKind::Module);
}

#[test]
fn functions_classes_and_methods() {
    let found = symbols(
        "def f():\n    def inner():\n        pass\nasync def g():\n    pass\nclass A:\n    def __init__(self):\n        pass\n    def run(self):\n        pass\n    @staticmethod\n    def s():\n        pass\n    class B:\n        def deep(self):\n            pass\n",
    );
    assert_eq!(kind(&found, "f"), SymbolKind::Function);
    assert_eq!(kind(&found, "inner"), SymbolKind::Function);
    assert_eq!(kind(&found, "g"), SymbolKind::Function);
    assert_eq!(kind(&found, "A"), SymbolKind::Class);
    assert_eq!(kind(&found, "__init__"), SymbolKind::Constructor);
    assert_eq!(
        named(&found, "run"),
        [(SymbolKind::Method, vec!["A".to_string(), "run".to_string()])]
    );
    assert_eq!(kind(&found, "s"), SymbolKind::Method);
    assert_eq!(
        named(&found, "deep"),
        [(
            SymbolKind::Method,
            vec!["B".to_string(), "deep".to_string()]
        )]
    );
}

#[test]
fn a_decorated_definition_holds_its_decorators() {
    let found = symbols("@deco\n@other(1)\ndef f():\n    pass\n");
    let f = found.iter().find(|s| s.name == "f").unwrap();
    assert_eq!(f.range.start_line, 1);
    assert_eq!(f.range.end_line, 4);
}

#[test]
fn overloads_are_one_symbol_at_the_implementation() {
    let found = symbols(
        "@overload\ndef f(a: int) -> int: ...\n@typing.overload\ndef f(a: str) -> str: ...\ndef f(a):\n    return a\n",
    );
    let f: Vec<_> = found.iter().filter(|s| s.name == "f").collect();
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].range.start_line, 5);
}

#[test]
fn overloads_with_no_implementation_are_the_first_signature() {
    let found =
        symbols("@overload\ndef f(a: int) -> int: ...\n@overload\ndef f(a: str) -> str: ...\n");
    let f: Vec<_> = found.iter().filter(|s| s.name == "f").collect();
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].range.start_line, 1);
}

#[test]
fn a_getter_and_a_setter_are_two_symbols() {
    let found = symbols(
        "class A:\n    @property\n    def x(self):\n        return 1\n    @x.setter\n    def x(self, v):\n        pass\n",
    );
    assert_eq!(found.iter().filter(|s| s.name == "x").count(), 2);
}

#[test]
fn what_a_module_assigns() {
    let found = symbols(
        "MAX_SIZE = 1\nname = 2\ntyped: int = 3\nonly_type: int\nfast = lambda x: x\na, b = 1, 2\ndef f():\n    local = 1\n",
    );
    assert_eq!(kind(&found, "MAX_SIZE"), SymbolKind::Constant);
    assert_eq!(kind(&found, "name"), SymbolKind::Variable);
    assert_eq!(kind(&found, "typed"), SymbolKind::Variable);
    assert_eq!(kind(&found, "only_type"), SymbolKind::Variable);
    assert_eq!(kind(&found, "fast"), SymbolKind::Function);
    assert_eq!(kind(&found, "a"), SymbolKind::Variable);
    assert_eq!(kind(&found, "b"), SymbolKind::Variable);
    assert!(named(&found, "local").is_empty());
}

#[test]
fn a_variable_under_a_condition_is_still_at_the_top() {
    let found = symbols(
        "try:\n    fast = 1\nexcept ImportError:\n    fast = None\nif True:\n    def g():\n        pass\n",
    );
    assert_eq!(named(&found, "fast").len(), 2);
    assert_eq!(kind(&found, "g"), SymbolKind::Function);
}

#[test]
fn fields_of_a_class_and_of_self() {
    let found = symbols(
        "class A:\n    x: int\n    y = 1\n    pick = lambda self: 1\n    def __init__(self, other):\n        self.x = 2\n        self.z = 3\n        self.z = 4\n        other.no = 5\n        if True:\n            self.w = 6\n",
    );
    let field = |name: &str| (SymbolKind::Field, vec!["A".to_string(), name.to_string()]);
    assert_eq!(named(&found, "x"), [field("x")]);
    assert_eq!(named(&found, "y"), [field("y")]);
    assert_eq!(kind(&found, "pick"), SymbolKind::Method);
    assert_eq!(named(&found, "z"), [field("z")]);
    assert_eq!(named(&found, "w"), [field("w")]);
    assert!(named(&found, "no").is_empty());
    let z = found.iter().find(|s| s.name == "z").unwrap();
    assert_eq!(z.range.start_line, 7);
}

#[test]
fn a_function_inside_a_method_declares_no_field() {
    let found = symbols(
        "class A:\n    def run(self):\n        def inner(self):\n            self.no = 1\n",
    );
    assert!(named(&found, "no").is_empty());
}

#[test]
fn members_of_an_enum() {
    let found = symbols(
        "class Color(Enum):\n    RED = 1\nclass Flags(enum.IntFlag):\n    A = 1\nclass Plain(Base):\n    B = 1\n",
    );
    assert_eq!(kind(&found, "RED"), SymbolKind::EnumVariant);
    assert_eq!(kind(&found, "A"), SymbolKind::EnumVariant);
    assert_eq!(kind(&found, "B"), SymbolKind::Field);
}

#[test]
fn a_type_alias() {
    let found = symbols("type Id = int\ntype Pair[T] = tuple[T, T]\n");
    assert_eq!(kind(&found, "Id"), SymbolKind::TypeAlias);
    assert_eq!(kind(&found, "Pair"), SymbolKind::TypeAlias);
}

#[test]
fn a_leading_underscore_is_private() {
    let found = symbols(
        "def _hidden():\n    pass\ndef shown():\n    pass\nclass A:\n    def __init__(self):\n        pass\n    def __secret(self):\n        pass\n",
    );
    let visibility = |name: &str| found.iter().find(|s| s.name == name).unwrap().visibility;
    assert_eq!(visibility("_hidden"), Visibility::Private);
    assert_eq!(visibility("shown"), Visibility::Public);
    assert_eq!(visibility("__init__"), Visibility::Public);
    assert_eq!(visibility("__secret"), Visibility::Private);
}
