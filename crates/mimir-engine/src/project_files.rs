//! The files of a project as they are on disk: which there are, and what one holds.

use ignore::overrides::OverrideBuilder;
use ignore::{Walk, WalkBuilder};
use mimir_protocol::{FileText, FilesResponse};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::GraphError;

/// A file over this is data, not something to read whole.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// How many lines of a file come back unless asked otherwise.
const DEFAULT_LINES: u32 = 2000;

/// A directory of git or of Mimir itself: inside the root, but not one of the project's files.
fn is_tooling(name: &str) -> bool {
    matches!(name, ".git" | ".mimir")
}

/// Every entry of a project git does not ignore, in path order, only those matching `glob` if given.
pub fn walk(root: &Path, glob: Option<&str>) -> Result<Walk, GraphError> {
    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(false)
        .require_git(false)
        .sort_by_file_path(Path::cmp)
        .filter_entry(|entry| !entry.file_name().to_str().is_some_and(is_tooling));
    if let Some(glob) = glob {
        let invalid = |source| GraphError::InvalidGlob {
            glob: glob.to_string(),
            source,
        };
        let mut only = OverrideBuilder::new(root);
        only.add(glob).map_err(invalid)?;
        walker.overrides(only.build().map_err(invalid)?);
    }
    Ok(walker.build())
}

/// The project's files, relative to its root.
pub fn list(root: &Path, glob: Option<&str>, limit: usize) -> Result<FilesResponse, GraphError> {
    let mut files: Vec<PathBuf> = walk(root, glob)?
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| {
            let path = entry.path();
            path.strip_prefix(root).unwrap_or(path).to_path_buf()
        })
        .collect();
    let total = files.len();
    files.truncate(limit);
    Ok(FilesResponse { files, total })
}

/// The real path of a file, which must be inside the project: a link cannot lead out of it.
pub fn inside(root: &Path, path: &Path) -> Result<PathBuf, GraphError> {
    let absolute = root.join(path);
    let real = absolute
        .canonicalize()
        .map_err(|_| GraphError::FileNotFound {
            path: absolute.clone(),
        })?;
    if real.starts_with(root) {
        Ok(real)
    } else {
        Err(GraphError::OutsideProject(absolute))
    }
}

/// Where a write to `path` lands, which may not exist yet: inside the project, not through a link
/// leading out of it, and not among the files of git or Mimir.
pub fn writable(root: &Path, path: &Path) -> Result<PathBuf, GraphError> {
    let absolute = root.join(path);
    let outside = || GraphError::OutsideProject(absolute.clone());
    let existing = absolute
        .ancestors()
        .find(|ancestor| ancestor.symlink_metadata().is_ok())
        .ok_or_else(outside)?;
    let rest = absolute.strip_prefix(existing).map_err(|_| outside())?;
    if !rest
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(outside());
    }
    let base = inside(root, existing).map_err(|e| match e {
        GraphError::OutsideProject(_) => outside(),
        other => other,
    })?;
    // Joining nothing would add a slash, and a file with a slash after it is taken for a directory.
    let real = if rest.as_os_str().is_empty() {
        base
    } else {
        base.join(rest)
    };
    let tooling = real
        .strip_prefix(root)
        .map_err(|_| outside())?
        .components()
        .any(|part| part.as_os_str().to_str().is_some_and(is_tooling));
    if tooling { Err(outside()) } else { Ok(real) }
}

