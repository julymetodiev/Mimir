//! Replacing text found by what it says, not by where it is.

use crate::{EditError, EditResult};
use std::path::Path;

const EXCERPT_CHARS: usize = 60;

/// Where `old` starts in `content`, counting matches that overlap: in `aaa`, `aa` occurs twice, and
/// replacing either would give a different file.
fn places(content: &str, old: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = content[from..].find(old) {
        let at = from + offset;
        found.push(at);
        let step = content[at..].chars().next().map_or(1, char::len_utf8);
        from = at + step;
    }
    found
}

/// The line a byte position is on, from 1.
fn line_of(content: &str, at: usize) -> usize {
    content[..at].matches('\n').count() + 1
}

/// `content` with `old` replaced by `new`. Unless `all` is set, `old` must occur exactly once: the
/// edit never picks one of several places.
pub fn replaced(path: &Path, content: &str, old: &str, new: &str, all: bool) -> EditResult<String> {
    if old.is_empty() {
        return Err(EditError::EmptyText {
            path: path.to_path_buf(),
        });
    }
    let found = places(content, old);
    match found.as_slice() {
        [] => Err(EditError::TextNotFound {
            path: path.to_path_buf(),
            text: excerpt(old),
        }),
        _ if all => Ok(content.replace(old, new)),
        [at] => Ok(format!(
            "{}{new}{}",
            &content[..*at],
            &content[at + old.len()..]
        )),
        many => Err(EditError::TextNotUnique {
            path: path.to_path_buf(),
            text: excerpt(old),
            count: many.len(),
            lines: many
                .iter()
                .map(|at| line_of(content, *at).to_string())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

/// The start of a text, enough to tell which of several edits an error is about.
fn excerpt(text: &str) -> String {
    let first = text.lines().next().unwrap_or(text).trim();
    let short: String = first.chars().take(EXCERPT_CHARS).collect();
    if short.len() == text.len() {
        short
    } else {
        format!("{short}...")
    }
}

/// Lines that change, at one place in a file. Line numbers count from 1.
#[derive(Debug, PartialEq)]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    pub old_text: String,
    pub new_text: String,
}

/// The first and last byte of the lines holding a range, without the last newline.
fn whole_lines(content: &str, start: usize, end: usize) -> (usize, usize) {
    let first = content[..start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let last = content[end..]
        .find('\n')
        .map_or(content.len(), |offset| end + offset);
    (first, last)
}

/// What replacing `old` with `new` changes, place by place. Places on the same line are one hunk.
pub fn hunks(content: &str, old: &str, new: &str) -> Vec<Hunk> {
    let mut regions: Vec<(usize, usize)> = Vec::new();
    for (at, found) in content.match_indices(old) {
        let (first, last) = whole_lines(content, at, at + found.len());
        match regions.last_mut() {
            Some(region) if first <= region.1 => region.1 = last,
            _ => regions.push((first, last)),
        }
    }
    let mut found = Vec::new();
    let mut lines_added = 0i64;
    for (first, last) in regions {
        let region = &content[first..last];
        let replaced = region.replace(old, new);
        let (skipped, old_text, new_text) = changed_lines(region, &replaced);
        let old_start = line_of(content, first) as i64 + skipped;
        found.push(Hunk {
            old_start: old_start as u32,
            new_start: (old_start + lines_added) as u32,
            old_text: old_text.clone(),
            new_text: new_text.clone(),
        });
        lines_added += new_text.lines().count() as i64 - old_text.lines().count() as i64;
    }
    found
}

/// The lines that differ between two texts, leaving out what they start and end with in common: how
/// many lines were left out at the start, and the lines of each.
fn changed_lines(old: &str, new: &str) -> (i64, String, String) {
    let (old_lines, new_lines): (Vec<&str>, Vec<&str>) =
        (old.lines().collect(), new.lines().collect());
    let same_start = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let same_end = old_lines[same_start..]
        .iter()
        .rev()
        .zip(new_lines[same_start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (
        same_start as i64,
        old_lines[same_start..old_lines.len() - same_end].join("\n"),
        new_lines[same_start..new_lines.len() - same_end].join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replace(content: &str, old: &str, new: &str, all: bool) -> EditResult<String> {
        replaced(Path::new("f.rs"), content, old, new, all)
    }

    #[test]
    fn test_hunks_show_each_place_and_not_what_lies_between() {
        let content = "a x\nkeep\nkeep\nb x x\nkeep\nc x\n";
        let found = hunks(content, "x", "y\nz");
        let places: Vec<_> = found
            .iter()
            .map(|h| (h.old_start, h.new_start, h.old_text.as_str()))
            .collect();
        assert_eq!(places, [(1, 1, "a x"), (4, 5, "b x x"), (6, 9, "c x")]);
        assert_eq!(found[0].new_text, "a y\nz");
        assert_eq!(found[1].new_text, "b y\nz y\nz");
    }

    #[test]
    fn test_a_hunk_leaves_out_lines_a_long_text_keeps() {
        let found = hunks("one\ntwo\nthree\n", "one\ntwo\nthree", "one\n2\nthree");
        assert_eq!(
            found,
            [Hunk {
                old_start: 2,
                new_start: 2,
                old_text: "two".to_string(),
                new_text: "2".to_string(),
            }]
        );
    }

    #[test]
    fn test_replaces_the_one_place_it_occurs() {
        assert_eq!(
            replace("let x = 1;\nlet y = 2;\n", "x = 1", "x = 9", false).unwrap(),
            "let x = 9;\nlet y = 2;\n"
        );
        assert_eq!(replace("a b", " b", "", false).unwrap(), "a");
    }

    #[test]
    fn test_spans_lines_and_keeps_letters_of_any_width() {
        assert_eq!(
            replace("едно\nдве\nтри\n", "дно\nдв", "X", false).unwrap(),
            "еXе\nтри\n"
        );
    }

    #[test]
    fn test_several_places_are_refused_and_named_unless_all() {
        let content = "x\ny\nx\nz\nx\n";
        let error = replace(content, "x", "w", false).unwrap_err();
        assert!(matches!(error, EditError::TextNotUnique { count: 3, .. }));
        assert!(error.to_string().contains("1, 3, 5"), "{error}");
        assert_eq!(replace(content, "x", "w", true).unwrap(), "w\ny\nw\nz\nw\n");
    }

    #[test]
    fn test_overlapping_places_count_as_several() {
        assert!(matches!(
            replace("aaa", "aa", "b", false),
            Err(EditError::TextNotUnique { count: 2, .. })
        ));
        assert!(matches!(
            replace("яяя", "яя", "b", false),
            Err(EditError::TextNotUnique { count: 2, .. })
        ));
    }

    #[test]
    fn test_an_error_says_which_text() {
        let error = replace("abc", "first line\nsecond", "y", false).unwrap_err();
        assert!(error.to_string().contains("`first line...`"), "{error}");
        let error = replace("x x", "x", "y", false).unwrap_err();
        assert!(error.to_string().contains("`x` occurs 2 times"), "{error}");
    }

    #[test]
    fn test_what_is_not_there_is_refused_even_with_all() {
        for all in [false, true] {
            assert!(matches!(
                replace("abc", "x", "y", all),
                Err(EditError::TextNotFound { .. })
            ));
            assert!(matches!(
                replace("abc", "", "y", all),
                Err(EditError::EmptyText { .. })
            ));
        }
    }

    #[test]
    fn test_changed_lines_leave_out_what_stays() {
        let (skipped, old, new) = changed_lines("a\nb\nc\nd\n", "a\nX\nY\nd\n");
        assert_eq!(skipped, 1);
        assert_eq!((old.as_str(), new.as_str()), ("b\nc", "X\nY"));

        let (skipped, old, new) = changed_lines("a\na\n", "a\na\na\n");
        assert_eq!(skipped, 2);
        assert_eq!((old.as_str(), new.as_str()), ("", "a"));
    }
}
