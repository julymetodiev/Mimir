use super::*;
use crate::channel::command_channel;
use latt_parse_rust::RustAdapter;
use latt_protocol::SearchOptions;
use latt_types::Language;

fn create_test_registry() -> Arc<AdapterRegistry> {
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));
    Arc::new(registry)
}

/// Names of the symbols a search with this kind filter finds in a small file, sorted.
async fn names_by_kind(kind: &str) -> Vec<String> {
    let source = "pub struct Point { x: i32 }\nenum Color { Red, Green }\nfn area(p: i32) -> i32 { p }\nstruct Wrap<T> { t: T }\n";
    let (cmd_tx, cmd_rx) = command_channel(16);
    let dir = tempfile::TempDir::new().unwrap();
    let actor = GraphActor::with_registry(
        dir.path().to_path_buf(),
        create_test_registry(),
        Arc::new(latt_resolve_ra::RustAnalyzer::default()),
    );
    let handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::AddFile {
            path: PathBuf::from("kinds.rs"),
            content: source.to_string(),
            language: Language::Rust,
            reply: reply_tx,
        })
        .await
        .unwrap();
    reply_rx.await.unwrap().unwrap();

    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::Search {
            options: SearchOptions {
                pattern: ".".to_string(),
                kind_filter: Some(kind.to_string()),
                limit: Some(100),
            },
            reply: reply_tx,
        })
        .await
        .unwrap();
    let response = reply_rx.await.unwrap().unwrap();

    cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
    handle.await.unwrap().unwrap();

    let mut names: Vec<String> = response.symbols.into_iter().map(|s| s.name).collect();
    names.sort();
    names
}

#[test]
fn a_method_is_found_by_its_type_or_trait_without_the_impl() {
    use crate::actor::queries::ends_with;
    let name = |segments: &[&str]| segments.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert!(ends_with(
        &name(&["impl Known<'a>", "add_go"]),
        "Known::add_go"
    ));
    let in_trait = name(&["impl LanguageAdapter for GoAdapter", "qualify"]);
    assert!(ends_with(&in_trait, "GoAdapter::qualify"));
    assert!(ends_with(&in_trait, "LanguageAdapter::qualify"));
    // Go has the directory first.
    let in_go = name(&["shapes", "Circle", "Area"]);
    assert!(ends_with(&in_go, "Circle::Area"));
    assert!(ends_with(&in_go, "shapes::Circle::Area"));
    assert!(!ends_with(
        &name(&["impl Other", "add_go"]),
        "Known::add_go"
    ));
    assert!(!ends_with(&name(&["add_go"]), "Known::add_go"));
    assert!(!ends_with(&in_go, "Square::Area"));
}

#[tokio::test]
async fn test_kind_filter_function() {
    assert_eq!(names_by_kind("function").await, vec!["area"]);
}

#[tokio::test]
async fn test_kind_filter_struct() {
    assert_eq!(names_by_kind("struct").await, vec!["Point", "Wrap"]);
}

/// A multi-word kind is named by its display name, which is one word for an enum variant.
#[tokio::test]
async fn test_kind_filter_variant() {
    assert_eq!(names_by_kind("variant").await, vec!["Green", "Red"]);
}

#[tokio::test]
async fn test_actor_lifecycle() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let dir = tempfile::TempDir::new().unwrap();
    let actor = GraphActor::with_registry(
        dir.path().to_path_buf(),
        create_test_registry(),
        Arc::new(latt_resolve_ra::RustAnalyzer::default()),
    );

    let handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
    handle.await.unwrap().unwrap();
}

