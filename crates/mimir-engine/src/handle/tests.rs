//! Tests of this module.

use super::*;
use crate::actor::GraphActor;
use crate::channel::command_channel;
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use std::sync::Arc;

fn create_test_registry() -> Arc<AdapterRegistry> {
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));
    Arc::new(registry)
}

#[tokio::test]
async fn test_handle_add_and_find() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let actor = GraphActor::with_registry(
        PathBuf::from("/tmp/test"),
        create_test_registry(),
        Arc::new(mimir_resolve::NoSemanticSource),
    );

    let actor_handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    let handle = GraphHandle::new(cmd_tx);

    // Add file
    let file_id = handle
        .add_file(
            PathBuf::from("test.rs"),
            "fn my_func() {}".to_string(),
            Language::Rust,
        )
        .await
        .unwrap();

    assert_eq!(file_id, FileId::new(1));

    // Find symbol
    let response = handle.find_symbol("my_func").await.unwrap();
    assert!(response.has_results());

    // Shutdown
    handle.shutdown().await.unwrap();
    actor_handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_handle_stats() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let actor = GraphActor::with_registry(
        PathBuf::from("/tmp/test"),
        create_test_registry(),
        Arc::new(mimir_resolve::NoSemanticSource),
    );

    let actor_handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    let handle = GraphHandle::new(cmd_tx);

    // Initial stats - files_count is not tracked in current impl
    let stats = handle.stats().await.unwrap();
    assert_eq!(stats.symbols_count, 0);

    // Add file and check stats
    handle
        .add_file(
            PathBuf::from("test.rs"),
            "fn a() {} fn b() {}".to_string(),
            Language::Rust,
        )
        .await
        .unwrap();

    let stats = handle.stats().await.unwrap();
    assert_eq!(stats.symbols_count, 2);

    handle.shutdown().await.unwrap();
    actor_handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_handle_clone() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let actor = GraphActor::with_registry(
        PathBuf::from("/tmp/test"),
        create_test_registry(),
        Arc::new(mimir_resolve::NoSemanticSource),
    );

    let actor_handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    let handle1 = GraphHandle::new(cmd_tx);
    let handle2 = handle1.clone();

    // Both handles should work
    handle1
        .add_file(
            PathBuf::from("file1.rs"),
            "fn a() {}".to_string(),
            Language::Rust,
        )
        .await
        .unwrap();

    handle2
        .add_file(
            PathBuf::from("file2.rs"),
            "fn b() {}".to_string(),
            Language::Rust,
        )
        .await
        .unwrap();

    let stats = handle1.stats().await.unwrap();
    assert_eq!(stats.symbols_count, 2); // 1 symbol per file

    handle1.shutdown().await.unwrap();
    actor_handle.await.unwrap().unwrap();
}
