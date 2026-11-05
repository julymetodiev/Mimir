//! A long sync does not stop the answers.

use mimir_engine::start;
use mimir_protocol::{ActivityKind, GraphEvent, SnapshotEdgeKind, TraceOptions};
use mimir_resolve::{NoSemanticSource, ProjectFile, Refresh, SemanticIndex, SemanticSource};
use mimir_types::Language;
use std::path::{Path, PathBuf};
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
    fn load(&self, _: &Path, _: &Path, _: &[ProjectFile], refresh: Refresh) -> Vec<SemanticIndex> {
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
    let engine = start(
        root.clone(),
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        source.clone(),
    )
    .await;
    let handle = engine.handle().clone();
    handle.stats().await.unwrap();

    let syncing = tokio::spawn({
        let handle = handle.clone();
        async move { handle.sync_complete(true, true).await }
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

/// A file that changes while a sync waits for its index is in what the sync ends with. Resolving
/// the files as they were would lose every edge of the changed one.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_file_changed_during_a_sync_keeps_its_edges() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn caller() { callee(); }\n").unwrap();
    std::fs::write(root.join("b.rs"), "fn callee() {}\n").unwrap();

    let source = Arc::new(HeldIndex::default());
    let engine = start(
        root.clone(),
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        source.clone(),
    )
    .await;
    let handle = engine.handle().clone();
    handle.stats().await.unwrap();

    let syncing = tokio::spawn({
        let handle = handle.clone();
        async move { handle.sync_complete(true, true).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle
        .write_file(
            root.join("a.rs"),
            "// changed\nfn caller() { callee(); }\n".to_string(),
            true,
        )
        .await
        .unwrap();
    source.release();

    let result = tokio::time::timeout(Duration::from_secs(10), syncing)
        .await
        .expect("the sync never ended")
        .unwrap()
        .unwrap();
    let saved = std::fs::read_to_string(result.saved_to.unwrap()).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved["edges"].as_array().unwrap().len(), 1, "{saved}");
}

/// A file reported again with the same text changes nothing, so it starts no new resolution.
#[tokio::test]
async fn test_a_file_with_the_same_text_keeps_its_symbols() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let engine = start(
        root.clone(),
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        Arc::new(NoSemanticSource),
    )
    .await;
    let handle = engine.handle();
    let before = handle.find_symbol("a").await.unwrap().symbols[0].id;

    let path = root.join("a.rs");
    handle
        .add_file(path, "fn a() {}\n".to_string(), Language::Rust)
        .await
        .unwrap();

    assert_eq!(handle.find_symbol("a").await.unwrap().symbols[0].id, before);
}

/// An id means the same symbol after a restart and after an edit, and an edit moves only what it
/// names: a second function of the same name keeps its own id.
#[tokio::test]
async fn test_ids_last_through_a_restart_and_an_edit() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let text = "mod m { fn a() {} }\nfn b() {}\nfn b() {}\n";
    std::fs::write(root.join("x.rs"), text).unwrap();

    let ids = async |handle: &mimir_engine::GraphHandle, name: &str| {
        let found = handle.find_symbol(name).await.unwrap().symbols;
        let mut ids: Vec<_> = found.iter().map(|symbol| symbol.id.as_u64()).collect();
        ids.sort_unstable();
        ids
    };
    let start_engine = async || {
        start(
            root.clone(),
            vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
            Arc::new(NoSemanticSource),
        )
        .await
    };

    let first = start_engine().await;
    first.handle().stats().await.unwrap();
    let (a, b) = (
        ids(first.handle(), "a").await,
        ids(first.handle(), "b").await,
    );
    assert_eq!((a.len(), b.len()), (1, 2));
    assert_ne!(b[0], b[1], "the same name twice in a file");
    first.handle().shutdown().await.unwrap();

    let second = start_engine().await;
    let handle = second.handle();
    handle.stats().await.unwrap();
    assert_eq!(ids(handle, "a").await, a, "after a restart");
    assert_eq!(ids(handle, "b").await, b, "after a restart");

    handle
        .write_file(
            root.join("x.rs"),
            format!("// new\n{text}\nfn c() {{}}\n"),
            true,
        )
        .await
        .unwrap();
    assert_eq!(ids(handle, "a").await, a, "after an edit");
    assert_eq!(ids(handle, "b").await, b, "after an edit");
}

/// A semantic source that counts how many links asked it.
#[derive(Default)]
struct CountedSource(Mutex<usize>);

impl SemanticSource for CountedSource {
    fn load(&self, _: &Path, _: &Path, _: &[ProjectFile], _: Refresh) -> Vec<SemanticIndex> {
        *self.0.lock().unwrap() += 1;
        Vec::new()
    }

    fn stop_if_idle(&self, _: Duration) {}
}

async fn callers_of(handle: &mimir_engine::GraphHandle, name: &str) -> Vec<String> {
    let options = TraceOptions {
        callers_only: true,
        ..Default::default()
    };
    let trace = handle
        .trace_by_name(name.to_string(), options)
        .await
        .unwrap();
    trace
        .callers
        .iter()
        .map(|node| node.symbol.name.clone())
        .collect()
}

/// A start with nothing changed answers from the file, edges too, with no link and nothing parsed.
/// A file changed while stopped is seen, and the first link then has every file.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_start_answers_from_the_file() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn caller() { b::callee(); }\n").unwrap();
    std::fs::write(root.join("b.rs"), "pub fn callee() {}\npub fn other() {}\n").unwrap();
    let begin = async |source: Arc<CountedSource>| {
        start(
            root.clone(),
            vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
            source,
        )
        .await
    };

    let first = begin(Arc::default()).await;
    first.handle().sync_complete(false, false).await.unwrap();
    assert_eq!(callers_of(first.handle(), "callee").await, ["caller"]);
    first.handle().shutdown().await.unwrap();
    assert!(root.join(".mimir/graph/graph.fb").exists());

    let source = Arc::new(CountedSource::default());
    let second = begin(source.clone()).await;
    assert_eq!(callers_of(second.handle(), "callee").await, ["caller"]);
    assert_eq!(*source.0.lock().unwrap(), 0, "nothing changed: no link");
    let read = second
        .handle()
        .read("callee".to_string(), None, 0, 0)
        .await
        .unwrap();
    assert!(
        read.lines.join("\n").contains("fn callee"),
        "text from the disk"
    );
    second.handle().shutdown().await.unwrap();

    // Another size, so this is seen by size alone. `a.rs`, which did not change, keeps its edge.
    std::fs::write(
        root.join("b.rs"),
        "pub fn callee() {}\npub fn other() { callee(); }\n",
    )
    .unwrap();
    let source = Arc::new(CountedSource::default());
    let third = begin(source.clone()).await;
    let mut callers = callers_of(third.handle(), "callee").await;
    callers.sort();
    assert_eq!(callers, ["caller", "other"]);
    assert_eq!(*source.0.lock().unwrap(), 1, "one link");
    third.handle().shutdown().await.unwrap();

    // Broken file: as if none, and it is written again.
    std::fs::write(root.join(".mimir/graph/graph.fb"), b"not a graph").unwrap();
    let fourth = begin(Arc::default()).await;
    let mut callers = callers_of(fourth.handle(), "callee").await;
    callers.sort();
    assert_eq!(callers, ["caller", "other"]);
    fourth.handle().sync_complete(false, false).await.unwrap();
    fourth.handle().shutdown().await.unwrap();
    assert!(
        std::fs::metadata(root.join(".mimir/graph/graph.fb"))
            .unwrap()
            .len()
            > 100
    );
}

/// A bare name with several symbols means the one most called, wherever its file comes in the order.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_name_means_the_symbol_most_called() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn same() {}\n").unwrap();
    std::fs::write(
        root.join("z.rs"),
        "fn same() {}\nfn one() { same(); }\nfn two() { same(); }\n",
    )
    .unwrap();
    let engine = start(
        root.clone(),
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        Arc::new(NoSemanticSource),
    )
    .await;
    let handle = engine.handle();
    handle.sync_complete(false, false).await.unwrap();
    let trace = handle
        .trace_by_name("same".to_string(), TraceOptions::default())
        .await
        .unwrap();
    assert_eq!(trace.root.file_path, Path::new("z.rs"));
    assert_eq!(trace.matches, 2);
}

