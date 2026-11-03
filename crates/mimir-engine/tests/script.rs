//! TypeScript and JavaScript from syntax alone: what a form allows is linked, and nothing else.

use mimir_engine::{Engine, GraphHandle, start};
use mimir_parse_core::LanguageAdapter;
use mimir_parse_rust::RustAdapter;
use mimir_parse_ts::ScriptAdapter;
use mimir_resolve::NoSemanticSource;
use std::collections::HashSet;
use std::sync::Arc;
use tempfile::TempDir;

async fn project(files: &[(&str, &str)]) -> (TempDir, Engine) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let languages: Vec<Arc<dyn LanguageAdapter>> = vec![
        Arc::new(RustAdapter::new()),
        Arc::new(ScriptAdapter::typescript()),
        Arc::new(ScriptAdapter::tsx()),
        Arc::new(ScriptAdapter::javascript()),
    ];
    let engine = start(root, languages, Arc::new(NoSemanticSource)).await;
    engine.handle().stats().await.unwrap();
    (dir, engine)
}

async fn callees(handle: &GraphHandle, name: &str) -> HashSet<String> {
    let id = handle.find_symbol(name).await.unwrap().symbols[0].id;
    let trace = handle.trace(id).await.unwrap();
    trace.callees.into_iter().map(|c| c.symbol.name).collect()
}

fn names(list: &[&str]) -> HashSet<String> {
    list.iter().map(|n| n.to_string()).collect()
}

#[tokio::test]
async fn a_call_links_to_this_file_or_a_relative_import() {
    let (_dir, engine) = project(&[
        (
            "src/a.ts",
            "import { shared, renamed as other } from './b';\n\
             import { fromPackage } from 'pkg';\n\
             import fallback from './b';\n\
             function local() {}\n\
             export function caller(p) {\n\
               local();\n\
               shared();\n\
               other();\n\
               fromPackage();\n\
               fallback();\n\
               p();\n\
               obj.local();\n\
             }\n",
        ),
        (
            "src/b.ts",
            "export function shared() {}\n\
             export function renamed() {}\n\
             export function fallback() {}\n\
             function hidden() {}\n",
        ),
    ])
    .await;
    assert_eq!(
        callees(engine.handle(), "caller").await,
        names(&["local", "shared"])
    );
}

#[tokio::test]
async fn a_name_in_another_language_is_never_a_target() {
    let (_dir, engine) = project(&[
        ("a.ts", "export function caller() { only_in_rust(); }\n"),
        (
            "b.rs",
            "fn only_in_rust() {}\nfn rust_caller() { only_in_ts(); }\n",
        ),
        ("c.ts", "export function only_in_ts() {}\n"),
    ])
    .await;
    assert!(callees(engine.handle(), "caller").await.is_empty());
    assert!(callees(engine.handle(), "rust_caller").await.is_empty());
}

#[tokio::test]
async fn javascript_and_typescript_link_to_each_other() {
    let (_dir, engine) = project(&[
        (
            "a.js",
            "import { helper } from './b.js';\nexport function caller() { helper(); }\n",
        ),
        ("b.ts", "export function helper() {}\n"),
    ])
    .await;
    assert_eq!(callees(engine.handle(), "caller").await, names(&["helper"]));
}