/// Lines of a file: from `first`, 1-indexed, up to `last` inclusive, or a screenful if not given.
pub fn read(
    root: &Path,
    path: &Path,
    first: Option<u32>,
    last: Option<u32>,
) -> Result<FileText, GraphError> {
    let real = inside(root, path)?;
    let unreadable = |source: io::Error| GraphError::Read {
        path: real.clone(),
        source,
    };
    let size = fs::metadata(&real).map_err(unreadable)?.len();
    if size > MAX_FILE_BYTES {
        return Err(GraphError::FileTooLarge { path: real, size });
    }
    let text = fs::read_to_string(&real).map_err(|source| match source.kind() {
        io::ErrorKind::InvalidData => GraphError::NotText { path: real.clone() },
        _ => unreadable(source),
    })?;

    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len() as u32;
    let first_line = first.unwrap_or(1).max(1);
    let last_line = last
        .unwrap_or(first_line.saturating_add(DEFAULT_LINES - 1))
        .min(total_lines);
    let shown = lines
        .iter()
        .skip(first_line as usize - 1)
        .take(last_line.saturating_sub(first_line - 1) as usize)
        .map(|line| (*line).to_string())
        .collect();
    Ok(FileText {
        path: real.strip_prefix(root).unwrap_or(&real).to_path_buf(),
        first_line,
        total_lines,
        lines: shown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn project() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::write(root.join(".gitignore"), "skipped.txt\n").unwrap();
        fs::write(root.join("skipped.txt"), "x\n").unwrap();
        fs::write(root.join("a.rs"), "one\ntwo\nthree\nfour\n").unwrap();
        fs::write(root.join("data.bin"), [0xff, 0xfe, 0x00]).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/b.md"), "b\n").unwrap();
        fs::create_dir(root.join(".mimir")).unwrap();
        fs::write(root.join(".mimir/log"), "x\n").unwrap();
        (dir, root)
    }

    fn names(found: &FilesResponse) -> Vec<&str> {
        found.files.iter().map(|f| f.to_str().unwrap()).collect()
    }

    #[test]
    fn test_lists_what_git_does_not_ignore() {
        let (_dir, root) = project();
        let found = list(&root, None, 100).unwrap();
        assert_eq!(
            names(&found),
            [".gitignore", "a.rs", "data.bin", "src/b.md"]
        );
    }

    #[test]
    fn test_a_glob_and_a_limit_narrow_the_list() {
        let (_dir, root) = project();
        assert_eq!(
            names(&list(&root, Some("**/*.md"), 100).unwrap()),
            ["src/b.md"]
        );
        let found = list(&root, None, 1).unwrap();
        assert_eq!((found.files.len(), found.total), (1, 4));
        assert!(list(&root, Some("{"), 1).is_err());
    }

    #[test]
    fn test_reads_all_or_a_range() {
        let (_dir, root) = project();
        let all = read(&root, Path::new("a.rs"), None, None).unwrap();
        assert_eq!(all.lines, ["one", "two", "three", "four"]);
        assert_eq!((all.first_line, all.total_lines), (1, 4));

        let part = read(&root, Path::new("a.rs"), Some(2), Some(3)).unwrap();
        assert_eq!(part.lines, ["two", "three"]);
        assert_eq!((part.first_line, part.total_lines), (2, 4));

        assert!(
            read(&root, Path::new("a.rs"), Some(9), None)
                .unwrap()
                .lines
                .is_empty()
        );
        let absolute = root.join("a.rs");
        assert_eq!(
            read(&root, &absolute, None, None).unwrap().path,
            Path::new("a.rs")
        );
    }

    #[test]
    fn test_a_write_may_land_on_a_new_path_of_the_project() {
        let (_dir, root) = project();
        for (path, real) in [
            ("new.rs", "new.rs"),
            ("a.rs", "a.rs"),
            ("src/deep/er/new.rs", "src/deep/er/new.rs"),
        ] {
            let written = writable(&root, Path::new(path)).unwrap();
            assert_eq!(written.to_str(), root.join(real).to_str(), "{path}");
        }
    }

    #[test]
    fn test_a_write_may_not_leave_the_project() {
        let (_dir, root) = project();
        let outside = TempDir::new().unwrap();
        let elsewhere = outside.path().canonicalize().unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("out")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("none.txt"), root.join("dangling.txt")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        std::os::unix::fs::symlink(root.join(".git"), root.join("to_git")).unwrap();

        for path in [
            elsewhere.join("new.txt"),
            PathBuf::from("../new.txt"),
            PathBuf::from("no_dir/../../new.txt"),
            PathBuf::from("out/new.txt"),
            PathBuf::from("dangling.txt"),
            PathBuf::from(".git/hooks/pre-commit"),
            PathBuf::from("to_git/config"),
            PathBuf::from(".mimir/new"),
        ] {
            assert!(writable(&root, &path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn test_refuses_what_is_not_a_text_file_of_the_project() {
        let (_dir, root) = project();
        let outside = TempDir::new().unwrap();
        let other = outside.path().canonicalize().unwrap().join("other.txt");
        fs::write(&other, "secret\n").unwrap();
        std::os::unix::fs::symlink(&other, root.join("link.txt")).unwrap();

        for (path, message) in [
            (other.as_path(), "outside the project"),
            (Path::new("link.txt"), "outside the project"),
            (Path::new("data.bin"), "not text"),
            (Path::new("no_such.rs"), "not found"),
        ] {
            let error = read(&root, path, None, None).unwrap_err().to_string();
            assert!(error.contains(message), "{path:?}: {error}");
        }
    }
}
