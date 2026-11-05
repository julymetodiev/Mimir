use super::*;
use crate::ScriptAdapter;
use latt_parse_core::LanguageAdapter;
use latt_types::FileId;

fn references(adapter: ScriptAdapter, source: &str) -> Vec<ExtractedReference> {
    let tree = adapter.parse(source).unwrap();
    adapter.extract_references(&tree, FileId::new(1))
}

fn ts(source: &str) -> Vec<ExtractedReference> {
    references(ScriptAdapter::typescript(), source)
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
    let found = ts(
        "function a() {\n  obj.run(1);\n  f();\n  new Box();\n  new x.Y();\n  o.p.q();\n  g`t`;\n}\n",
    );
    assert_eq!(
        kinds(&found, ReferenceKind::Call),
        ["run", "f", "Box", "Y", "q", "g"]
    );
    let run = found.iter().find(|r| r.name == "run").unwrap();
    assert_eq!((run.range.start_line, run.range.start_col), (2, 6));
    assert_eq!(run.call_form, Some(CallForm::Method));
    assert_eq!(run.context.as_deref(), Some("obj"));
}

#[test]
fn a_computed_call_has_no_name() {
    let found = ts("obj[name]();\n");
    assert!(kinds(&found, ReferenceKind::Call).is_empty(), "{found:?}");
    // What computes it is still used.
    assert_eq!(kinds(&found, ReferenceKind::FunctionValue), ["obj", "name"]);
}

#[test]
fn a_name_bound_nearer_is_marked() {
    let found = ts("function a(p) {\n  const q = 1;\n  p();\n  q();\n  r();\n}\n");
    let form = |name: &str| {
        found
            .iter()
            .find(|r| r.name == name && r.kind == ReferenceKind::Call)
            .and_then(|r| r.call_form)
    };
    assert_eq!(form("p"), Some(CallForm::Bound));
    assert_eq!(form("q"), Some(CallForm::Bound));
    assert_eq!(form("r"), Some(CallForm::Plain));
}

#[test]
fn a_nested_function_does_not_bind_for_the_one_around_it() {
    let found = ts("function a() {\n  function b() { const c = 1; }\n  c();\n  b();\n}\n");
    let form = |name: &str| {
        found
            .iter()
            .find(|r| r.name == name && r.kind == ReferenceKind::Call)
            .and_then(|r| r.call_form)
    };
    assert_eq!(form("c"), Some(CallForm::Plain));
    assert_eq!(form("b"), Some(CallForm::Bound));
}

#[test]
fn values_are_not_their_declarations() {
    let found =
        ts("const f = g;\nfunction h(a = dflt, { b, c: d } = {}) { use(a, f, { x, y: z }); }\n");
    let values = kinds(&found, ReferenceKind::FunctionValue);
    for name in ["g", "dflt", "f", "x", "z"] {
        assert!(values.contains(&name), "{name} in {values:?}");
    }
    for name in ["h", "a", "b", "c", "d", "y"] {
        assert!(!values.contains(&name), "{name} in {values:?}");
    }
}

#[test]
fn imports_name_what_they_bring_and_from_where() {
    let found = ts(
        "import a, { b, c as d } from './m';\nimport * as ns from 'pkg';\nexport { e } from './n';\n",
    );
    let imports: Vec<_> = found
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| (r.name.as_str(), r.context.as_deref().unwrap()))
        .collect();
    assert_eq!(
        imports,
        [
            ("a", "./m"),
            ("b", "./m"),
            ("c", "./m"),
            ("ns", "pkg"),
            ("e", "./n")
        ]
    );
}

#[test]
fn require_is_an_import() {
    let found = references(
        ScriptAdapter::javascript(),
        "const a = require('./a');\nconst { b, c: d } = require('./b');\n",
    );
    let imports: Vec<_> = found
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| (r.name.as_str(), r.context.as_deref().unwrap()))
        .collect();
    assert_eq!(imports, [("a", "./a"), ("b", "./b"), ("d", "./b")]);
    assert!(kinds(&found, ReferenceKind::Call).is_empty(), "{found:?}");
}

#[test]
fn a_field_is_read_written_or_both() {
    let found = ts("a.x;\na.y = 1;\na.z += 1;\nthis.w();\n");
    assert_eq!(kinds(&found, ReferenceKind::FieldUse), ["x", "z"]);
    assert_eq!(kinds(&found, ReferenceKind::FieldWrite), ["y", "z"]);
    assert_eq!(kinds(&found, ReferenceKind::Call), ["w"]);
}

#[test]
fn types_are_uses_and_their_declarations_are_not() {
    let found = ts(
        "interface A { b: C; }\ntype D = E.F;\nclass G extends H implements I { j(k: L): M { return 1; } }\n",
    );
    let types = kinds(&found, ReferenceKind::TypeUse);
    for name in ["C", "F", "H", "I", "L", "M"] {
        assert!(types.contains(&name), "{name} in {types:?}");
    }
    for name in ["A", "D", "G"] {
        assert!(!types.contains(&name), "{name} in {types:?}");
    }
    let context = |name: &str| {
        found
            .iter()
            .find(|r| r.name == name)
            .and_then(|r| r.context.as_deref())
    };
    assert_eq!(context("H"), Some("extends"));
    assert_eq!(context("I"), Some("implements"));
}

#[test]
fn a_component_is_a_value_and_an_element_is_not() {
    let found = references(
        ScriptAdapter::tsx(),
        "const a = <Foo x={y}><div /><ui.Bar /></Foo>;\n",
    );
    let values = kinds(&found, ReferenceKind::FunctionValue);
    assert!(values.contains(&"Foo"), "{values:?}");
    assert!(values.contains(&"Bar"), "{values:?}");
    assert!(values.contains(&"y"), "{values:?}");
    assert!(!values.contains(&"div"), "{values:?}");
    assert_eq!(values.iter().filter(|n| **n == "Foo").count(), 1);
}

#[test]
fn a_decorator_is_a_call() {
    let found = ts("@one\nclass A { @two() run() {} }\n");
    assert_eq!(kinds(&found, ReferenceKind::Call), ["one", "two"]);
}

#[test]
fn a_very_deep_expression_does_not_end_the_process() {
    let source = format!("const a = {}1;\n", "1 + ".repeat(50_000));
    ts(&source);
}