/// A sync with nothing slow in it answers as before.
#[tokio::test]
async fn test_a_sync_without_a_semantic_source_is_answered() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let engine = start(
        root,
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        Arc::new(NoSemanticSource),
    )
    .await;

    let result = engine.handle().sync_complete(false, true).await.unwrap();

    assert!(result.saved_to.is_some());
}

/// A watcher hears that files changed, and which symbols a query touched; a snapshot shows the graph
/// and rises in generation as edges are resolved anew.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_watcher_hears_changes_and_activity() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("a.rs"),
        "fn caller() { callee(); }\nfn callee() {}\n",
    )
    .unwrap();
    let engine = start(
        root.clone(),
        vec![Arc::new(mimir_parse_rust::RustAdapter::new())],
        Arc::new(NoSemanticSource),
    )
    .await;
    let handle = engine.handle().clone();
    handle.stats().await.unwrap();

    let snapshot = handle.snapshot().await.unwrap();
    assert_eq!(snapshot.files, [PathBuf::from("a.rs")]);
    assert_eq!(snapshot.symbols.len(), 2);
    assert_eq!(snapshot.edges.len(), 1);
    assert_eq!(snapshot.edges[0].kind, SnapshotEdgeKind::Call);
    let before = snapshot.generation;

    let mut events = handle.watch().await.unwrap();
    let wait = Duration::from_secs(10);

    handle
        .trace_by_name("callee".to_string(), TraceOptions::default())
        .await
        .unwrap();
    let event = tokio::time::timeout(wait, events.recv())
        .await
        .unwrap()
        .unwrap();
    let GraphEvent::Activity { symbols, kind } = event else {
        panic!("{event:?}");
    };
    assert_eq!(kind, ActivityKind::Read);
    assert_eq!(symbols.len(), 2, "the symbol and its caller");

    handle
        .write_file(
            root.join("a.rs"),
            "fn caller() { callee(); }\nfn callee() {}\nfn more() {}\n".to_string(),
            true,
        )
        .await
        .unwrap();
    let mut told = false;
    let mut activity = false;
    while !(told && activity) {
        match tokio::time::timeout(wait, events.recv())
            .await
            .unwrap()
            .unwrap()
        {
            GraphEvent::GraphChanged { .. } => told = true,
            GraphEvent::Activity { kind, .. } => activity = kind == ActivityKind::Edit,
            GraphEvent::Lagged => panic!("fell behind"),
        }
    }

    let snapshot = handle.snapshot().await.unwrap();
    assert_eq!(snapshot.symbols.len(), 3);
    assert!(snapshot.generation > before);
}
