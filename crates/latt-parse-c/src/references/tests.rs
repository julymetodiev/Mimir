use latt_parse_core::LanguageAdapter;
use latt_parse_core::extracted::{CallForm, ExtractedReference};
use latt_types::{FileId, ReferenceKind};

use crate::CFamilyAdapter;

fn read(adapter: CFamilyAdapter, source: &str) -> Vec<ExtractedReference> {
    let tree = adapter.parse(source).unwrap();
    adapter.extract_references(&tree, FileId::new(1))
}

fn c(source: &str) -> Vec<ExtractedReference> {
    read(CFamilyAdapter::c(), source)
}

fn cpp(source: &str) -> Vec<ExtractedReference> {
    read(CFamilyAdapter::cpp(), source)
}

fn of(found: &[ExtractedReference], kind: ReferenceKind) -> Vec<&str> {
    found
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| r.name.as_str())
        .collect()
}

fn form(found: &[ExtractedReference], name: &str) -> Option<CallForm> {
    found
        .iter()
        .find(|r| r.name == name && r.kind == ReferenceKind::Call)?
        .call_form
}

#[test]
fn calls_in_each_form_of_c() {
    let found = c("void f(struct s *o, struct s v) { g(); o->m(); v.n(); (h)(); }\n");
    assert_eq!(of(&found, ReferenceKind::Call), ["g", "m", "n", "h"]);
    assert_eq!(form(&found, "g"), Some(CallForm::Plain));
    assert_eq!(form(&found, "m"), Some(CallForm::Method));
    assert_eq!(form(&found, "n"), Some(CallForm::Method));
}

#[test]
fn calls_in_each_form_of_cpp() {
    let found = cpp(
        "void f(S& o) { g(); o.m(); ns::h(); ns::t<int>(); t2<int>(); this->k(); o.tm<int>(); }\n",
    );
    assert_eq!(
        of(&found, ReferenceKind::Call),
        ["g", "m", "h", "t", "t2", "k", "tm"]
    );
    assert_eq!(form(&found, "h"), Some(CallForm::Scoped));
    assert_eq!(form(&found, "t"), Some(CallForm::Scoped));
    assert_eq!(form(&found, "t2"), Some(CallForm::Plain));
    assert_eq!(form(&found, "k"), Some(CallForm::Method));
}

#[test]
fn a_reference_is_at_the_name() {
    let found = c("void f(void) {\n  g();\n}\n");
    let call = found.iter().find(|r| r.name == "g").unwrap();
    assert_eq!((call.range.start_line, call.range.start_col), (2, 2));
}

#[test]
fn a_name_a_function_binds_is_no_value_and_gives_no_call() {
    let found =
        c("void f(int (*cb)(void)) { int (*fp)(void) = helper; fp(); cb(); int x = 1; g(x); }\n");
    assert_eq!(of(&found, ReferenceKind::FunctionValue), ["helper"]);
    assert_eq!(form(&found, "fp"), Some(CallForm::Bound));
    assert_eq!(form(&found, "cb"), Some(CallForm::Bound));
    assert_eq!(form(&found, "g"), Some(CallForm::Plain));
}

#[test]
fn a_function_named_as_a_value_is_no_call() {
    let found = c("void f(struct ops *o) { o->run = twice; }\n");
    assert_eq!(of(&found, ReferenceKind::FunctionValue), ["twice"]);
    assert_eq!(of(&found, ReferenceKind::FieldWrite), ["run"]);
    assert!(of(&found, ReferenceKind::Call).is_empty());
}

#[test]
fn a_call_through_a_field_is_a_call_with_a_name_only_the_answer_can_place() {
    let found = c("void f(struct ops *o) { o->run(1); }\n");
    assert_eq!(form(&found, "run"), Some(CallForm::Method));
}

#[test]
fn fields_read_and_written() {
    let found = c("void f(struct s *o) { o->a = o->b; }\n");
    assert_eq!(of(&found, ReferenceKind::FieldWrite), ["a"]);
    assert_eq!(of(&found, ReferenceKind::FieldUse), ["b"]);
}

