//! A daemon nobody asks anything stops by itself, and frees the project's socket.

use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread")]
async fn test_stops_when_idle() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    let socket = mimir_protocol::socket_path(&root).unwrap();

    let served = mimir_daemon::run(
        root,
        Duration::from_millis(200),
        Arc::new(mimir_resolve::NoSemanticSource),
    );
    tokio::time::timeout(Duration::from_secs(20), served)
        .await
        .expect("the daemon did not stop")
        .unwrap();
    assert!(!socket.exists());
}

/// A test that fails, or anyone removing a project, leaves no daemon behind for half an hour.
#[tokio::test(flavor = "multi_thread")]
async fn test_stops_when_its_project_is_gone() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    let socket = mimir_protocol::socket_path(&root).unwrap();

    let served = tokio::spawn(mimir_daemon::run(
        root.clone(),
        Duration::from_secs(3600),
        Arc::new(mimir_resolve::NoSemanticSource),
    ));
    while !socket.exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(project);

    tokio::time::timeout(Duration::from_secs(20), served)
        .await
        .expect("the daemon did not stop")
        .unwrap()
        .unwrap();
    assert!(!socket.exists());
}
