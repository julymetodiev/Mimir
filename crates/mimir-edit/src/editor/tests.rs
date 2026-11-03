use super::*;
use mimir_types::Span;
use tempfile::tempdir;

mod batch;

async fn create_test_file(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).await.unwrap();
    path
}

#[tokio::test]
async fn test_replace_operation() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.rs", "fn hello() {}").await;

    let op = EditOp::replace(&path, Span::new(3, 8), "world");
    let editor = Editor::new();

    let result = editor.apply(&op).await.unwrap();
    assert_eq!(result.file_path, path);

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "fn world() {}");
}

#[tokio::test]
async fn test_insert_operation() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.rs", "fn test() {}").await;

    let op = EditOp::insert(&path, 3, "pub ");
    let editor = Editor::new();

    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "fn pub test() {}");
}

#[tokio::test]
async fn test_delete_operation() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.rs", "fn hello_world() {}").await;

    let op = EditOp::delete(&path, Span::new(8, 14)); // delete "_world"
    let editor = Editor::new();

    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "fn hello() {}");
}

#[tokio::test]
async fn test_replace_lines_operation() {
    let dir = tempdir().unwrap();
    let content = "line1\nline2\nline3\nline4\n";
    let path = create_test_file(dir.path(), "test.txt", content).await;

    let op = EditOp::replace_lines(&path, 2, 3, "new_line2\nnew_line3\n");
    let editor = Editor::new();

    editor.apply(&op).await.unwrap();

    let result = fs::read_to_string(&path).await.unwrap();
    assert_eq!(result, "line1\nnew_line2\nnew_line3\nline4\n");
}

#[tokio::test]
async fn test_optimistic_locking_success() {
    let dir = tempdir().unwrap();
    let content = "original content";
    let path = create_test_file(dir.path(), "test.txt", content).await;

    let hash = ContentHash::compute(content.as_bytes());
    let op = EditOp::replace(&path, Span::new(0, 8), "modified").with_expected_hash(hash);
    let editor = Editor::new();

    let result = editor.apply(&op).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_optimistic_locking_failure() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "original content").await;

    let wrong_hash = ContentHash::compute(b"different content");
    let op = EditOp::replace(&path, Span::new(0, 8), "modified").with_expected_hash(wrong_hash);
    let editor = Editor::new();

    let result = editor.apply(&op).await;
    assert!(matches!(result, Err(EditError::HashMismatch { .. })));
}

#[tokio::test]
async fn test_preview_operation() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.rs", "fn old() {}").await;

    let op = EditOp::replace(&path, Span::new(3, 6), "new");
    let editor = Editor::new();

    let diff = editor.preview(&op).await.unwrap();
    assert!(diff.unified.contains("-fn old() {}"));
    assert!(diff.unified.contains("+fn new() {}"));

    // Original file should be unchanged
    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "fn old() {}");
}

#[tokio::test]
async fn test_invalid_span() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "short").await;

    let op = EditOp::replace(&path, Span::new(0, 100), "new");
    let editor = Editor::new();

    let result = editor.apply(&op).await;
    assert!(matches!(result, Err(EditError::InvalidSpan { .. })));
}

#[tokio::test]
async fn test_file_not_found() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nonexistent.txt");

    let op = EditOp::replace(&path, Span::new(0, 5), "new");
    let editor = Editor::new();

    let result = editor.apply(&op).await;
    assert!(matches!(result, Err(EditError::FileNotFound { .. })));
}

#[tokio::test]
async fn test_backup_creation() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "original").await;

    let op = EditOp::replace(&path, Span::new(0, 8), "modified");
    let editor = Editor::new().with_backups();

    editor.apply(&op).await.unwrap();

    let backup_path = path.with_extension("bak");
    let backup_content = fs::read_to_string(&backup_path).await.unwrap();
    assert_eq!(backup_content, "original");
}

// ==================== Advanced Tests ====================

