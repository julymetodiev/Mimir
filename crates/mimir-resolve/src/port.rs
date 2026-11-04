//! The port for a semantic source: what knows more than syntax about a project's files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::ProjectFile;

/// A semantic source. The engine asks it from a blocking task, and it may take minutes.
pub trait SemanticSource: Send + Sync {
    /// What is known about a project's files. Files in no index keep their syntax edges.
    /// With [`Refresh::Saved`] this returns at once, whatever else is running.
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<SemanticIndex>;

    /// Give up whatever is held for the next edit, if unused for `idle`.
    fn stop_if_idle(&self, idle: Duration);

    /// What a person should know about the answers for this project, as that its packages are not
    /// installed. It returns at once.
    fn notes(&self, _project: &Path) -> Vec<String> {
        Vec::new()
    }
}

/// Syntax rules only.
pub struct NoSemanticSource;

impl SemanticSource for NoSemanticSource {
    fn load(&self, _: &Path, _: &Path, _: &[ProjectFile], _: Refresh) -> Vec<SemanticIndex> {
        Vec::new()
    }

    fn stop_if_idle(&self, _: Duration) {}
}

/// Several sources as one. Each picks the files of its language out of what it is given, and each
/// works at the same time: one may take minutes.
pub struct SemanticSources {
    sources: Vec<Arc<dyn SemanticSource>>,
}

impl SemanticSources {
    pub fn new(sources: Vec<Arc<dyn SemanticSource>>) -> Self {
        Self { sources }
    }
}

impl SemanticSource for SemanticSources {
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<SemanticIndex> {
        std::thread::scope(|scope| {
            let running: Vec<_> = self
                .sources
                .iter()
                .map(|source| scope.spawn(move || source.load(project, cache_dir, files, refresh)))
                .collect();
            running
                .into_iter()
                .flat_map(|source| match source.join() {
                    Ok(indexes) => indexes,
                    Err(panic) => std::panic::resume_unwind(panic),
                })
                .collect()
        })
    }

    fn stop_if_idle(&self, idle: Duration) {
        for source in &self.sources {
            source.stop_if_idle(idle);
        }
    }

    fn notes(&self, project: &Path) -> Vec<String> {
        self.sources
            .iter()
            .flat_map(|source| source.notes(project))
            .collect()
    }
}

/// What a position in an edited file refers to. `line` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Path relative to the project. Most answers name a path and a name many others do.
    pub path: Arc<Path>,
    pub line: u32,
    pub name: Arc<str>,
}

/// The answers for one edited file.
#[derive(Debug, Clone, Default)]
pub struct LiveDocument {
    /// By a reference's start: 1-based line, 0-based column.
    pub sites: HashMap<(u32, u32), Target>,
    /// Every answer for a site that may have several. In `Config { name }` the name is a field and a
    /// variable at once, and the server gives both.
    pub all_sites: HashMap<(u32, u32), Vec<Target>>,
    /// The trait of each `impl Tr for T`, by the line of the `impl`.
    pub traits: HashMap<u32, Target>,
    /// What each method in the file implements, or is implemented by, by the 1-based line and column
    /// of its name.
    pub implementations: HashMap<(u32, u32), Vec<Target>>,
}

/// One occurrence of a symbol in a document. Lines and columns are 0-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub symbol: String,
    pub roles: u64,
}

/// A decoded index. Document paths are relative to `root`, itself relative to the project.
#[derive(Debug, Clone, Default)]
pub struct SemanticIndex {
    pub root: PathBuf,
    pub documents: HashMap<PathBuf, Vec<Occurrence>>,
    /// Files a running language server answered for, by path relative to the project: for Rust those
    /// edited since the index, and for a language with no index, every file.
    pub answers: HashMap<PathBuf, Arc<LiveDocument>>,
    /// Files edited since the index with no answers yet, which syntax rules resolve for now.
    pub unanswered: HashSet<PathBuf>,
    /// Pairs of symbols from an index: a method, and the method it implements.
    pub implementations: Vec<(String, String)>,
    /// What the packages of this root are imported as: in Go the module's path, with `root` its
    /// directory. Syntax rules go from an import to a directory by it.
    pub import_path: Option<String>,
}

/// Whether an index may be kept after an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Refresh {
    /// Keep it, and resolve edited files with a running analyzer.
    #[default]
    Edits,
    /// Write it again if anything changed.
    Full,
    /// Only what is on disk, at once: no analyzer, no new index, and edited files left to syntax.
    Saved,
}

impl SemanticIndex {
    pub fn covers(&self, path: &Path) -> bool {
        path.strip_prefix(&self.root)
            .is_ok_and(|p| self.documents.contains_key(p))
    }

    /// Whether a file changed since the index, so its entries here no longer hold.
    pub fn is_edited(&self, path: &Path) -> bool {
        self.answers.contains_key(path) || self.unanswered.contains(path)
    }

    /// Whether the index holds any file under a directory.
    pub fn reaches(&self, dir: &Path) -> bool {
        self.documents
            .keys()
            .any(|path| self.root.join(path).starts_with(dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        root: &'static str,
        idle: Mutex<Vec<Duration>>,
    }

    impl SemanticSource for Fake {
        fn load(&self, _: &Path, _: &Path, _: &[ProjectFile], _: Refresh) -> Vec<SemanticIndex> {
            vec![SemanticIndex {
                root: PathBuf::from(self.root),
                ..SemanticIndex::default()
            }]
        }

        fn stop_if_idle(&self, idle: Duration) {
            self.idle.lock().unwrap().push(idle);
        }
    }

    #[test]
    fn several_sources_answer_as_one_in_their_order() {
        let first = Arc::new(Fake {
            root: "a",
            ..Fake::default()
        });
        let second = Arc::new(Fake {
            root: "b",
            ..Fake::default()
        });
        let all = SemanticSources::new(vec![first.clone(), second.clone()]);

        let roots: Vec<PathBuf> = all
            .load(Path::new("."), Path::new("."), &[], Refresh::Saved)
            .into_iter()
            .map(|index| index.root)
            .collect();
        assert_eq!(roots, [PathBuf::from("a"), PathBuf::from("b")]);

        all.stop_if_idle(Duration::from_secs(1));
        assert_eq!(first.idle.lock().unwrap().len(), 1);
        assert_eq!(second.idle.lock().unwrap().len(), 1);
    }
}
