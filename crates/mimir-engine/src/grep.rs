//! Searching the text of a project's files. It reads them as they are on disk, including those in
//! languages Mimir does not parse, and skips what git ignores.

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use mimir_protocol::{GrepMatch, GrepOptions};
use regex::RegexBuilder;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::GraphError;

/// A file over this is data, not code.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// A line over this is minified; the match still counts, without all of its line.
const MAX_LINE_CHARS: usize = 400;

/// Everything that matches, in path and line order, each without its symbol.
pub fn search(root: &Path, options: &GrepOptions) -> Result<Vec<GrepMatch>, GraphError> {
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

    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(false)
        .require_git(false)
        .sort_by_file_path(Path::cmp)
        .filter_entry(|entry| !matches!(entry.file_name().to_str(), Some(".git" | ".mimir")));
    if let Some(glob) = &options.path {
        let invalid = |source| GraphError::InvalidGlob {
            glob: glob.clone(),
            source,
        };
        let mut only = OverrideBuilder::new(root);
        only.add(glob).map_err(invalid)?;
        walker.overrides(only.build().map_err(invalid)?);
    }

    let mut found = Vec::new();
    for entry in walker.build().flatten() {
        let path = entry.path();
        let small = entry
            .metadata()
            .is_ok_and(|file| file.is_file() && file.len() <= MAX_FILE_BYTES);
        // A file that is not text cannot be read as text, and has nothing to find.
        if let (true, Ok(text)) = (small, fs::read_to_string(path)) {
            let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            matches_in(&relative, &text, &regex, options.context, &mut found);
        }
    }
    Ok(found)
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
        fs::create_dir(dir.path().join(".mimir")).unwrap();
        fs::write(dir.path().join(".mimir/log"), "needle\n").unwrap();
        dir
    }

    #[test]
    fn test_finds_text_in_any_language_and_skips_what_git_ignores() {
        let dir = project();
        let mut asked = options("needle");
        asked.ignore_case = true;
        let found = search(dir.path(), &asked).unwrap();
        let places: Vec<_> = found
            .iter()
            .map(|m| (m.path.to_str().unwrap(), m.line))
            .collect();
        assert_eq!(places, [("a.rs", 2), ("notes.md", 1)]);
    }

    #[test]
    fn test_case_literal_context_and_path() {
        let dir = project();
        assert_eq!(search(dir.path(), &options("needle")).unwrap().len(), 1);

        let mut literal = options("Needle.*");
        literal.literal = true;
        assert_eq!(
            search(dir.path(), &literal).unwrap()[0].path,
            Path::new("notes.md")
        );

        let mut around = options("needle");
        around.context = 1;
        let found = &search(dir.path(), &around).unwrap()[0];
        assert_eq!(
            (found.before.as_slice(), found.after.as_slice()),
            (&["one".to_string()][..], &["three".to_string()][..])
        );

        let mut only = options("(?i)needle");
        only.path = Some("*.md".to_string());
        let found = search(dir.path(), &only).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, Path::new("notes.md"));
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
