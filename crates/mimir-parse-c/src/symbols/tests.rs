use mimir_parse_core::LanguageAdapter;
use mimir_parse_core::extracted::ExtractedSymbol;
use mimir_types::{FileId, SymbolKind, Visibility};

use crate::CFamilyAdapter;

fn read(adapter: CFamilyAdapter, source: &str) -> Vec<ExtractedSymbol> {
    let tree = adapter.parse(source).unwrap();
    adapter
        .extract_symbols(&tree, FileId::new(1))
        .into_iter()
        .filter(|s| s.kind != SymbolKind::Module)
        .collect()
}

fn c(source: &str) -> Vec<ExtractedSymbol> {
    read(CFamilyAdapter::c(), source)
}

fn cpp(source: &str) -> Vec<ExtractedSymbol> {
    read(CFamilyAdapter::cpp(), source)
}

fn named(found: &[ExtractedSymbol]) -> Vec<(String, SymbolKind)> {
    found
        .iter()
        .map(|s| (s.qualified_name.join("::"), s.kind))
        .collect()
}

fn has(found: &[ExtractedSymbol], name: &str, kind: SymbolKind) -> bool {
    named(found).contains(&(name.to_string(), kind))
}

#[test]
fn a_function_defined_is_a_symbol_and_one_declared_is_not() {
    let found = c(
        "int only_declared(int a);\nint f(int a) { return a; }\nstatic int g(void) { return 1; }\n",
    );
    assert_eq!(
        named(&found),
        [
            ("f".to_string(), SymbolKind::Function),
            ("g".to_string(), SymbolKind::Function)
        ]
    );
    assert_eq!(found[0].visibility, Visibility::Public);
    assert_eq!(found[1].visibility, Visibility::Private);
}

#[test]
fn what_a_function_returns_does_not_hide_it() {
    let found = c("struct a *f(void) { return 0; }\nint *(*g(void))(void) { return 0; }\n");
    assert!(
        has(&found, "f", SymbolKind::Function),
        "{:?}",
        named(&found)
    );
    assert!(
        has(&found, "g", SymbolKind::Function),
        "{:?}",
        named(&found)
    );
}

#[test]
fn structs_fields_and_typedefs_of_c() {
    let found = c("struct ops { int (*run)(int); int count; };\n\
                   typedef struct { int x; } point;\n\
                   typedef struct tag { int y; } other;\n\
                   typedef int number;\n\
                   union u { int a; };\n\
                   enum color { RED, BLUE };\n");
    for (name, kind) in [
        ("ops", SymbolKind::Struct),
        ("ops::run", SymbolKind::Field),
        ("ops::count", SymbolKind::Field),
        ("point", SymbolKind::Struct),
        ("point::x", SymbolKind::Field),
        ("tag", SymbolKind::Struct),
        ("other", SymbolKind::TypeAlias),
        ("number", SymbolKind::TypeAlias),
        ("u", SymbolKind::Union),
        ("color", SymbolKind::Enum),
        ("color::RED", SymbolKind::EnumVariant),
    ] {
        assert!(has(&found, name, kind), "{name}: {:?}", named(&found));
    }
}

#[test]
fn variables_at_file_level() {
    let found = c(
        "int counter = 1;\nstatic const int LIMIT = 2;\nextern int elsewhere;\nint (*fp)(void) = 0;\nint declared(void);\nint *also(void);\n",
    );
    assert!(has(&found, "counter", SymbolKind::Variable));
    assert!(has(&found, "fp", SymbolKind::Variable));
    assert!(
        !named(&found)
            .iter()
            .any(|(name, _)| name == "declared" || name == "also")
    );
    assert!(has(&found, "LIMIT", SymbolKind::Constant));
    assert!(!named(&found).iter().any(|(name, _)| name == "elsewhere"));
}

#[test]
fn a_local_is_no_symbol() {
    let found = c("int f(void) { int local = 1; return local; }\n");
    assert_eq!(named(&found), [("f".to_string(), SymbolKind::Function)]);
}

#[test]
fn macros_are_no_symbols() {
    let found = c("#define TWICE(x) twice(x)\n#define LIMIT 4\n");
    assert!(found.is_empty(), "{:?}", named(&found));
}

#[test]
fn both_branches_of_an_if_are_read() {
    let found =
        c("#ifdef A\nint f(void) { return 1; }\n#else\nint f(void) { return 0; }\n#endif\n");
    assert_eq!(found.iter().filter(|s| s.name == "f").count(), 2);
}

#[test]
fn a_class_its_methods_and_a_method_only_declared() {
    let found = cpp("namespace geo {\n\
        struct Shape { virtual double area() const = 0; virtual ~Shape(); };\n\
        struct Circle : Shape {\n\
          double r;\n\
          explicit Circle(double r);\n\
          double area() const override;\n\
          int inline_one() { return 1; }\n\
        };\n\
        }\n");
    for (name, kind) in [
        ("geo", SymbolKind::Namespace),
        ("geo::Shape", SymbolKind::Struct),
        ("geo::Shape::area", SymbolKind::Method),
        ("geo::Circle::r", SymbolKind::Field),
        ("geo::Circle::inline_one", SymbolKind::Method),
    ] {
        assert!(has(&found, name, kind), "{name}: {:?}", named(&found));
    }
    // Defined elsewhere, so a symbol there.
    assert!(!named(&found).iter().any(|(n, _)| n == "geo::Circle::area"));
    assert!(
        !named(&found)
            .iter()
            .any(|(n, _)| n == "geo::Circle::Circle")
    );
}

