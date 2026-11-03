//! What the graph holds follows the disk, whoever changed it, and is what a scan would find.

use mimir_engine::{Engine, GraphHandle, start};
use mimir_protocol::{Edit, EditChange, EditTarget};
use mimir_resolve::NoSemanticSource;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// A scan does not take a file over this.
const OVER_THE_LIMIT: usize = 500 * 1024 + 1;

/// A running engine on a project with these files.
async fn engine(files: &[(&str, &str)]) -> (TempDir, PathBuf, Engine) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let engine = start(root.clone(), Arc::new(NoSemanticSource)).await;
    // The first scan is done when the first answer comes.
    engine.handle().stats().await.unwrap();
    (dir, root, engine)
}

async fn known(handle: &GraphHandle, name: &str) -> bool {
    !handle.find_symbol(name).await.unwrap().symbols.is_empty()
}

/// Whether the graph comes to know `name`, or forget it, within the time a watcher may take.
async fn comes_to(handle: &GraphHandle, name: &str, wanted: bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if known(handle, name).await == wanted {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// A file whose text is `fn name() {}` and enough comment to pass the limit.
fn large_file(name: &str) -> String {
    let mut text = format!("fn {name}() {{}}\n");
    text.push_str(&"// filler\n".repeat(OVER_THE_LIMIT / 10 + 1));
    text
}

fn replace(path: &Path, old: &str, new: &str) -> Edit {
    Edit {
        target: EditTarget::File(path.to_path_buf()),
        change: EditChange::Text {
            old: old.to_string(),
            new: new.to_string(),
            all: false,
        },
    }
}

#[tokio::test]
async fn test_a_file_removed_from_outside_leaves_the_graph() {
    let (_dir, root, engine) =
        engine(&[("a.rs", "fn in_a() {}\n"), ("b.rs", "fn in_b() {}\n")]).await;
    let handle = engine.handle();
    assert!(known(handle, "in_a").await);

    std::fs::remove_file(root.join("a.rs")).unwrap();

    assert!(comes_to(handle, "in_a", false).await, "still known");
    assert!(known(handle, "in_b").await);
}

#[tokio::test]
async fn test_a_file_moved_from_outside_is_known_at_its_new_place() {
    let (_dir, root, engine) = engine(&[("a.rs", "fn moved() {}\n")]).await;
    let handle = engine.handle();

    std::fs::rename(root.join("a.rs"), root.join("b.rs")).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut places = Vec::new();
    while Instant::now() < deadline {
        places = handle
            .find_symbol("moved")
            .await
            .unwrap()
            .symbols
            .into_iter()
            .map(|symbol| symbol.file_path)
            .collect();
        if places == [PathBuf::from("b.rs")] {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(places, [PathBuf::from("b.rs")]);
}

#[tokio::test]
async fn test_a_directory_moved_from_outside_is_known_at_its_new_place() {
    let (_dir, root, engine) = engine(&[("old/deep/a.rs", "fn in_dir() {}\n")]).await;
    let handle = engine.handle();

    std::fs::rename(root.join("old"), root.join("new")).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut places = Vec::new();
    while Instant::now() < deadline {
        places = handle
            .find_symbol("in_dir")
            .await
            .unwrap()
            .symbols
            .into_iter()
            .map(|symbol| symbol.file_path)
            .collect();
        if places == [PathBuf::from("new/deep/a.rs")] {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(places, [PathBuf::from("new/deep/a.rs")]);
}

#[tokio::test]
async fn test_a_directory_removed_from_outside_leaves_the_graph() {
    let (_dir, root, engine) = engine(&[("gone/a.rs", "fn in_gone() {}\n")]).await;
    let handle = engine.handle();

    std::fs::remove_dir_all(root.join("gone")).unwrap();

    assert!(comes_to(handle, "in_gone", false).await, "still known");
}

/// A scan leaves out a large file, so an edit must not bring it in for the next scan to take out.
#[tokio::test]
async fn test_an_edit_does_not_bring_in_a_file_too_large_for_a_scan() {
    let large = large_file("first");
    let (_dir, _root, engine) = engine(&[("large.rs", &large)]).await;
    let handle = engine.handle();
    assert!(!known(handle, "first").await);

    let edit = replace(Path::new("large.rs"), "fn first", "fn second");
    handle.edit(vec![edit], false, false).await.unwrap();

    assert!(!known(handle, "second").await);
    handle.sync().await.unwrap();
    assert!(!known(handle, "second").await);
}

/// The same when the change comes from outside.
#[tokio::test]
async fn test_the_watcher_does_not_bring_in_a_file_too_large_for_a_scan() {
    let (_dir, root, engine) = engine(&[("small.rs", "fn small() {}\n")]).await;
    let handle = engine.handle();

    std::fs::write(root.join("large.rs"), large_file("from_outside")).unwrap();
    std::fs::write(root.join("marker.rs"), "fn marker() {}\n").unwrap();

    assert!(comes_to(handle, "marker", true).await);
    assert!(!known(handle, "from_outside").await);
}

/// A file that grows past the limit is what a scan leaves out, so it leaves the graph.
#[tokio::test]
async fn test_a_file_that_grows_too_large_leaves_the_graph() {
    let (_dir, root, engine) = engine(&[("grows.rs", "fn grows() {}\n")]).await;
    let handle = engine.handle();
    assert!(known(handle, "grows").await);

    std::fs::write(root.join("grows.rs"), large_file("grows")).unwrap();

    assert!(comes_to(handle, "grows", false).await, "still known");
}

/// An edit that makes a file too large takes it out at once, as a scan would.
#[tokio::test]
async fn test_an_edit_that_makes_a_file_too_large_takes_it_out() {
    let (_dir, _root, engine) = engine(&[("grows.rs", "fn grows() {}\n")]).await;
    let handle = engine.handle();

    let edit = replace(
        Path::new("grows.rs"),
        "fn grows() {}\n",
        &large_file("grows"),
    );
    handle.edit(vec![edit], false, false).await.unwrap();

    assert!(!known(handle, "grows").await);
}