#[tokio::test]
async fn test_empty_file_operations() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "empty.txt", "").await;
    let editor = Editor::new();

    // Insert into empty file
    let op = EditOp::insert(&path, 0, "hello world");
    let result = editor.apply(&op).await.unwrap();
    assert_eq!(result.bytes_changed, 11);

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn test_unicode_content() {
    let dir = tempdir().unwrap();
    let original = "Привет мир! 你好世界 🦀🔥";
    let path = create_test_file(dir.path(), "unicode.txt", original).await;
    let editor = Editor::new();

    // Replace unicode characters - need byte offsets
    let privet_end = "Привет".len(); // 12 bytes in UTF-8

    let op = EditOp::replace(&path, Span::new(0, privet_end as u32), "Здравей");
    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert!(content.starts_with("Здравей"));
    assert!(content.contains("🦀"));
}

#[tokio::test]
async fn test_multiline_replace() {
    let dir = tempdir().unwrap();
    let content = r#"fn main() {
println!("Hello");
println!("World");
}
"#;
    let path = create_test_file(dir.path(), "main.rs", content).await;
    let editor = Editor::new();

    // Replace lines 2-3 with a single line (4 lines -> 3 lines)
    let op = EditOp::replace_lines(&path, 2, 3, "    println!(\"Hello World\");\n");
    editor.apply(&op).await.unwrap();

    let result = fs::read_to_string(&path).await.unwrap();
    assert!(result.contains("Hello World"));
    assert!(!result.contains("\"Hello\""));
    assert!(!result.contains("\"World\""));
    // Original: 4 lines, replaced 2 lines with 1 = 3 lines total
    assert_eq!(result.lines().count(), 3);
}

#[tokio::test]
async fn test_replace_first_line() {
    let dir = tempdir().unwrap();
    let content = "first\nsecond\nthird\n";
    let path = create_test_file(dir.path(), "test.txt", content).await;
    let editor = Editor::new();

    let op = EditOp::replace_lines(&path, 1, 1, "FIRST\n");
    editor.apply(&op).await.unwrap();

    let result = fs::read_to_string(&path).await.unwrap();
    assert_eq!(result, "FIRST\nsecond\nthird\n");
}

#[tokio::test]
async fn test_replace_last_line() {
    let dir = tempdir().unwrap();
    let content = "first\nsecond\nthird\n";
    let path = create_test_file(dir.path(), "test.txt", content).await;
    let editor = Editor::new();

    let op = EditOp::replace_lines(&path, 3, 3, "THIRD\n");
    editor.apply(&op).await.unwrap();

    let result = fs::read_to_string(&path).await.unwrap();
    assert_eq!(result, "first\nsecond\nTHIRD\n");
}

#[tokio::test]
async fn test_invalid_line_range_zero() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "line1\nline2\n").await;
    let editor = Editor::new();

    // Line 0 is invalid (1-indexed)
    let op = EditOp::replace_lines(&path, 0, 1, "new\n");
    let result = editor.apply(&op).await;
    assert!(matches!(result, Err(EditError::InvalidLineRange { .. })));
}

#[tokio::test]
async fn test_invalid_line_range_reversed() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "line1\nline2\nline3\n").await;
    let editor = Editor::new();

    // start > end is invalid
    let op = EditOp::replace_lines(&path, 3, 1, "new\n");
    let result = editor.apply(&op).await;
    assert!(matches!(result, Err(EditError::InvalidLineRange { .. })));
}

#[tokio::test]
async fn test_invalid_line_range_exceeds_file() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "line1\nline2\n").await;
    let editor = Editor::new();

    // Line 5 doesn't exist
    let op = EditOp::replace_lines(&path, 1, 5, "new\n");
    let result = editor.apply(&op).await;
    assert!(matches!(result, Err(EditError::InvalidLineRange { .. })));
}

#[tokio::test]
async fn test_concurrent_modification_detected() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "original").await;
    let editor = Editor::new();

    // Get hash of original content
    let original_hash = ContentHash::compute(b"original");

    // Simulate another process modifying the file
    fs::write(&path, "modified by other").await.unwrap();

    // Try to edit with old hash - should fail
    let op = EditOp::replace(&path, Span::new(0, 8), "my edit").with_expected_hash(original_hash);

    let result = editor.apply(&op).await;
    assert!(matches!(result, Err(EditError::HashMismatch { .. })));

    // Verify the "other" modification is preserved
    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "modified by other");
}

