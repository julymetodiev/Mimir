//! The choice between a kept index, a running server and a new index, for one root.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mimir_lsp::{Stopping, stop};
use mimir_resolve::{ProjectFile, Refresh, SemanticIndex};
use mimir_types::ContentHash;
use tracing::{info, warn};

use crate::decode::decode;
use crate::snapshot::Snapshot;
use crate::{Answered, IndexLanguage, ScipError};

/// An edit can change what files outside it resolve to, so edits may not pile up forever.
const MAX_EDITED_FILES: usize = 50;

/// The files of one root's index, in the cache directory.
pub struct IndexFiles {
    pub index: PathBuf,
    pub log: PathBuf,
    key: PathBuf,
    /// The list of [`IndexLanguage::suspects`]. Failed macros are why there is one, so its name.
    suspects: PathBuf,
    stem: PathBuf,
}

impl IndexFiles {
    /// The files of a root's index with this prefix.
    pub fn of(cache_dir: &Path, prefix: &str, root: &Path) -> Self {
        let hash = ContentHash::compute(root.to_string_lossy().as_bytes()).to_hex();
        let stem = cache_dir.join(format!("{prefix}-{hash}"));
        let with = |extension: &str| stem.with_extension(extension);
        Self {
            index: with("scip"),
            key: with("key"),
            log: with("log"),
            suspects: with("macros"),
            stem: stem.clone(),
        }
    }

    /// Another file a language keeps for this root.
    pub fn other(&self, extension: &str) -> PathBuf {
        self.stem.with_extension(extension)
    }
}

/// Take in what a server said.
fn take_answers(index: &mut SemanticIndex, answered: Answered) {
    index.answers = answered
        .documents
        .into_iter()
        .map(|(path, document)| (path, Arc::new(document)))
        .collect();
    index.unanswered.extend(answered.unanswered);
}

fn remove_if_there(path: &Path) -> Result<(), ScipError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(ScipError::Failed(format!("{}: {e}", path.display())))
        }
        _ => Ok(()),
    }
}

/// The index of one root, relative to the project, with answers for what changed since it.
pub fn load_root<L: IndexLanguage + ?Sized>(
    language: &mut L,
    project: &Path,
    cache_dir: &Path,
    root: &Path,
    files: &[ProjectFile],
    refresh: Refresh,
) -> Result<SemanticIndex, ScipError> {
    let abs = project.join(root);
    let snapshot = Snapshot::of(language, project, root, files);
    let paths = IndexFiles::of(cache_dir, language.stem_prefix(), root);
    let read = |language: &L| -> Result<SemanticIndex, ScipError> {
        let bytes = fs::read(&paths.index).map_err(|e| ScipError::Failed(e.to_string()))?;
        let mut index = decode(&bytes)?;
        index.root = root.to_path_buf();
        language.read_back(&mut index);
        Ok(index)
    };

    let saved = fs::read_to_string(&paths.key)
        .ok()
        .filter(|_| paths.index.is_file())
        .map(|text| Snapshot::parse(&text));
    let suspects = || listed(&paths.suspects, files);
    if refresh == Refresh::Saved {
        let saved = saved.ok_or(ScipError::NotSaved)?;
        let mut index = read(language)?;
        let edited = edited_files(&index, &saved, &snapshot, files);
        index.unanswered = edited
            .into_iter()
            .chain(suspects())
            .map(|f| f.path.clone())
            .collect();
        return Ok(index);
    }
    let usable = saved.filter(|saved| match refresh {
        Refresh::Full => *saved == snapshot,
        _ => saved.manifests == snapshot.manifests,
    });
    if let Some(saved) = usable {
        let mut index = read(language)?;
        let edited = edited_files(&index, &saved, &snapshot, files);
        if edited.len() <= MAX_EDITED_FILES {
            let edits_only = edited.is_empty();
            let asked: Vec<_> = union(edited, suspects());
            if asked.is_empty() {
                return Ok(index);
            }
            match language.answer(project, &abs, files, &asked) {
                Ok(answered) => {
                    take_answers(&mut index, answered);
                    return Ok(index);
                }
                // A new index would not help files whose macros may fail again.
                Err(e) if edits_only => {
                    warn!(
                        "No running server for {:?}, keeping what the index says: {e}",
                        root
                    );
                    return Ok(index);
                }
                Err(e) => warn!("No running server for {:?}, writing a new index: {e}", root),
            }
        }
    }

    fs::create_dir_all(cache_dir).map_err(|e| ScipError::Failed(e.to_string()))?;
    remove_if_there(&paths.key)?;
    remove_if_there(&paths.suspects)?;
    let started = Instant::now();
    language.write_index(&abs, &paths)?;
    // Writing may change a manifest, as `go` fills `go.work.sum`: taken before, that would call for
    // another index at once.
    let snapshot = Snapshot::of(language, project, root, files);
    fs::write(&paths.key, snapshot.text()).map_err(|e| ScipError::Failed(e.to_string()))?;
    info!(
        "Semantic index for {:?} written in {:?}",
        root,
        started.elapsed()
    );

    let mut index = read(language)?;
    let suspects = language.suspects(&paths.log, root, &index, files);
    if !suspects.is_empty() {
        let list: String = suspects
            .iter()
            .map(|f| format!("{}\n", f.path.display()))
            .collect();
        fs::write(&paths.suspects, list).map_err(|e| ScipError::Failed(e.to_string()))?;
        match language.answer(project, &abs, files, &suspects) {
            Ok(answered) => take_answers(&mut index, answered),
            Err(e) => warn!(
                "No running server for {:?}, keeping what the index says: {e}",
                root
            ),
        }
    }
    Ok(index)
}

