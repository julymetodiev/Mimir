//! A daemon nobody asks anything stops by itself, and frees the project's socket.

use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread")]
async fn test_stops_when_idle() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    let socket = mimir_protocol::socket_path(&root).unwrap();

    let served = mimir_daemon::run(root, Duration::from_millis(200));
    tokio::time::timeout(Duration::from_secs(20), served)
        .await
        .expect("the daemon did not stop")
        .unwrap();
    assert!(!socket.exists());
}
