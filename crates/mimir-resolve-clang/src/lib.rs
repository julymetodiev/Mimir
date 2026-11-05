//! C and C++ as a semantic source (`docs/CPP.md`, section 4): an index of the project by `clangd-indexer`,
//! from its compilation database, and `clangd` for what that leaves out. Mimir never runs what makes a
//! database, as that runs the project's code.

mod clangd;
mod database;
mod dex;
mod index;
mod scip;
mod sites;
mod tools;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use mimir_lsp::Stopping;
use mimir_resolve::{ProjectFile, Refresh, SemanticIndex, SemanticSource};
use mimir_resolve_scip::{Answered, IndexFiles, IndexLanguage, Running, ScipError, load_root};
use mimir_types::Language;
use tracing::{info, warn};

use crate::clangd::Clangd;

/// `clangd-indexer` as a semantic source.
#[derive(Default)]
pub struct ClangSource {
    /// Held while an index is written too: the engine loads from several threads, and two would write
    /// and remove the same files.
    servers: Mutex<Running<Clangd>>,
    notes: Mutex<Vec<String>>,
    stopping: Stopping,
}

fn is_ours(path: &Path) -> bool {
    Language::from_path(path).family() == Language::C
}

impl SemanticSource for ClangSource {
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<SemanticIndex> {
        if !files.iter().any(|f| is_ours(&f.path)) {
            return Vec::new();
        }
        // Reading what is saved uses no server, so it must not wait for one that is busy.
        if refresh == Refresh::Saved {
            let mut none = Running::default();
            return load_project(
                project,
                cache_dir,
                files,
                &mut none,
                refresh,
                &self.stopping,
            )
            .0;
        }
        let mut servers = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        let (indexes, notes) = load_project(
            project,
            cache_dir,
            files,
            &mut servers,
            refresh,
            &self.stopping,
        );
        *self.notes.lock().unwrap_or_else(|e| e.into_inner()) = notes;
        indexes
    }

    fn stop_if_idle(&self, idle: Duration) {
        if let Ok(mut servers) = self.servers.try_lock()
            && let Some(stopped) = servers.stop_if_idle(idle)
        {
            info!("Stopping {stopped} idle clangd");
        }
    }

    fn stop(&self) {
        self.stopping.begin();
    }

    fn notes(&self, _project: &Path) -> Vec<String> {
        self.notes.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn load_project(
    project: &Path,
    cache_dir: &Path,
    files: &[ProjectFile],
    servers: &mut Running<Clangd>,
    refresh: Refresh,
    stopping: &Stopping,
) -> (Vec<SemanticIndex>, Vec<String>) {
    let Some(database) = database::find(project) else {
        return (Vec::new(), vec![database::missing_note()]);
    };
    let mut language = Clang {
        database,
        cache_dir,
        servers,
        files,
        stopping,
    };
    let root = PathBuf::new();
    let database = project.join(&language.database);
    match load_root(&mut language, project, cache_dir, &root, files, refresh) {
        Ok(index) => {
            let paths = IndexFiles::of(cache_dir, index::STEM_PREFIX, &root);
            let notes = index::failure_note(&paths.log, &database);
            (vec![index], notes.into_iter().collect())
        }
        Err(ScipError::NotSaved) => (Vec::new(), Vec::new()),
        Err(e) => {
            warn!("No index for C and C++, using syntax rules: {e}");
            (
                Vec::new(),
                vec![format!("C and C++: syntax rules only. {e}")],
            )
        }
    }
}

struct Clang<'a> {
    /// Relative to the project.
    database: PathBuf,
    cache_dir: &'a Path,
    servers: &'a mut Running<Clangd>,
    files: &'a [ProjectFile],
    stopping: &'a Stopping,
}

impl IndexLanguage for Clang<'_> {
    fn stem_prefix(&self) -> &'static str {
        index::STEM_PREFIX
    }

    fn owns(&self, path: &Path) -> bool {
        is_ours(path)
    }

    fn manifests(&self, _root: &Path, _sources: &[&Path]) -> Vec<PathBuf> {
        vec![self.database.clone()]
    }

    fn write_index(&mut self, root: &Path, paths: &IndexFiles) -> Result<(), ScipError> {
        index::write(
            root,
            &root.join(&self.database),
            self.files,
            paths,
            self.stopping,
        )
    }

    fn answer(
        &mut self,
        project: &Path,
        root: &Path,
        files: &[ProjectFile],
        asked: &[&ProjectFile],
    ) -> Result<Answered, String> {
        let paths = IndexFiles::of(self.cache_dir, index::STEM_PREFIX, Path::new(""));
        let database = root.join(&self.database);
        clangd::answer(
            self.servers,
            project,
            &database,
            &paths,
            files,
            asked,
            self.stopping,
        )
    }
}
