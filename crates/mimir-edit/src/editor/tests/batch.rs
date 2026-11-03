use super::*;

#[tokio::test]
async fn test_batch_operations() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "aaa bbb ccc").await;

    let ops = vec![
        EditOp::replace(&path, Span::new(0, 3), "AAA"),
        EditOp::replace(&path, Span::new(8, 11), "CCC"),
    ];
    let editor = Editor::new();

    let results = editor.apply_batch(&ops).await.unwrap();
    assert_eq!(results.len(), 1);

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "AAA bbb CCC");
}

#[tokio::test]
async fn test_overlapping_edits_detection() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "0123456789").await;
    let editor = Editor::new();

    // These spans overlap: 2-6 and 4-8
    let ops = vec![
        EditOp::replace(&path, Span::new(2, 6), "XX"),
        EditOp::replace(&path, Span::new(4, 8), "YY"),
    ];

    let result = editor.apply_batch(&ops).await;
    assert!(matches!(result, Err(EditError::OverlappingEdits)));
}

#[tokio::test]
async fn test_adjacent_edits_allowed() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "0123456789").await;
    let editor = Editor::new();

    // Adjacent spans: 0-3 and 3-6 (touching but not overlapping)
    let ops = vec![
        EditOp::replace(&path, Span::new(0, 3), "AAA"),
        EditOp::replace(&path, Span::new(3, 6), "BBB"),
    ];

    let results = editor.apply_batch(&ops).await.unwrap();
    assert_eq!(results.len(), 1);

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "AAABBB6789");
}

#[tokio::test]
async fn test_batch_multiple_files() {
    let dir = tempdir().unwrap();
    let path1 = create_test_file(dir.path(), "file1.txt", "content1").await;
    let path2 = create_test_file(dir.path(), "file2.txt", "content2").await;
    let editor = Editor::new();

    let ops = vec![
        EditOp::replace(&path1, Span::new(0, 7), "CONTENT"),
        EditOp::replace(&path2, Span::new(0, 7), "MODIFIED"),
    ];

    let results = editor.apply_batch(&ops).await.unwrap();
    assert_eq!(results.len(), 2);

    let c1 = fs::read_to_string(&path1).await.unwrap();
    let c2 = fs::read_to_string(&path2).await.unwrap();
    assert_eq!(c1, "CONTENT1");
    assert_eq!(c2, "MODIFIED2");
}

#[tokio::test]
async fn test_sequential_edits_same_file() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "hello world").await;
    let editor = Editor::new();

    // First edit
    let op1 = EditOp::replace(&path, Span::new(0, 5), "HELLO");
    let result1 = apply(&editor, &op1).await.unwrap();

    // Second edit - use new hash
    let op2 =
        EditOp::replace(&path, Span::new(6, 11), "WORLD").with_expected_hash(result1.new_hash);
    apply(&editor, &op2).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "HELLO WORLD");
}

#[tokio::test]
async fn test_hash_chain_verification() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "step0").await;
    let editor = Editor::new();

    // Chain of edits, each using previous hash
    let mut current_hash = ContentHash::compute(b"step0");

    for i in 1..=5 {
        let new_content = format!("step{}", i);
        let op =
            EditOp::replace(&path, Span::new(0, 5), &new_content).with_expected_hash(current_hash);

        let result = apply(&editor, &op).await.unwrap();
        current_hash = result.new_hash;

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, new_content);
    }
}

#[tokio::test]
async fn test_batch_with_inserts_and_deletes() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "AABBCCDD").await;
    let editor = Editor::new();

    // Multiple non-overlapping operations
    let ops = vec![
        EditOp::delete(&path, Span::new(0, 2)), // Remove "AA"
        EditOp::insert(&path, 4, "XX"),         // Insert after "BB"
        EditOp::delete(&path, Span::new(6, 8)), // Remove "DD"
    ];

    let results = editor.apply_batch(&ops).await.unwrap();
    assert_eq!(results.len(), 1);

    let content = fs::read_to_string(&path).await.unwrap();
    // After sorting by position (descending) and applying:
    // Original: AABBCCDD
    // Delete 6-8 (DD): AABBCC
    // Insert at 4 (XX): AABBXXCC
    // Delete 0-2 (AA): BBXXCC
    assert_eq!(content, "BBXXCC");
}
