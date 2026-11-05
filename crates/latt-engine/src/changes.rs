//! What git says changed: the files that differ from a commit, with their text in that commit.

use crate::error::GraphError;
use latt_types::Language;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A file that differs from the commit compared with.
pub struct ChangedFile {
    /// The file, relative to the project.
    pub path: PathBuf,
    /// Its text in that commit, if it was there, is text, and is in a language that may be parsed.
    pub before: Option<String>,
    /// It is still there.
    pub exists: bool,
}

/// What differs from a commit.
pub struct Changes {
    /// The commit.
    pub base: String,
    /// The files, by path.
    pub files: Vec<ChangedFile>,
}

/// What differs from the last commit, or from where `base` split off from it, untracked files too.
pub fn read(root: &Path, base: Option<&str>) -> Result<Changes, GraphError> {
    let head = commit(root, "HEAD")?;
    let base = match base {
        Some(base) => {
            let base = commit(root, base)?;
            text_of(git(root, &["merge-base", &base, &head])?)
                .trim()
                .to_string()
        }
        None => head,
    };
    let listed = git(
        root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--relative",
            &base,
            "--",
        ],
    )?;
    let listed = text_of(listed);
    let mut fields = listed.split('\0');
    let mut files = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let path = PathBuf::from(path);
        let before = match status {
            "A" => None,
            _ => text_in(root, &base, &path)?,
        };
        let exists = status != "D";
        files.push(ChangedFile {
            path,
            before,
            exists,
        });
    }
    let untracked = text_of(git(
        root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?);
    files.extend(
        untracked
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| ChangedFile {
                path: PathBuf::from(path),
                before: None,
                exists: true,
            }),
    );
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Changes { base, files })
}

/// The commit a name means. `--end-of-options` keeps a name from being taken as an option.
fn commit(root: &Path, name: &str) -> Result<String, GraphError> {
    let wanted = format!("{name}^{{commit}}");
    let found = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &wanted],
    )?;
    Ok(text_of(found).trim().to_string())
}

/// A file's text in a commit, unless it is not text or Lantern Lattice cannot parse its language.
fn text_in(root: &Path, commit: &str, path: &Path) -> Result<Option<String>, GraphError> {
    if Language::from_path(path) == Language::Unknown {
        return Ok(None);
    }
    let wanted = format!("{commit}:./{}", path.display());
    Ok(String::from_utf8(git(root, &["show", &wanted])?).ok())
}

fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn git(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, GraphError> {
    let command = arguments.first().copied().unwrap_or_default().to_string();
    let output = Command::new("git")
        .current_dir(root)
        .args(arguments)
        .output()
        .map_err(|source| GraphError::Git {
            command: command.clone(),
            reason: source.to_string(),
        })?;
    if !output.status.success() {
        return Err(GraphError::Git {
            command,
            reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output.stdout)
}
