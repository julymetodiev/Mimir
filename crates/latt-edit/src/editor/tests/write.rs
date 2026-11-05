//! Writing whole files.

use super::*;

#[tokio::test]
async fn test_write_file_makes_a_new_file_and_its_directories() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a/b/new.rs");

    let result = Editor::new()
        .write_file(&path, "one\ntwo\n", false)
        .await
        .unwrap();

    assert_eq!(fs::read_to_string(&path).await.unwrap(), "one\ntwo\n");
    assert_eq!((result.bytes_changed, result.lines_changed), (8, 2));
}

#[tokio::test]
async fn test_write_file_leaves_an_existing_file_unless_told() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "old.rs", "old\nfile\n").await;
    let editor = Editor::new();

    let refused = editor.write_file(&path, "new\n", false).await;
    assert!(matches!(refused, Err(EditError::FileExists { .. })));
    assert_eq!(fs::read_to_string(&path).await.unwrap(), "old\nfile\n");

    let result = editor.write_file(&path, "new\n", true).await.unwrap();
    assert_eq!(fs::read_to_string(&path).await.unwrap(), "new\n");
    assert_eq!((result.bytes_changed, result.lines_changed), (-5, -1));
}

#[tokio::test]
async fn test_write_file_does_not_replace_what_is_not_text() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("data.bin");
    fs::write(&path, [0xff, 0xfe]).await.unwrap();

    assert!(Editor::new().write_file(&path, "x", true).await.is_err());
    assert_eq!(fs::read(&path).await.unwrap(), [0xff, 0xfe]);
}