#[test]
fn a_constructor_at_the_name_and_new_at_the_type() {
    let found = cpp("void f() { Circle c(1.0); Square q; auto p = new Circle(2.0); }\n");
    assert_eq!(of(&found, ReferenceKind::Call), ["c", "Circle"]);
    assert_eq!(
        of(&found, ReferenceKind::TypeUse),
        ["Circle", "Square", "Circle"]
    );
}

#[test]
fn an_operator_gives_no_reference() {
    let found = cpp("Vec f(Vec a, Vec b) { return a + b; }\n");
    assert!(of(&found, ReferenceKind::Call).is_empty());
}

#[test]
fn a_macro_body_gives_nothing_and_its_use_is_a_call() {
    let found = c("#define TWICE(x) twice(x)\nvoid f(void) { TWICE(1); }\n");
    assert_eq!(of(&found, ReferenceKind::Call), ["TWICE"]);
}

#[test]
fn a_condition_gives_nothing_and_both_branches_do() {
    let found = c(
        "#if defined(A) && B\nvoid f(void) { g(); }\n#else\nvoid f(void) { h(); }\n#endif\n#ifdef C\nvoid k(void) { m(); }\n#endif\n",
    );
    assert_eq!(of(&found, ReferenceKind::Call), ["g", "h", "m"]);
    assert!(of(&found, ReferenceKind::FunctionValue).is_empty());
}

#[test]
fn an_include_is_an_import() {
    let found = c("#include <stdio.h>\n#include \"util.h\"\n");
    assert_eq!(of(&found, ReferenceKind::Import), ["stdio.h", "util.h"]);
}

#[test]
fn types_are_used_and_a_definition_is_no_use() {
    let found = cpp(
        "class Base {};\nclass A : public Base { Field f; };\nOut g(In a) { return Local(); }\n",
    );
    assert_eq!(
        of(&found, ReferenceKind::TypeUse),
        ["Base", "Field", "Out", "In"]
    );
}

#[test]
fn a_template_parameter_is_no_type_used() {
    let found = cpp("template <class T> T f(T a, Other b) { return a; }\n");
    assert_eq!(of(&found, ReferenceKind::TypeUse), ["Other"]);
}

#[test]
fn a_lambda_binds_what_it_binds_and_calls_what_it_calls() {
    let found = cpp("void f() { auto l = [](int a) { return helper(a); }; l(1); }\n");
    assert_eq!(form(&found, "helper"), Some(CallForm::Plain));
    assert_eq!(form(&found, "l"), Some(CallForm::Bound));
    assert!(!of(&found, ReferenceKind::FunctionValue).contains(&"a"));
}

#[test]
fn a_range_loop_binds() {
    let found = cpp("void f(Items items) { for (auto& item : items) { use(item); } }\n");
    assert!(
        of(&found, ReferenceKind::FunctionValue).is_empty(),
        "{found:?}"
    );
    assert_eq!(form(&found, "use"), Some(CallForm::Plain));
}

#[test]
fn a_method_says_where_it_is_and_a_free_declaration_of_a_pointer_does_not() {
    let found = cpp(
        "struct S { virtual int area() const = 0; int (*cb)(void); };\ndouble C::area() const { return 1; }\n",
    );
    assert_eq!(
        of(&found, ReferenceKind::MethodDeclaration),
        ["area", "area"]
    );
}

#[test]
fn what_a_definition_outside_its_class_names() {
    let found = cpp("A::A(int v) : x(v), Base(v) {}\nint a::B::f() { return 1; }\n");
    // The grammar cannot tell a base from a field there, and the answer can.
    assert_eq!(of(&found, ReferenceKind::FieldWrite), ["x", "Base"]);
    assert_eq!(of(&found, ReferenceKind::TypeUse), ["A", "a", "B"]);
}

#[test]
fn a_scoped_name_not_called_is_a_value_and_a_scoped_type_is_a_type() {
    let found = cpp("void f() { g(ns::h); ns::T x; }\n");
    assert_eq!(of(&found, ReferenceKind::FunctionValue), ["h"]);
    assert_eq!(of(&found, ReferenceKind::TypeUse), ["T"]);
}

#[test]
fn a_declaration_in_a_function_may_run_a_constructor() {
    let found = cpp("int g(int);\nvoid f() { T x(y); }\n");
    assert_eq!(of(&found, ReferenceKind::Call), ["x"]);
}
