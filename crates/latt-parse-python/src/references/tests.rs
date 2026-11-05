use super::*;
use crate::PythonAdapter;
use latt_parse_core::LanguageAdapter;
use latt_types::FileId;

fn references(source: &str) -> Vec<ExtractedReference> {
    let adapter = PythonAdapter;
    let tree = adapter.parse(source).unwrap();
    assert!(!tree.root_node().has_error(), "{source}");
    adapter.extract_references(&tree, FileId::new(1))
}

fn kinds(found: &[ExtractedReference], kind: ReferenceKind) -> Vec<&str> {
    found
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| r.name.as_str())
        .collect()
}

#[test]
fn a_call_is_found_at_its_name() {
    let found = references(
        "def a():\n    obj.run(1)\n    f()\n    Box()\n    o.p.q()\n    (x or y).z()\n    g()()\n",
    );
    assert_eq!(
        kinds(&found, ReferenceKind::Call),
        ["run", "f", "Box", "q", "z", "g"]
    );
    let run = &found[0];
    assert_eq!((run.range.start_line, run.range.start_col), (2, 8));
    assert_eq!(run.call_form, Some(CallForm::Method));
    assert_eq!(run.context.as_deref(), Some("obj"));
    assert_eq!(kinds(&found, ReferenceKind::FieldUse), ["p"]);
}

#[test]
fn a_decorator_is_a_call() {
    let found =
        references("@deco\n@app.route(\"/\")\ndef f():\n    pass\n@a.b\nclass C:\n    pass\n");
    assert_eq!(kinds(&found, ReferenceKind::Call), ["deco", "route", "b"]);
}

#[test]
fn a_setter_names_no_getter() {
    let found = references("class A:\n    @size.setter\n    def size(self, v):\n        pass\n");
    assert!(kinds(&found, ReferenceKind::Call).is_empty());
    assert!(kinds(&found, ReferenceKind::FunctionValue).is_empty());
}

#[test]
fn a_function_named_without_a_call_is_a_value() {
    let found = references("def a():\n    return b.map(helper)\n");
    assert!(kinds(&found, ReferenceKind::FunctionValue).contains(&"helper"));
}

#[test]
fn what_a_function_binds_is_no_value_and_its_call_is_bound() {
    let found = references(
        "def a(p, q=dflt, *r, s: int = 1, **t):\n    v = 1\n    for w in y:\n        pass\n    with z as u:\n        pass\n    try:\n        pass\n    except E as e:\n        pass\n    def inner():\n        pass\n    import m\n    from n import o\n    if (k := 1):\n        pass\n    (p, q, r, s, t, v, w, u, e, inner, m, o, k, dflt, y, z, E)\n    p()\n    other()\n",
    );
    let values = kinds(&found, ReferenceKind::FunctionValue);
    assert_eq!(values, ["dflt", "y", "z", "E", "dflt", "y", "z", "E"]);
    let calls: Vec<_> = found
        .iter()
        .filter(|r| r.kind == ReferenceKind::Call)
        .map(|r| (r.name.as_str(), r.call_form))
        .collect();
    assert_eq!(
        calls,
        [
            ("p", Some(CallForm::Bound)),
            ("other", Some(CallForm::Plain))
        ]
    );
}

#[test]
fn what_is_written_to_is_no_read() {
    let found = references("x = y\na.b = c\na.d += 1\nq[i] = 1\nm, n.o = 1, 2\n");
    assert_eq!(kinds(&found, ReferenceKind::FieldWrite), ["b", "d", "o"]);
    assert_eq!(kinds(&found, ReferenceKind::FieldUse), ["d"]);
    let values = kinds(&found, ReferenceKind::FunctionValue);
    assert!(values.contains(&"y"));
    assert!(values.contains(&"c"));
    assert!(values.contains(&"i"));
    assert!(!values.contains(&"x"));
    assert!(!values.contains(&"m"));
}

#[test]
fn a_keyword_is_no_reference_and_its_value_is() {
    let found = references("f(key=value)\n");
    assert_eq!(kinds(&found, ReferenceKind::FunctionValue), ["value"]);
}

#[test]
fn a_comprehension_binds_its_names() {
    let found = references("[f(x) for x in y if x]\n{k: v for k, v in z}\n");
    assert_eq!(kinds(&found, ReferenceKind::FunctionValue), ["y", "z"]);
}

#[test]
fn types_are_found_in_annotations() {
    let found = references(
        "def a(p: Shape, q: list[Item] | None = None) -> mod.Result:\n    v: Local = 1\nb: Other\ntype T = Base\nc: \"Later\"\n",
    );
    assert_eq!(
        kinds(&found, ReferenceKind::TypeUse),
        ["Shape", "list", "Item", "Result", "Local", "Other", "Base"]
    );
}

#[test]
fn a_class_says_what_it_inherits() {
    let found = references("class A(B, mod.C, Generic[T], metaclass=M):\n    pass\n");
    let extends: Vec<_> = found
        .iter()
        .filter(|r| r.context.as_deref() == Some("extends"))
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(extends, ["B", "C", "Generic"]);
    assert!(kinds(&found, ReferenceKind::TypeUse).contains(&"T"));
    assert_eq!(kinds(&found, ReferenceKind::FunctionValue), ["M"]);
}

#[test]
fn imports_say_their_module() {
    let found = references(
        "import a.b\nimport c as d\nfrom .e import f, g as h\nfrom . import i\nfrom ..j.k import l\nfrom m import *\n",
    );
    let imports: Vec<_> = found
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| (r.name.as_str(), r.context.as_deref().unwrap(), r.call_form))
        .collect();
    let bound = Some(CallForm::Bound);
    assert_eq!(
        imports,
        [
            ("a.b", "a.b", bound),
            ("c", "c", bound),
            ("f", ".e", None),
            (".e.f", ".e.f", bound),
            ("g", ".e", bound),
            (".e.g", ".e.g", bound),
            ("i", ".", None),
            (".i", ".i", bound),
            ("l", "..j.k", None),
            ("..j.k.l", "..j.k.l", bound),
        ]
    );
}

#[test]
fn a_string_holds_the_calls_of_an_f_string() {
    let found = references("x = f\"{run(1)} and text()\"\n");
    assert_eq!(kinds(&found, ReferenceKind::Call), ["run"]);
}
