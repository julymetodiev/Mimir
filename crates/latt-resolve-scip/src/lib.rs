//! A SCIP index as a semantic source, and what does not depend on the language it was written for:
//! reading it, keeping it by a hash of the sources, and choosing between a kept index, a running
//! server and a new index. Each language says what is its own through [`IndexLanguage`].

mod decode;
mod load;
mod running;
mod snapshot;
mod version;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use latt_resolve::{LiveDocument, ProjectFile, SemanticIndex};

pub use decode::decode;
pub use load::{IndexFiles, load_root, wait_for};
pub use running::Running;
pub use version::check_version;

/// Why a root has no index.
#[derive(Debug, thiserror::Error)]
pub enum ScipError {
    /// The tool is not installed, so no other root can do better.
    #[error("{0}")]
    ToolMissing(String),
    #[error("{0}")]
    Failed(String),
    /// [`latt_resolve::Refresh::Saved`] found no index.
    #[error("no saved index")]
    NotSaved,
}

impl From<String> for ScipError {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

/// What a running server said of the files it was asked about.
#[derive(Default)]
pub struct Answered {
    /// By path relative to the project.
    pub documents: HashMap<PathBuf, LiveDocument>,
    /// Files it has no answer for, which syntax rules resolve: the index no longer holds for them.
    pub unanswered: Vec<PathBuf>,
}

/// What is one language's own in the life of an index.
pub trait IndexLanguage {
    /// How the files of its indexes start in the cache directory.
    fn stem_prefix(&self) -> &'static str;

    /// Whether a file under the root belongs to its index, or to something else that is there.
    fn owns(&self, _path: &Path) -> bool {
        true
    }

    /// Make what was read from an index the form this language links in.
    fn read_back(&self, _index: &mut SemanticIndex) {}

    /// The files, relative to the project, whose change means a new index for a root.
    fn manifests(&self, root: &Path, sources: &[&Path]) -> Vec<PathBuf>;

    /// Write a new index, and whatever goes with it, for the root at this absolute path.
    fn write_index(&mut self, root: &Path, paths: &IndexFiles) -> Result<(), ScipError>;

    /// Files a new index cannot be trusted on, which go to a running server from now on.
    fn suspects<'f>(
        &self,
        _log: &Path,
        _root: &Path,
        _index: &SemanticIndex,
        _files: &'f [ProjectFile],
    ) -> Vec<&'f ProjectFile> {
        Vec::new()
    }

    /// What a running server says of the files in `asked`, for the root at this absolute path.
    fn answer(
        &mut self,
        project: &Path,
        root: &Path,
        files: &[ProjectFile],
        asked: &[&ProjectFile],
    ) -> Result<Answered, String>;
}
