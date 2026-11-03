//! Edited files: what a running analyzer gives against what a new index gives.

use mimir_graph::{CodeGraph, EdgeKind};
use mimir_resolve::{ProjectFile, Refresh, link};
use mimir_resolve_ra::{Analyzers, load_or_generate};

use crate::expected::SOURCES;
use crate::support::{edges_of_kind, index_at, workspace};

/// An edit answered by a running analyzer gives what a new index gives.
#[test]
fn edited_files_match_a_new_index() {
    if std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipped: rust-analyzer is not installed");
        return;
    }
    let scratch = std::env::temp_dir().join(format!("mimir-resolve-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    for path in ["Cargo.toml", "alpha/Cargo.toml", "beta/Cargo.toml"]
        .into_iter()
        .chain(SOURCES)
    {
        std::fs::create_dir_all(scratch.join(path).parent().unwrap()).unwrap();
        std::fs::copy(workspace().join(path), scratch.join(path)).unwrap();
    }
    let edit = |path: &str, from: &str, to: &str| {
        let text = std::fs::read_to_string(scratch.join(path)).unwrap();
        assert!(text.contains(from), "{from}");
        std::fs::write(scratch.join(path), text.replacen(from, to, 1)).unwrap();
    };
    let edges = |graph: &CodeGraph, files: &[ProjectFile]| {
        [
            EdgeKind::Calls,
            EdgeKind::UsesAsValue,
            EdgeKind::UsesConstant,
            EdgeKind::Implements,
            EdgeKind::UsesType,
            EdgeKind::UsesField,
        ]
        .map(|kind| edges_of_kind(graph, files, kind))
    };

    let cache = scratch.join("cache");
    let mut analyzers = Analyzers::default();
    let (_, files) = index_at(&scratch);
    load_or_generate(&scratch, &cache, &files, &mut analyzers, Refresh::Edits);

    // Lines move in both files, a function is added, and a call and a value change.
    edit(
        "alpha/src/lib.rs",
        "pub struct Disk;",
        "pub fn added() -> u32 {\n    helper()\n}\n\npub struct Disk;",
    );
    edit(
        "beta/src/lib.rs",
        "pub fn direct() -> u32 {\n    helper()",
        "pub fn direct() -> u32 {\n    alpha::added()",
    );
    edit(
        "beta/src/lib.rs",
        "disks.iter().map(Disk::get)",
        "disks.iter().map(Disk::load_value)",
    );
    edit(
        "alpha/src/lib.rs",
        "    fn read(&self) -> u32 {",
        "    pub fn load_value(&self) -> u32 {\n        self.get()\n    }\n    fn read(&self) -> u32 {",
    );
    // Another type and another field are used, and lines move above a type and a field.
    edit(
        "beta/src/lib.rs",
        "pub fn reads_field(config: &alpha::Config) -> u32 {\n    config.limit",
        "pub fn reads_field(config: &alpha::Other) -> u32 {\n    config.limit",
    );
    edit(
        "alpha/src/lib.rs",
        "pub struct Config {\n    pub limit: u32,",
        "pub struct Config {\n    /// Moved a line down.\n    pub limit: u32,",
    );

    // Before any analyzer answers, syntax rules stand in for the edited files, and only those.
    let (graph, files) = index_at(&scratch);
    let indexes = load_or_generate(
        &scratch,
        &cache,
        &files,
        &mut Analyzers::default(),
        Refresh::Saved,
    );
    let report = link(&graph, &files, &indexes);
    assert_eq!((report.live_files, report.syntax_files), (0, 2));
    let saved = edges(&graph, &files);

    let (graph, files) = index_at(&scratch);
    let indexes = load_or_generate(&scratch, &cache, &files, &mut analyzers, Refresh::Edits);
    let report = link(&graph, &files, &indexes);
    assert_eq!(report.live_files, 2, "both edited files go to the analyzer");
    let live = edges(&graph, &files);

    let (graph, files) = index_at(&scratch);
    let indexes = load_or_generate(&scratch, &cache, &files, &mut analyzers, Refresh::Full);
    let report = link(&graph, &files, &indexes);
    assert_eq!(report.live_files, 0);
    let full = edges(&graph, &files);
    let _ = std::fs::remove_dir_all(&scratch);

    let wrong: Vec<_> = saved[0].difference(&full[0]).collect();
    assert!(wrong.is_empty(), "calls syntax got wrong: {wrong:#?}");
    assert_eq!(live[0], full[0], "calls");
    assert_eq!(live[2], full[2], "constants");
    assert_eq!(live[3], full[3], "implementations");
    assert_eq!(live[4], full[4], "types");
    assert_eq!(live[5], full[5], "fields");
    for kind in [4, 5] {
        let wrong: Vec<_> = saved[kind].difference(&full[kind]).collect();
        assert!(wrong.is_empty(), "uses syntax got wrong: {wrong:#?}");
    }
    // Only a value among a macro's tokens has to wait for the next index.
    let missing: Vec<_> = full[1].difference(&live[1]).collect();
    assert_eq!(
        missing,
        [&(
            "beta::value_in_macro".to_string(),
            "beta::helper".to_string()
        )]
    );
    assert!(live[1].difference(&full[1]).next().is_none());
}
