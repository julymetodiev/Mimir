//! A long sync does not stop the answers.

use mimir_engine::start;
use mimir_resolve::{NoSemanticSource, ProjectFile, Refresh, ScipIndex, SemanticSource};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tempfile::TempDir;

/// A semantic source whose new index takes until it is let go.
#[derive(Default)]
struct HeldIndex {
    released: Mutex<bool>,
    let_go: Condvar,
}

impl HeldIndex {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.let_go.notify_all();
    }
}

impl SemanticSource for HeldIndex {
    fn load(&self, _: &Path, _: &Path, _: &[ProjectFile], refresh: Refresh) -> Vec<ScipIndex> {
        if matches!(refresh, Refresh::Full) {
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.let_go.wait(released).unwrap();
            }
        }
        Vec::new()
    }

    fn stop_if_idle(&self, _: Duration) {}
}

#[tokio::test(flavor = "multi_thread")]
async fn test_queries_are_answered_during_a_sync_and_the_sync_still_ends() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("a.rs"),
        "fn caller() { callee(); }\nfn callee() {}\n",
    )
    .unwrap();

    let source = Arc::new(HeldIndex::default());
    let engine = start(root.clone(), source.clone()).await;
    let handle = engine.handle().clone();
    handle.stats().await.unwrap();

    let syncing = tokio::spawn({
        let handle = handle.clone();
        async move { handle.sync_complete(true).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Every kind of question answers although the index is held.
    let wait = Duration::from_secs(10);
    let found = tokio::time::timeout(wait, handle.find_symbol("callee"))
        .await
        .expect("a search waited for the sync")
        .unwrap();
    let trace = tokio::time::timeout(wait, handle.trace(found.symbols[0].id))
        .await
        .expect("a trace waited for the sync")
        .unwrap();
    assert!(trace.provisional, "the sync is still going");
    assert_eq!(
        trace.callers.len(),
        1,
        "edges stay as syntax rules give them"
    );
    assert!(!syncing.is_finished());

    source.release();
    let result = tokio::time::timeout(wait, syncing)
        .await
        .expect("the sync never ended")
        .unwrap()
        .unwrap();
    assert!(result.link.is_some());
    assert!(result.saved_to.is_some());
    let found = handle.find_symbol("callee").await.unwrap();
    let trace = handle.trace(found.symbols[0].id).await.unwrap();
    assert!(!trace.provisional);
}

/// A sync with nothing slow in it answers as before.
#[tokio::test]
async fn test_a_sync_without_a_semantic_source_is_answered() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let engine = start(root, Arc::new(NoSemanticSource)).await;

    let result = engine.handle().sync_complete(false).await.unwrap();

    assert!(result.saved_to.is_some());
}
