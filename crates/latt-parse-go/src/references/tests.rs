use latt_parse_core::LanguageAdapter;
use latt_parse_core::extracted::{CallForm, ExtractedReference};
use latt_types::{FileId, ReferenceKind};

use crate::GoAdapter;

fn references(source: &str) -> Vec<ExtractedReference> {
    let tree = GoAdapter.parse(source).unwrap();
    GoAdapter.extract_references(&tree, FileId::new(1))
}

/// The references of a function's body, with what is around it.
fn in_function(body: &str) -> Vec<ExtractedReference> {
    references(&format!(
        "package a\nimport \"x/inner\"\nfunc f() {{\n{body}\n}}\n"
    ))
}

fn of(found: &[ExtractedReference], kind: ReferenceKind) -> Vec<String> {
    found
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| r.name.clone())
        .collect()
}

fn form(found: &[ExtractedReference], name: &str) -> Option<CallForm> {
    found
        .iter()
        .find(|r| r.kind == ReferenceKind::Call && r.name == name)
        .and_then(|r| r.call_form)
}

#[test]
fn calls_in_each_form() {
    let found = in_function(
        "g()\ninner.F()\nx.M()\ngo h()\ndefer k()\nm[int]()\nn[int, string](1)\nfunc() { in() }()\n",
    );
    assert_eq!(
        of(&found, ReferenceKind::Call),
        ["g", "F", "M", "h", "k", "m", "n", "in"]
    );
    assert_eq!(form(&found, "g"), Some(CallForm::Plain));
    assert_eq!(form(&found, "F"), Some(CallForm::Scoped));
    assert_eq!(form(&found, "M"), Some(CallForm::Method));
    let scoped = found.iter().find(|r| r.name == "F").unwrap();
    assert_eq!(scoped.context.as_deref(), Some("inner"));
}

#[test]
fn a_conversion_is_a_call_until_the_answer_says() {
    let found = in_function("MyInt(1)\ninner.T(2)\n");
    assert_eq!(of(&found, ReferenceKind::Call), ["MyInt", "T"]);
}

#[test]
fn a_name_the_function_binds_is_no_value_and_no_plain_call() {
    let found = in_function("x := helper\nx\nx()\nvar y = 1\ny\n");
    assert_eq!(of(&found, ReferenceKind::FunctionValue), ["helper"]);
    assert_eq!(form(&found, "x"), Some(CallForm::Bound));
}

#[test]
fn a_binding_holds_only_after_it_and_in_its_scope() {
    let found = in_function("helper\n{\n  helper := 1\n  helper\n}\nhelper\n");
    assert_eq!(
        of(&found, ReferenceKind::FunctionValue),
        ["helper", "helper"]
    );
}

#[test]
fn a_parameter_a_receiver_and_a_range_bind() {
    let found = references(
        "package a\nfunc (c *T) m(p int, q ...string) (r int) {\nc\np\nq\nr\nfor k, v := range items {\nk\nv\n}\nk\nreturn 0\n}\n",
    );
    assert_eq!(of(&found, ReferenceKind::FunctionValue), ["items", "k"]);
    assert_eq!(
        of(&found, ReferenceKind::TypeUse),
        ["T", "int", "string", "int"],
        "a built-in type is asked about as any other, and gets no edge"
    );
}

#[test]
fn a_type_switch_binds_in_its_cases() {
    let found = in_function("switch v := x.(type) {\ncase T:\nv.M()\ndefault:\nv\n}\nv\n");
    assert_eq!(of(&found, ReferenceKind::FunctionValue), ["x", "v"]);
    assert_eq!(of(&found, ReferenceKind::TypeUse), ["T"]);
    assert_eq!(of(&found, ReferenceKind::Call), ["M"]);
}

#[test]
fn values_named_without_a_call() {
    let found = in_function(
        "p := helper\nq := c.Area\nr := Circle.Area\ns := inner.Limit\nt := helper[int]\n",
    );
    assert_eq!(
        of(&found, ReferenceKind::FunctionValue),
        ["helper", "c", "Circle", "helper", "int"]
    );
    assert_eq!(
        of(&found, ReferenceKind::FieldUse),
        ["Area", "Area", "Limit"]
    );
}

#[test]
fn a_type_named_anywhere() {
    let found = references(
        "package a\nimport \"x/inner\"\n\
         type A struct {\n  f B\n  C\n  g inner.D\n  h []E\n  i map[K]V\n}\n\
         type I interface {\n  N(o P) Q\n  R\n}\n\
         type G[T any] struct{ t T }\n\
         var v L\n\
         func f(a M) *O {\n  x := S{}\n  y := []U{}\n  x.(W)\n  return nil\n}\n",
    );
    let mut types = of(&found, ReferenceKind::TypeUse);
    types.sort();
    let mut expected = [
        "B", "C", "D", "E", "K", "L", "M", "O", "P", "Q", "R", "S", "U", "V", "W", "any",
    ];
    expected.sort();
    assert_eq!(types, expected);
}

#[test]
fn a_field_read_and_written() {
    let found = in_function("x := a.b\na.c = 1\na.d.e = 2\na.g++\nz := T{h: 1, i: y}\n");
    assert_eq!(
        of(&found, ReferenceKind::FieldUse),
        ["b", "d", "g", "h", "i"]
    );
    assert_eq!(of(&found, ReferenceKind::FieldWrite), ["c", "e"]);
}

#[test]
fn an_import_gives_its_name_and_path() {
    let found = references(
        "package a\nimport (\n  \"x/inner\"\n  al \"x/other\"\n  \"x/mod/v2\"\n  . \"x/dot\"\n  _ \"x/blank\"\n)\nimport \"single\"\n",
    );
    let imports: Vec<(&str, Option<&str>, bool)> = found
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| {
            (
                r.name.as_str(),
                r.context.as_deref(),
                r.call_form == Some(CallForm::Bound),
            )
        })
        .collect();
    assert_eq!(
        imports,
        [
            ("inner", Some("x/inner"), false),
            ("al", Some("x/other"), true),
            ("mod", Some("x/mod/v2"), false),
            (".", Some("x/dot"), true),
            ("_", Some("x/blank"), true),
            ("single", Some("single"), false),
        ]
    );
}

#[test]
fn a_method_is_named_where_it_is_declared() {
    let found = references(
        "package a\nfunc (c T) Area() {}\nfunc plain() {}\ntype I interface {\n  Name() string\n}\n",
    );
    assert_eq!(
        of(&found, ReferenceKind::MethodDeclaration),
        ["Area", "Name"]
    );
}

#[test]
fn a_local_hiding_a_package_is_not_the_package() {
    let found = in_function("inner := make()\ninner.M()\n");
    assert_eq!(form(&found, "M"), Some(CallForm::Method));
}

#[test]
fn code_at_package_level_is_read() {
    let found = references("package a\nvar x = helper()\nvar y = other\nconst c = d\n");
    assert_eq!(of(&found, ReferenceKind::Call), ["helper"]);
    assert_eq!(of(&found, ReferenceKind::FunctionValue), ["other", "d"]);
}

#[test]
fn a_name_is_where_a_server_is_asked() {
    let found = in_function("  inner.F()\n");
    let call = found.iter().find(|r| r.name == "F").unwrap();
    assert_eq!((call.range.start_line, call.range.start_col), (4, 8));
}