/// Test: adding a file indexes symbols.
#[tokio::test]
async fn test_add_file() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let dir = tempfile::TempDir::new().unwrap();
    let actor = GraphActor::with_registry(
        dir.path().to_path_buf(),
        create_test_registry(),
        Arc::new(latt_resolve_ra::RustAnalyzer::default()),
    );

    let handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    // Add a file
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::AddFile {
            path: PathBuf::from("test.rs"),
            content: "fn hello() {}".to_string(),
            language: Language::Rust,
            reply: reply_tx,
        })
        .await
        .unwrap();

    let file_id = reply_rx.await.unwrap().unwrap();
    assert_eq!(file_id, FileId::of_path(std::path::Path::new("test.rs")));

    cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_find_symbol() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let dir = tempfile::TempDir::new().unwrap();
    let actor = GraphActor::with_registry(
        dir.path().to_path_buf(),
        create_test_registry(),
        Arc::new(latt_resolve_ra::RustAnalyzer::default()),
    );

    let handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    // Add a file
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::AddFile {
            path: PathBuf::from("test.rs"),
            content: "fn process() {} fn helper() {}".to_string(),
            language: Language::Rust,
            reply: reply_tx,
        })
        .await
        .unwrap();
    reply_rx.await.unwrap().unwrap();

    // Find symbol
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::FindSymbol {
            name: "process".to_string(),
            options: Default::default(),
            reply: reply_tx,
        })
        .await
        .unwrap();

    let response = reply_rx.await.unwrap();
    assert!(response.has_results());
    assert_eq!(response.symbols[0].name, "process");

    cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_stats() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let dir = tempfile::TempDir::new().unwrap();
    let actor = GraphActor::with_registry(
        dir.path().to_path_buf(),
        create_test_registry(),
        Arc::new(latt_resolve_ra::RustAnalyzer::default()),
    );

    let handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    // Add a file
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::AddFile {
            path: PathBuf::from("test.rs"),
            content: "fn a() {} fn b() {}".to_string(),
            language: Language::Rust,
            reply: reply_tx,
        })
        .await
        .unwrap();
    reply_rx.await.unwrap().unwrap();

    // Get stats
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::Stats { reply: reply_tx })
        .await
        .unwrap();

    let stats = reply_rx.await.unwrap();
    assert_eq!(stats.symbols_count, 2);

    cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_remove_file() {
    let (cmd_tx, cmd_rx) = command_channel(16);

    let dir = tempfile::TempDir::new().unwrap();
    let actor = GraphActor::with_registry(
        dir.path().to_path_buf(),
        create_test_registry(),
        Arc::new(latt_resolve_ra::RustAnalyzer::default()),
    );

    let handle = tokio::spawn(async move { actor.run(cmd_rx).await });

    // Add a file
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::AddFile {
            path: PathBuf::from("test.rs"),
            content: "fn test() {}".to_string(),
            language: Language::Rust,
            reply: reply_tx,
        })
        .await
        .unwrap();
    let file_id = reply_rx.await.unwrap().unwrap();

    // Check stats before
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::Stats { reply: reply_tx })
        .await
        .unwrap();
    let stats = reply_rx.await.unwrap();
    assert_eq!(stats.symbols_count, 1);

    // Remove file
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::RemoveFile {
            file_id,
            reply: reply_tx,
        })
        .await
        .unwrap();
    let removed = reply_rx.await.unwrap().unwrap();
    assert!(removed);

    // Check stats after
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(GraphCommand::Stats { reply: reply_tx })
        .await
        .unwrap();
    let stats = reply_rx.await.unwrap();
    assert_eq!(stats.symbols_count, 0);

    cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
    handle.await.unwrap().unwrap();
}

#[test]
fn test_kind_filter_takes_short_and_full_names() {
    use crate::actor::queries::kind_matches;
    use latt_types::SymbolKind;
    assert!(kind_matches(SymbolKind::Constant, "const"));
    assert!(kind_matches(SymbolKind::Constant, "Constant"));
    assert!(kind_matches(SymbolKind::EnumVariant, "enum_variant"));
    assert!(kind_matches(SymbolKind::TypeAlias, "type"));
    assert!(!kind_matches(SymbolKind::EnumVariant, "enum"));
    assert!(!kind_matches(SymbolKind::Function, "struct"));
    assert!(kind_matches(SymbolKind::Method, "function"));
    assert!(kind_matches(SymbolKind::Method, "method"));
    assert!(!kind_matches(SymbolKind::Function, "method"));
}
