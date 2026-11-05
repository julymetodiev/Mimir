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
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        Arc::new(mimir_resolve::NoSemanticSource),
    );
    tokio::time::timeout(Duration::from_secs(20), served)
        .await
        .expect("the daemon did not stop")
        .unwrap();
    assert!(!socket.exists());
}

/// A new index that takes until the daemon stops, as one of a large project takes for over the idle time.
#[derive(Default)]
struct HeldIndex {
    stopped: std::sync::Mutex<bool>,
    let_go: std::sync::Condvar,
}

impl mimir_resolve::SemanticSource for HeldIndex {
    fn load(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
        _: &[mimir_resolve::ProjectFile],
        refresh: mimir_resolve::Refresh,
    ) -> Vec<mimir_resolve::SemanticIndex> {
        if matches!(refresh, mimir_resolve::Refresh::Full) {
            let mut stopped = self.stopped.lock().unwrap();
            while !*stopped {
                stopped = self.let_go.wait(stopped).unwrap();
            }
        }
        Vec::new()
    }

    fn stop_if_idle(&self, _: Duration) {}

    fn stop(&self) {
        *self.stopped.lock().unwrap() = true;
        self.let_go.notify_all();
    }
}

/// A sync longer than the idle time is use: the daemon that stopped during one lost a whole index.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_long_sync_keeps_the_daemon_and_a_stop_ends_it() {
    use mimir_protocol::{IpcCommand, IpcRequest};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    let socket = mimir_protocol::socket_path(&root).unwrap();
    let served = tokio::spawn(mimir_daemon::run(
        root.clone(),
        Duration::from_millis(200),
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        Arc::new(HeldIndex::default()),
    ));
    let connect = || async {
        loop {
            match tokio::net::UnixStream::connect(&socket).await {
                Ok(stream) => break stream,
                Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    };
    let mut syncing = connect().await;
    let sync = IpcCommand::Sync {
        full: true,
        json: false,
    };
    let request = serde_json::to_string(&IpcRequest::new(1, sync)).unwrap() + "\n";
    syncing.write_all(request.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!served.is_finished(), "stopped during a sync");

    let mut stopping = connect().await;
    let request = serde_json::to_string(&IpcRequest::new(2, IpcCommand::Shutdown)).unwrap() + "\n";
    stopping.write_all(request.as_bytes()).await.unwrap();
    let mut reader = BufReader::new(stopping);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();

    tokio::time::timeout(Duration::from_secs(20), served)
        .await
        .expect("the daemon waited for the index to end by itself")
        .unwrap()
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
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
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

/// Someone watching is using the daemon, so it does not stop as idle; when they go, it does.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_watcher_keeps_the_daemon_until_it_goes() {
    use mimir_protocol::{IpcCommand, IpcRequest};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    let socket = mimir_protocol::socket_path(&root).unwrap();
    let served = tokio::spawn(mimir_daemon::run(
        root.clone(),
        Duration::from_millis(200),
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        Arc::new(mimir_resolve::NoSemanticSource),
    ));
    // The socket's file comes a moment before it listens.
    let mut stream = loop {
        match tokio::net::UnixStream::connect(&socket).await {
            Ok(stream) => break stream,
            Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
        }
    };
    let request = serde_json::to_string(&IpcRequest::new(1, IpcCommand::Watch)).unwrap() + "\n";
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert!(line.contains("Watching"), "{line}");

    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!served.is_finished(), "stopped although someone watches");

    drop(reader);
    tokio::time::timeout(Duration::from_secs(20), served)
        .await
        .expect("the daemon did not stop")
        .unwrap()
        .unwrap();
}