fn union<'a>(mut files: Vec<&'a ProjectFile>, more: Vec<&'a ProjectFile>) -> Vec<&'a ProjectFile> {
    for file in more {
        if !files.iter().any(|f| f.path == file.path) {
            files.push(file);
        }
    }
    files
}

/// The files recorded as ones a new index cannot be trusted on.
fn listed<'a>(list: &Path, files: &'a [ProjectFile]) -> Vec<&'a ProjectFile> {
    let Ok(list) = fs::read_to_string(list) else {
        return Vec::new();
    };
    let listed: HashSet<&Path> = list.lines().map(Path::new).collect();
    files
        .iter()
        .filter(|f| listed.contains(f.path.as_path()))
        .collect()
}

/// Files whose index entries no longer hold: changed ones it covered, and new ones.
/// A changed file it did not cover is behind `cfg`, where an analyzer knows no more than it did.
fn edited_files<'a>(
    index: &SemanticIndex,
    saved: &Snapshot,
    now: &Snapshot,
    files: &'a [ProjectFile],
) -> Vec<&'a ProjectFile> {
    files
        .iter()
        .filter(|f| {
            let current = now.sources.get(&f.path);
            let before = saved.sources.get(&f.path);
            current.is_some() && current != before && (before.is_none() || index.covers(&f.path))
        })
        .collect()
}

/// Wait for a tool until a moment, and stop it if that comes first or the daemon is stopping.
pub fn wait_for(
    child: &mut Child,
    until: Instant,
    tool: &str,
    stopping: &Stopping,
) -> Result<(), ScipError> {
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("{tool} exited with {status}").into()),
            None if stopping.has_begun() => {
                stop(child);
                return Err(format!("{tool} was stopped: the daemon is stopping").into());
            }
            None if Instant::now() > until => {
                stop(child);
                return Err(format!("{tool} timed out").into());
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tool that changes a manifest while it writes an index, as `go list` fills `go.work.sum`.
    struct Filling {
        project: PathBuf,
        written: usize,
    }

    impl IndexLanguage for Filling {
        fn stem_prefix(&self) -> &'static str {
            "test"
        }

        fn manifests(&self, root: &Path, _sources: &[&Path]) -> Vec<PathBuf> {
            vec![root.join("go.work.sum")]
        }

        fn write_index(&mut self, _root: &Path, paths: &IndexFiles) -> Result<(), ScipError> {
            self.written += 1;
            fs::write(self.project.join("go.work.sum"), "filled").unwrap();
            fs::write(&paths.index, []).unwrap();
            Ok(())
        }

        fn answer(
            &mut self,
            _project: &Path,
            _root: &Path,
            _files: &[ProjectFile],
            _asked: &[&ProjectFile],
        ) -> Result<Answered, String> {
            Ok(Answered::default())
        }
    }

    #[test]
    fn a_tool_ends_when_the_daemon_stops() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let stopping = Stopping::default();
        stopping.begin();
        let started = Instant::now();
        let until = started + Duration::from_secs(60);
        assert!(wait_for(&mut child, until, "sleep", &stopping).is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(child.try_wait().unwrap().is_some(), "it is still running");
    }

    #[test]
    fn what_writing_an_index_changes_does_not_call_for_another() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let mut language = Filling {
            project: project.path().to_path_buf(),
            written: 0,
        };
        let root = Path::new("");
        for _ in 0..2 {
            load_root(
                &mut language,
                project.path(),
                cache.path(),
                root,
                &[],
                Refresh::Edits,
            )
            .unwrap();
        }
        assert_eq!(language.written, 1);

        fs::write(project.path().join("go.work.sum"), "changed by a person").unwrap();
        load_root(
            &mut language,
            project.path(),
            cache.path(),
            root,
            &[],
            Refresh::Edits,
        )
        .unwrap();
        assert_eq!(language.written, 2, "a later change still does");
    }
}
