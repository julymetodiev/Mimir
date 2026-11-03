//! The port for a semantic source: what knows more than syntax about a project's files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    ) -> Vec<ScipIndex>;

    /// Give up whatever is held for the next edit, if unused for `idle`.
    fn stop_if_idle(&self, idle: Duration);
}

/// Syntax rules only.
pub struct NoSemanticSource;

impl SemanticSource for NoSemanticSource {
    fn load(&self, _: &Path, _: &Path, _: &[ProjectFile], _: Refresh) -> Vec<ScipIndex> {
        Vec::new()
    }

    fn stop_if_idle(&self, _: Duration) {}
}

/// What a position in an edited file refers to. `line` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Path relative to the project.
    pub path: PathBuf,
    pub line: u32,
    pub name: String,
}

/// The answers for one edited file.
#[derive(Debug, Clone, Default)]
pub struct LiveDocument {
    /// By a reference's start: 1-based line, 0-based column.
    pub sites: HashMap<(u32, u32), Target>,
    /// The trait of each `impl Tr for T`, by the line of the `impl`.
    pub traits: HashMap<u32, Target>,
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
pub struct ScipIndex {
    pub root: PathBuf,
    pub documents: HashMap<PathBuf, Vec<Occurrence>>,
    /// Files edited since the index, by path relative to the project.
    pub live: HashMap<PathBuf, LiveDocument>,
    /// Files edited since the index with no answers yet, which syntax rules resolve for now.
    pub unanswered: HashSet<PathBuf>,
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

impl ScipIndex {
    pub fn covers(&self, path: &Path) -> bool {
        path.strip_prefix(&self.root)
            .is_ok_and(|p| self.documents.contains_key(p))
    }

    /// Whether a file changed since the index, so its entries here no longer hold.
    pub fn is_edited(&self, path: &Path) -> bool {
        self.live.contains_key(path) || self.unanswered.contains(path)
    }

    /// Whether the index holds any file under a directory.
    pub fn reaches(&self, dir: &Path) -> bool {
        self.documents
            .keys()
            .any(|path| self.root.join(path).starts_with(dir))
    }
}
