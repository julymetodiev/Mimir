//! Integration tests for mimir-engine.

use mimir_engine::{GraphActor, GraphHandle};
use mimir_engine::{command_channel, event_channel};
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_types::Language;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

fn create_test_registry() -> Arc<AdapterRegistry> {
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));
    Arc::new(registry)
}

#[tokio::test]
async fn test_full_sync_workflow() {
    // Create a temporary directory with Rust files
    let temp_dir = TempDir::new().unwrap();

    // Create src directory
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();

    // Create main.rs
    let mut main_file = std::fs::File::create(src_dir.join("main.rs")).unwrap();
    main_file
        .write_all(
            b"
fn main() {
    helper();
}

fn helper() {
    println!(\"Hello!\");
}
",
        )
        .unwrap();

    // Create lib.rs
    let mut lib_file = std::fs::File::create(src_dir.join("lib.rs")).unwrap();
    lib_file
        .write_all(
            b"
pub fn public_func() {}

fn private_func() {}

pub struct MyStruct {
    field: i32,
}

impl MyStruct {
    pub fn new() -> Self {
        Self { field: 0 }
    }
}
",
        )
        .unwrap();

    // Set up channels and actor
    let (cmd_tx, cmd_rx) = command_channel(64);
    let (event_tx, _event_rx) = event_channel(64);
    let event_tx_clone = event_tx.clone();

    let actor = GraphActor::with_registry(temp_dir.path().to_path_buf(), create_test_registry());

    // Start actor
    let actor_handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

    let handle = GraphHandle::new(cmd_tx, event_tx_clone);

    // Trigger sync
    let sync_result = handle.sync().await.unwrap();

    // Verify sync found our files
    assert_eq!(sync_result.files_added, 2, "Expected 2 files to be added");
    assert!(
        sync_result.symbols_added > 0,
        "Expected some symbols to be added"
    );

    // Verify we can find symbols
    let main_result = handle.find_symbol("main").await.unwrap();
    assert!(main_result.has_results(), "Should find 'main' symbol");

    let helper_result = handle.find_symbol("helper").await.unwrap();
    assert!(helper_result.has_results(), "Should find 'helper' symbol");

    let public_func_result = handle.find_symbol("public_func").await.unwrap();
    assert!(
        public_func_result.has_results(),
        "Should find 'public_func' symbol"
    );

    let struct_result = handle.find_symbol("MyStruct").await.unwrap();
    assert!(struct_result.has_results(), "Should find 'MyStruct' symbol");

    // Check stats
    let stats = handle.stats().await.unwrap();
    assert_eq!(stats.files_count, 2, "Expected 2 files tracked");
    assert!(stats.symbols_count >= 6, "Expected at least 6 symbols");

    // Shutdown
    handle.shutdown().await.unwrap();
    actor_handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_incremental_sync() {
    // Create a temporary directory
    let temp_dir = TempDir::new().unwrap();

    // Create initial file
    let mut file1 = std::fs::File::create(temp_dir.path().join("file1.rs")).unwrap();
    file1.write_all(b"fn original() {}").unwrap();

    // Set up channels and actor
    let (cmd_tx, cmd_rx) = command_channel(64);
    let (event_tx, _event_rx) = event_channel(64);
    let event_tx_clone = event_tx.clone();

    let actor = GraphActor::with_registry(temp_dir.path().to_path_buf(), create_test_registry());

    let actor_handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

    let handle = GraphHandle::new(cmd_tx, event_tx_clone);

    // First sync
    let first_sync = handle.sync().await.unwrap();
    assert_eq!(first_sync.files_added, 1);
    assert_eq!(first_sync.symbols_added, 1);

    // Add a new file
    let mut file2 = std::fs::File::create(temp_dir.path().join("file2.rs")).unwrap();
    file2.write_all(b"fn new_func() {}").unwrap();

    // Second sync should find the new file
    let second_sync = handle.sync().await.unwrap();
    assert_eq!(second_sync.files_added, 1);
    assert_eq!(second_sync.files_updated, 0);
    assert_eq!(second_sync.files_removed, 0);

    // Modify the first file
    let mut file1 = std::fs::File::create(temp_dir.path().join("file1.rs")).unwrap();
    file1
        .write_all(b"fn modified() {}\nfn another() {}")
        .unwrap();

    // Third sync should detect the modification
    let third_sync = handle.sync().await.unwrap();
    assert_eq!(third_sync.files_added, 0);
    assert_eq!(third_sync.files_updated, 1);

    // Delete a file
    std::fs::remove_file(temp_dir.path().join("file2.rs")).unwrap();

    // Fourth sync should detect the removal
    let fourth_sync = handle.sync().await.unwrap();
    assert_eq!(fourth_sync.files_removed, 1);

    // Shutdown
    handle.shutdown().await.unwrap();
    actor_handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_add_file_directly() {
    let (cmd_tx, cmd_rx) = command_channel(64);
    let (event_tx, mut event_rx) = event_channel(64);
    let event_tx_clone = event_tx.clone();

    let actor = GraphActor::with_registry(PathBuf::from("/tmp"), create_test_registry());

    let actor_handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

    let handle = GraphHandle::new(cmd_tx, event_tx_clone);

    // Add file directly
    let file_id = handle
        .add_file(
            PathBuf::from("test.rs"),
            "fn test_fn() {}\nstruct TestStruct {}".to_string(),
            Language::Rust,
        )
        .await
        .unwrap();

    // Check we received the event
    let event = event_rx.recv().await.unwrap();
    assert!(matches!(event, mimir_engine::GraphEvent::FileAdded { .. }));

    // Find the symbols
    let fn_result = handle.find_symbol("test_fn").await.unwrap();
    assert!(fn_result.has_results());

    let struct_result = handle.find_symbol("TestStruct").await.unwrap();
    assert!(struct_result.has_results());

    // Remove the file
    let removed = handle.remove_file(file_id).await.unwrap();
    assert!(removed);

    // Symbols should be gone
    let fn_result = handle.find_symbol("test_fn").await.unwrap();
    assert!(!fn_result.has_results());

    // Shutdown
    handle.shutdown().await.unwrap();
    actor_handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_trace_symbols() {
    let (cmd_tx, cmd_rx) = command_channel(64);
    let (event_tx, _event_rx) = event_channel(64);
    let event_tx_clone = event_tx.clone();

    let actor = GraphActor::with_registry(PathBuf::from("/tmp"), create_test_registry());

    let actor_handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

    let handle = GraphHandle::new(cmd_tx, event_tx_clone);

    // Add file with function calls
    handle
        .add_file(
            PathBuf::from("caller.rs"),
            r#"
fn caller() {
    callee();
}
fn callee() {}
"#
            .to_string(),
            Language::Rust,
        )
        .await
        .unwrap();

    // Find the caller symbol
    let find_result = handle.find_symbol("caller").await.unwrap();
    assert!(find_result.has_results());

    let caller_id = find_result.symbols[0].id;

    // Trace from caller
    let trace_result = handle.trace(caller_id).await.unwrap();

    // The trace should include the root symbol
    assert_eq!(
        trace_result.root.id, caller_id,
        "Root should be the caller symbol"
    );
    assert_eq!(trace_result.root.name, "caller");

    // Shutdown
    handle.shutdown().await.unwrap();
    actor_handle.await.unwrap().unwrap();
}

/// An edit reported by the file watcher must not cost callers in other files.
/// Callers once semantic resolution has finished.
async fn settled_callers(handle: &GraphHandle, name: &str) -> Vec<String> {
    let found = handle.find_symbol(name).await.unwrap();
    for _ in 0..600 {
        let trace = handle.trace(found.symbols[0].id).await.unwrap();
        if !trace.provisional {
            return trace
                .callers
                .iter()
                .map(|c| c.symbol.name.clone())
                .collect();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("semantic resolution did not finish");
}

/// A query does not wait for a new index: it gets what syntax rules give, marked as such,
/// and a later query gets the rest.
#[tokio::test]
async fn test_query_answers_before_the_index_is_done() {
    if std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipped: rust-analyzer is not installed");
        return;
    }
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/resolve-workspace");
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path().canonicalize().unwrap();
    for file in [
        "Cargo.toml",
        "alpha/Cargo.toml",
        "alpha/src/lib.rs",
        "beta/Cargo.toml",
        "beta/src/lib.rs",
    ] {
        let target = project.join(file);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(file), target).unwrap();
    }
    let (cmd_tx, cmd_rx) = command_channel(32);
    let (event_tx, _event_rx) = event_channel(32);
    let actor = GraphActor::with_registry(project.clone(), create_test_registry())
        .with_query_wait(std::time::Duration::ZERO);
    tokio::spawn(actor.run(cmd_rx, event_tx.clone()));
    let handle = GraphHandle::new(cmd_tx, event_tx);
    handle.sync().await.unwrap();

    let found = handle.find_symbol("load_twice").await.unwrap();
    let trace = handle.trace(found.symbols[0].id).await.unwrap();
    assert!(
        trace.provisional,
        "an index takes longer than no wait at all"
    );
    // `store.load_twice()` needs types, so syntax leaves it out rather than guess.
    assert!(trace.callers.is_empty());

    assert_eq!(
        settled_callers(&handle, "load_twice").await,
        ["through_trait"]
    );
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_edit_keeps_callers_in_other_files() {
    if std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipped: rust-analyzer is not installed");
        return;
    }
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/resolve-workspace");
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path().canonicalize().unwrap();
    for file in [
        "Cargo.toml",
        "alpha/Cargo.toml",
        "alpha/src/lib.rs",
        "beta/Cargo.toml",
        "beta/src/lib.rs",
    ] {
        let target = project.join(file);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(file), target).unwrap();
    }
    let (cmd_tx, cmd_rx) = command_channel(32);
    let (event_tx, _event_rx) = event_channel(32);
    let actor = GraphActor::with_registry(project.clone(), create_test_registry());
    tokio::spawn(actor.run(cmd_rx, event_tx.clone()));
    let handle = GraphHandle::new(cmd_tx, event_tx);
    handle.sync().await.unwrap();

    let callers = async |handle: &GraphHandle| settled_callers(handle, "load_twice").await;
    assert_eq!(callers(&handle).await, ["through_trait"]);

    let path = project.join("alpha/src/lib.rs");
    let content = std::fs::read_to_string(&path).unwrap() + "\n// edited\n";
    std::fs::write(&path, &content).unwrap();
    handle
        .add_file(path, content, Language::Rust)
        .await
        .unwrap();

    assert_eq!(callers(&handle).await, ["through_trait"]);
    handle.shutdown().await.unwrap();
}