#[tokio::test]
async fn test_insert_at_end() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "hello").await;
    let editor = Editor::new();

    let op = EditOp::insert(&path, 5, " world");
    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn test_insert_at_start() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "world").await;
    let editor = Editor::new();

    let op = EditOp::insert(&path, 0, "hello ");
    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn test_delete_entire_content() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "delete me").await;
    let editor = Editor::new();

    let op = EditOp::delete(&path, Span::new(0, 9));
    let result = editor.apply(&op).await.unwrap();

    assert_eq!(result.bytes_changed, -9);

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "");
}

#[tokio::test]
async fn test_preview_does_not_modify() {
    let dir = tempdir().unwrap();
    let original = "do not change";
    let path = create_test_file(dir.path(), "test.txt", original).await;
    let editor = Editor::new();

    // Preview multiple times
    for _ in 0..3 {
        let op = EditOp::replace(&path, Span::new(0, 2), "XX");
        let diff = editor.preview(&op).await.unwrap();
        assert!(diff.unified.contains("-do"));
        assert!(diff.unified.contains("+XX"));
    }

    // File should be unchanged
    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, original);
}

#[tokio::test]
async fn test_result_stats_accuracy() {
    let dir = tempdir().unwrap();
    let original = "line1\nline2\nline3\n";
    let path = create_test_file(dir.path(), "test.txt", original).await;
    let editor = Editor::new();

    // Replace 2 lines with 3 lines
    let op = EditOp::replace_lines(&path, 1, 2, "new1\nnew2\nnew3\n");
    let result = editor.apply(&op).await.unwrap();

    assert_eq!(result.lines_changed, 1); // 3 lines - 2 lines = +1
    assert!(result.bytes_changed > 0);

    // Verify hashes are different
    assert_ne!(result.old_hash, result.new_hash);
}

#[tokio::test]
async fn test_large_file_edit() {
    let dir = tempdir().unwrap();

    // Create a file with 1000 lines (1-indexed: line 1 = "Line number 1")
    let mut content = String::new();
    for i in 1..=1000 {
        content.push_str(&format!("Line number {}\n", i));
    }
    let path = create_test_file(dir.path(), "large.txt", &content).await;
    let editor = Editor::new();

    // Edit line 500 (which contains "Line number 500")
    let op = EditOp::replace_lines(&path, 500, 500, "MODIFIED LINE 500\n");
    let result = editor.apply(&op).await.unwrap();

    assert!(result.bytes_changed != 0);

    let result_content = fs::read_to_string(&path).await.unwrap();
    assert!(result_content.contains("MODIFIED LINE 500"));
    // Line 499 and 501 should be unchanged
    assert!(result_content.contains("Line number 499"));
    assert!(result_content.contains("Line number 501"));
    // Original line 500 should be gone
    assert!(!result_content.contains("Line number 500\n"));
}

#[tokio::test]
async fn test_special_characters_in_content() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "placeholder").await;
    let editor = Editor::new();

    // Content with special regex/shell characters
    let special = r#"$HOME/*.rs | grep -E "test\d+" && echo 'done'"#;
    let op = EditOp::replace(&path, Span::new(0, 11), special);
    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, special);
}

#[tokio::test]
async fn test_newline_handling_no_trailing() {
    let dir = tempdir().unwrap();
    // File without trailing newline
    let path = create_test_file(dir.path(), "test.txt", "no newline").await;
    let editor = Editor::new();

    let op = EditOp::replace(&path, Span::new(3, 10), "NEWLINE");
    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "no NEWLINE");
    assert!(!content.ends_with('\n'));
}

#[tokio::test]
async fn test_newline_handling_with_trailing() {
    let dir = tempdir().unwrap();
    // File with trailing newline
    let path = create_test_file(dir.path(), "test.txt", "with newline\n").await;
    let editor = Editor::new();

    let op = EditOp::replace(&path, Span::new(5, 12), "NEWLINE");
    editor.apply(&op).await.unwrap();

    let content = fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, "with NEWLINE\n");
}

#[tokio::test]
async fn test_preview_with_hash_mismatch() {
    let dir = tempdir().unwrap();
    let path = create_test_file(dir.path(), "test.txt", "content").await;
    let editor = Editor::new();

    let wrong_hash = ContentHash::compute(b"wrong");
    let op = EditOp::replace(&path, Span::new(0, 7), "new").with_expected_hash(wrong_hash);

    let result = editor.preview(&op).await;
    assert!(matches!(result, Err(EditError::HashMismatch { .. })));
}
