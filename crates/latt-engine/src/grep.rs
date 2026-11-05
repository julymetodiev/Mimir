//! Searching the text of a project's files. It reads them as they are on disk, including those in
//! languages Lantern Lattice does not parse, and skips what git ignores.

use latt_protocol::{GrepMatch, GrepOptions};
use regex::RegexBuilder;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::GraphError;
use crate::project_files;

/// A file over this is data, not code.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// A line over this is minified; the match still counts, without all of its line.
const MAX_LINE_CHARS: usize = 400;

/// Everything that matches, in path and line order, each without its symbol, and the files too
/// large to search.
pub fn search(
    root: &Path,
    options: &GrepOptions,
) -> Result<(Vec<GrepMatch>, Vec<PathBuf>), GraphError> {
    let pattern = if options.literal {
        regex::escape(&options.pattern)
    } else {
        options.pattern.clone()
    };
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(options.ignore_case)
        .build()
        .map_err(|source| GraphError::InvalidPattern {
            pattern: options.pattern.clone(),
            source,
        })?;

    let mut found = Vec::new();
    let mut too_large = Vec::new();
    for entry in project_files::walk(root, options.path.as_deref())?.flatten() {
        let path = entry.path();
        let Ok(file) = entry.metadata() else {
            continue;
        };
        if !file.is_file() {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        if file.len() > MAX_FILE_BYTES {
            too_large.push(relative);
            continue;
        }
        // A file that is not text cannot be read as text, and has nothing to find.
        if let Ok(text) = fs::read_to_string(path) {
            matches_in(&relative, &text, &regex, options.context, &mut found);
        }
    }
    Ok((found, too_large))
}

fn matches_in(
    path: &Path,
    text: &str,
    regex: &regex::Regex,
    context: u32,
    found: &mut Vec<GrepMatch>,
) {
    let lines: Vec<&str> = text.lines().collect();
    let context = context as usize;
    for (index, line) in lines.iter().enumerate() {
        if regex.is_match(line) {
            found.push(GrepMatch {
                path: PathBuf::from(path),
                line: index as u32 + 1,
                text: shortened(line),
                before: lines[index.saturating_sub(context)..index]
                    .iter()
                    .map(|line| shortened(line))
                    .collect(),
                after: lines[index + 1..(index + 1 + context).min(lines.len())]
                    .iter()
                    .map(|line| shortened(line))
                    .collect(),
                symbol: None,
            });
        }
    }
}

fn shortened(line: &str) -> String {
    line.chars().take(MAX_LINE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn options(pattern: &str) -> GrepOptions {
        GrepOptions {
            pattern: pattern.to_string(),
            ..GrepOptions::default()
        }
    }

    fn project() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "needle\n").unwrap();
        fs::write(dir.path().join("a.rs"), "one\nneedle here\nthree\n").unwrap();
        fs::write(dir.path().join("notes.md"), "a Needle.*\n").unwrap();
        fs::write(dir.path().join("data.bin"), [0xff, 0xfe, 0x00, b'n']).unwrap();
        fs::create_dir(dir.path().join(".latt")).unwrap();
        fs::write(dir.path().join(".latt/log"), "needle\n").unwrap();
        dir
    }

    #[test]
    fn test_finds_text_in_any_language_and_skips_what_git_ignores() {
        let dir = project();
        let mut asked = options("needle");
        asked.ignore_case = true;
        let (found, _) = search(dir.path(), &asked).unwrap();
        let places: Vec<_> = found
            .iter()
            .map(|m| (m.path.to_str().unwrap(), m.line))
            .collect();
        assert_eq!(places, [("a.rs", 2), ("notes.md", 1)]);
    }

    #[test]
    fn test_case_literal_context_and_path() {
        let dir = project();
        assert_eq!(search(dir.path(), &options("needle")).unwrap().0.len(), 1);

        let mut literal = options("Needle.*");
        literal.literal = true;
        assert_eq!(
            search(dir.path(), &literal).unwrap().0[0].path,
            Path::new("notes.md")
        );

        let mut around = options("needle");
        around.context = 1;
        let found = &search(dir.path(), &around).unwrap().0[0];
        assert_eq!(
            (found.before.as_slice(), found.after.as_slice()),
            (&["one".to_string()][..], &["three".to_string()][..])
        );

        let mut only = options("(?i)needle");
        only.path = Some("*.md".to_string());
        let (found, _) = search(dir.path(), &only).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, Path::new("notes.md"));
    }

    #[test]
    fn test_a_file_too_large_is_named_and_not_searched() {
        let dir = project();
        let large = "needle\n".repeat(MAX_FILE_BYTES as usize / 7 + 1);
        fs::write(dir.path().join("large.log"), large).unwrap();

        let (found, too_large) = search(dir.path(), &options("needle")).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(too_large, [PathBuf::from("large.log")]);
    }

    #[test]
    fn test_a_bad_pattern_or_glob_is_an_error() {
        let dir = project();
        assert!(search(dir.path(), &options("(")).is_err());
        let mut bad = options("x");
        bad.path = Some("{".to_string());
        assert!(search(dir.path(), &bad).is_err());
    }
}