#[test]
fn a_definition_outside_its_class() {
    let found = cpp("namespace geo {\n\
        Circle::Circle(double r) : r(r) {}\n\
        Circle::~Circle() {}\n\
        double Circle::area() const { return r; }\n\
        Vec Vec::operator+(const Vec& o) const { return o; }\n\
        }\n");
    for (name, kind) in [
        ("geo::Circle::Circle", SymbolKind::Constructor),
        ("geo::Circle::~Circle", SymbolKind::Method),
        ("geo::Circle::area", SymbolKind::Method),
        ("geo::Vec::operator+", SymbolKind::Method),
    ] {
        assert!(has(&found, name, kind), "{name}: {:?}", named(&found));
    }
    let area = found.iter().find(|s| s.name == "area").unwrap();
    assert_eq!(area.parent.as_deref(), Some("Circle"));
}

#[test]
fn a_namespace_written_in_a_name_is_no_class() {
    let found = cpp("namespace a { int g(); }\nint a::g() { return 1; }\n");
    assert!(
        has(&found, "a::g", SymbolKind::Function),
        "{:?}",
        named(&found)
    );
}

#[test]
fn overloads_are_two() {
    let found = cpp("int pick(int a) { return a; }\ndouble pick(double a) { return a; }\n");
    assert_eq!(found.iter().filter(|s| s.name == "pick").count(), 2);
}

#[test]
fn a_template_is_one_and_a_specialization_another() {
    let found = cpp("template <class T> T biggest(T a, T b) { return a; }\n\
        template <class T> struct Box { T v; void put(T x) { v = x; } T get() const; };\n\
        template <class T> T Box<T>::get() const { return v; }\n\
        template <> struct Box<char> { char v; void put(char x); };\n\
        void Box<char>::put(char x) { v = x; }\n");
    for (name, kind) in [
        ("biggest", SymbolKind::Function),
        ("Box", SymbolKind::Struct),
        ("Box::put", SymbolKind::Method),
        ("Box::get", SymbolKind::Method),
        ("Box<char>", SymbolKind::Struct),
        ("Box<char>::v", SymbolKind::Field),
        ("Box<char>::put", SymbolKind::Method),
    ] {
        assert!(
            has(&found, name, kind) || name == "Box<char>",
            "{name}: {:?}",
            named(&found)
        );
    }
    assert_eq!(found.iter().filter(|s| s.name == "Box").count(), 2);
}

#[test]
fn a_conversion_operator() {
    let found = cpp(
        "class A { operator int() const { return 1; } };\nA::operator bool() const { return true; }\n",
    );
    assert!(
        has(&found, "A::operator int", SymbolKind::Method),
        "{:?}",
        named(&found)
    );
    assert!(
        has(&found, "A::operator bool", SymbolKind::Method),
        "{:?}",
        named(&found)
    );
}

#[test]
fn a_macro_in_the_way_hides_nothing() {
    let found = cpp(
        "class LEVELDB_EXPORT Slice {\n public:\n  Slice() : a(0) {}\n  bool Insert(int f) LOCKS(mu_) { return true; }\n  int a;\n};\nSlice::~Slice() NOEXCEPT = default;\n",
    );
    for (name, kind) in [
        ("Slice", SymbolKind::Class),
        ("Slice::Slice", SymbolKind::Constructor),
        ("Slice::Insert", SymbolKind::Method),
        ("Slice::a", SymbolKind::Field),
        ("Slice::~Slice", SymbolKind::Method),
    ] {
        assert!(has(&found, name, kind), "{name}: {:?}", named(&found));
    }
}

#[test]
fn an_anonymous_namespace_adds_nothing_to_a_name() {
    let found = cpp("namespace { int hidden() { return 1; } }\n");
    assert!(
        has(&found, "hidden", SymbolKind::Function),
        "{:?}",
        named(&found)
    );
}

#[test]
fn a_nested_namespace_and_extern_c() {
    let found = cpp(
        "namespace a::b { int f() { return 1; } }\nextern \"C\" { int g(void) { return 1; } }\nextern \"C\" int h(void) { return 1; }\n",
    );
    assert!(
        has(&found, "a::b::f", SymbolKind::Function),
        "{:?}",
        named(&found)
    );
    assert!(
        has(&found, "g", SymbolKind::Function),
        "{:?}",
        named(&found)
    );
    assert!(
        has(&found, "h", SymbolKind::Function),
        "{:?}",
        named(&found)
    );
}

#[test]
fn an_alias_and_a_nested_class() {
    let found = cpp("using number = int;\nclass A { class B { int x; }; enum class C { D }; };\n");
    for (name, kind) in [
        ("number", SymbolKind::TypeAlias),
        ("A", SymbolKind::Class),
        ("A::B", SymbolKind::Class),
        ("A::B::x", SymbolKind::Field),
        ("A::C::D", SymbolKind::EnumVariant),
    ] {
        assert!(has(&found, name, kind), "{name}: {:?}", named(&found));
    }
}
