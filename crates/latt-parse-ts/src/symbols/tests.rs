use super::*;
use crate::ScriptAdapter;
use latt_parse_core::LanguageAdapter;
use latt_types::FileId;

fn symbols(adapter: ScriptAdapter, source: &str) -> Vec<(String, SymbolKind)> {
    let tree = adapter.parse(source).unwrap();
    adapter
        .extract_symbols(&tree, FileId::new(1))
        .into_iter()
        .map(|s| {
            let name = match s.parent {
                Some(parent) => format!("{parent}.{}", s.name),
                None => s.name,
            };
            (name, s.kind)
        })
        .collect()
}

fn has(found: &[(String, SymbolKind)], name: &str, kind: SymbolKind) -> bool {
    found.iter().any(|(n, k)| n == name && *k == kind)
}

#[test]
fn finds_the_declarations_of_typescript() {
    let found = symbols(
        ScriptAdapter::typescript(),
        r#"
export function free(a: number) { return a; }
const arrow = (a: number) => a;
const LIMIT = 3;
let counter = 0;
export class Box<T> extends Base implements Shape {
  private size = 1;
  handler = () => 1;
  constructor(a: number) { super(); }
  get width() { return 1; }
  set width(v) {}
  run(): void {}
  #secret() {}
}
abstract class Partly { abstract area(): number; }
export interface Shape { area(): number; name: string; }
type Id = string;
enum Color { Red, Green = 2 }
namespace Tools { export function help() {} const IN = 1; }
"#,
    );
    for (name, kind) in [
        ("free", SymbolKind::Function),
        ("arrow", SymbolKind::Function),
        ("LIMIT", SymbolKind::Constant),
        ("counter", SymbolKind::Variable),
        ("Box", SymbolKind::Class),
        ("Box.size", SymbolKind::Field),
        ("Box.handler", SymbolKind::Method),
        ("Box.constructor", SymbolKind::Constructor),
        ("Box.width", SymbolKind::Method),
        ("Box.run", SymbolKind::Method),
        ("Box.#secret", SymbolKind::Method),
        ("Partly", SymbolKind::Class),
        ("Partly.area", SymbolKind::Method),
        ("Shape", SymbolKind::Interface),
        ("Shape.area", SymbolKind::Method),
        ("Shape.name", SymbolKind::Field),
        ("Id", SymbolKind::TypeAlias),
        ("Color", SymbolKind::Enum),
        ("Color.Red", SymbolKind::EnumVariant),
        ("Color.Green", SymbolKind::EnumVariant),
        ("Tools", SymbolKind::Namespace),
        ("Tools.help", SymbolKind::Function),
        ("Tools.IN", SymbolKind::Constant),
    ] {
        assert!(has(&found, name, kind), "{name} in {found:?}");
    }
}

#[test]
fn a_local_constant_is_not_a_symbol_and_a_local_function_is() {
    let found = symbols(
        ScriptAdapter::typescript(),
        "function outer() { const LOCAL = 1; const inner = () => LOCAL; function deep() {} }\n",
    );
    assert!(has(&found, "inner", SymbolKind::Function), "{found:?}");
    assert!(has(&found, "deep", SymbolKind::Function), "{found:?}");
    assert!(!found.iter().any(|(n, _)| n == "LOCAL"), "{found:?}");
}

#[test]
fn overloads_are_one_symbol_at_what_has_a_body() {
    let adapter = ScriptAdapter::typescript();
    let source =
        "function f(a: string): void;\nfunction f(a: number): void;\nfunction f(a: any) {}\n";
    let tree = adapter.parse(source).unwrap();
    let found = adapter.extract_symbols(&tree, FileId::new(1));
    assert_eq!(found.len(), 2, "the function and the module: {found:?}");
    assert_eq!(found[0].range.start_line, 3);
}

#[test]
fn signatures_without_a_body_keep_the_first() {
    let adapter = ScriptAdapter::typescript();
    let source = "declare function f(a: string): void;\ndeclare function f(a: number): void;\n";
    let tree = adapter.parse(source).unwrap();
    let found = adapter.extract_symbols(&tree, FileId::new(1));
    assert_eq!(found.len(), 2, "the function and the module: {found:?}");
    assert_eq!(found[0].range.start_line, 1);
}

#[test]
fn finds_the_functions_javascript_assigns() {
    let found = symbols(
        ScriptAdapter::javascript(),
        r#"
var app = exports = module.exports = {};
app.use = function use(fn) {};
exports.send = function () {};
Thing.prototype.get = function () {};
module.exports = function create() {};
const handlers = { run: () => 1, stop() {} };
class Old { field = 1; }
"#,
    );
    for (name, kind) in [
        ("app.use", SymbolKind::Method),
        ("send", SymbolKind::Function),
        ("Thing.get", SymbolKind::Method),
        ("create", SymbolKind::Function),
        ("handlers", SymbolKind::Constant),
        ("handlers.run", SymbolKind::Method),
        ("handlers.stop", SymbolKind::Method),
        ("Old.field", SymbolKind::Field),
    ] {
        assert!(has(&found, name, kind), "{name} in {found:?}");
    }
}

#[test]
fn an_export_is_part_of_the_declaration_and_public() {
    let adapter = ScriptAdapter::typescript();
    let source = "export const f = () => 1;\nconst g = 2;\nexport { g };\nconst h = 3;\n";
    let tree = adapter.parse(source).unwrap();
    let found = adapter.extract_symbols(&tree, FileId::new(1));
    let get = |name: &str| found.iter().find(|s| s.name == name).unwrap();
    assert_eq!(get("f").span.start, 0);
    assert_eq!(get("f").visibility, Visibility::Public);
    assert_eq!(get("g").visibility, Visibility::Public);
    assert_eq!(get("h").visibility, Visibility::Private);
}

#[test]
fn a_very_deep_expression_does_not_end_the_process() {
    let source = format!("const a = {}1;\n", "1 + ".repeat(50_000));
    let found = symbols(ScriptAdapter::typescript(), &source);
    assert!(has(&found, "a", SymbolKind::Constant));
}
