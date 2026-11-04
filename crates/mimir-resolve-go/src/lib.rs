//! Go as a semantic source (`docs/GO.md`, section 4): an index of each `go.mod` by `scip-go`, and for
//! the files edited since, a running `gopls`.

mod gopls;
mod index;
mod roots;
mod symbols;
mod tools;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use mimir_resolve::{ProjectFile, Refresh, SemanticIndex, SemanticSource};
use mimir_resolve_scip::{Answered, IndexFiles, IndexLanguage, Running, ScipError, load_root};
use mimir_types::Language;
use tracing::{info, warn};

use crate::gopls::Gopls;

/// `scip-go` and `gopls` as a semantic source.
#[derive(Default)]
pub struct GoSource {
    servers: Mutex<Running<Gopls>>,
    notes: Mutex<Vec<String>>,
}

impl SemanticSource for GoSource {
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<SemanticIndex> {
        let found = roots::of_files(project, files);
        if found.roots.is_empty() && found.owners.is_empty() {
            return Vec::new();
        }
        // Reading what is saved uses no server, so it must not wait for one that is busy.
        if refresh == Refresh::Saved {
            let mut none = Running::default();
            return load_all(project, cache_dir, files, &found, &mut none, refresh).0;
        }
        let mut servers = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        let (indexes, notes) = load_all(project, cache_dir, files, &found, &mut servers, refresh);
        *self.notes.lock().unwrap_or_else(|e| e.into_inner()) = notes;
        indexes
    }

    fn stop_if_idle(&self, idle: Duration) {
        if let Ok(mut servers) = self.servers.try_lock()
            && let Some(stopped) = servers.stop_if_idle(idle)
        {
            info!("Stopping {stopped} idle gopls");
        }
    }

    fn notes(&self, _project: &Path) -> Vec<String> {
        self.notes.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// One root at a time, the fewest files first: each takes all cores, and later ones reuse what
/// earlier ones compiled. A root without an index still says what it is imported as.
fn load_all(
    project: &Path,
    cache_dir: &Path,
    files: &[ProjectFile],
    found: &roots::Roots,
    servers: &mut Running<Gopls>,
    refresh: Refresh,
) -> (Vec<SemanticIndex>, Vec<String>) {
    let mut indexes = Vec::new();
    let mut notes = Vec::new();
    let mut tool_missing = false;
    for root in &found.roots {
        let module = roots::module_path(&project.join(&root.path).join("go.mod"));
        let without_index = || SemanticIndex {
            root: root.path.clone(),
            import_path: module.clone(),
            ..SemanticIndex::default()
        };
        if tool_missing {
            indexes.push(without_index());
            continue;
        }
        let mut language = Go {
            root: &root.path,
            owners: &found.owners,
            servers: &mut *servers,
        };
        match load_root(
            &mut language,
            project,
            cache_dir,
            &root.path,
            files,
            refresh,
        ) {
            Ok(mut index) => {
                index.import_path = module.clone();
                notes.extend(index::incomplete_packages(cache_dir, &root.path));
                indexes.push(index);
            }
            Err(ScipError::ToolMissing(e)) => {
                warn!("No index for Go, using syntax rules: {e}");
                notes.push(format!("Go: syntax rules only. {e}"));
                tool_missing = true;
                indexes.push(without_index());
            }
            Err(ScipError::Failed(e)) => {
                warn!(
                    "No index for Go in {:?}, using syntax rules: {e}",
                    root.path
                );
                notes.push(format!(
                    "Go: {} has syntax rules only. {e}",
                    roots::shown(&root.path)
                ));
                indexes.push(without_index());
            }
            Err(ScipError::NotSaved) => indexes.push(without_index()),
        }
    }
    let outside = outside_the_build(files, &indexes, &found.owners);
    if outside > 0 && !tool_missing {
        notes.push(format!(
            "Go: {outside} files are outside the build, behind a build tag or for another system, and have syntax edges"
        ));
    }
    (indexes, notes)
}

/// Go files in a module that no index holds and no server answered for.
fn outside_the_build(
    files: &[ProjectFile],
    indexes: &[SemanticIndex],
    owners: &HashMap<&Path, PathBuf>,
) -> usize {
    files
        .iter()
        .filter(|f| owners.contains_key(f.path.as_path()))
        .filter(|f| {
            let path = f.path.as_path();
            indexes.iter().any(|index| !index.documents.is_empty())
                && !indexes
                    .iter()
                    .any(|index| index.covers(path) || index.answers.contains_key(path))
        })
        .count()
}

struct Go<'a> {
    root: &'a Path,
    owners: &'a HashMap<&'a Path, PathBuf>,
    servers: &'a mut Running<Gopls>,
}

impl IndexLanguage for Go<'_> {
    fn stem_prefix(&self) -> &'static str {
        index::STEM_PREFIX
    }

    /// A nested module is a root of its own.
    fn owns(&self, path: &Path) -> bool {
        Language::from_path(path) == Language::Go
            && self.owners.get(path).is_some_and(|root| root == self.root)
    }

    fn read_back(&self, index: &mut SemanticIndex) {
        symbols::drop_versions(index);
    }

    fn manifests(&self, root: &Path, _sources: &[&Path]) -> Vec<PathBuf> {
        roots::manifests(root)
    }

    fn write_index(&mut self, root: &Path, paths: &IndexFiles) -> Result<(), ScipError> {
        index::write(root, paths)
    }

    fn answer(
        &mut self,
        project: &Path,
        root: &Path,
        files: &[ProjectFile],
        asked: &[&ProjectFile],
    ) -> Result<Answered, String> {
        gopls::answer(self.servers, project, root, files, asked)
    }
}
