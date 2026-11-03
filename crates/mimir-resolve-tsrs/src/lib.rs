//! The semantic source for TypeScript and JavaScript: a running `tsrs`, asked about each site the
//! parser found, one open file at a time (`docs/TYPESCRIPT.md`, section 4).

mod ask;
mod saved;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mimir_lsp::{LspError, Server, ServerSpec};
use mimir_resolve::{ProjectFile, Refresh, SemanticIndex, SemanticSource};
use mimir_types::{ContentHash, Language};
use serde_json::json;
use tracing::{info, warn};

use crate::saved::{Known, ProjectFiles};

/// The version this was measured with, which `scripts/check-deps.py` compares with the newest.
/// Another one still runs, and the daemon's log says so.
const TESTED_VERSION: &str = "0.20.1";

/// Files whose change can change any answer.
const PROJECT_FILES: [&str; 6] = [
    "tsconfig.json",
    "jsconfig.json",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
];

/// `tsrs` as a semantic source.
#[derive(Default)]
pub struct Tsrs {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    server: Option<Server>,
    last_used: Option<Instant>,
    /// What each file held when it was asked about, and the answers.
    known: Known,
    project_files: ProjectFiles,
    /// Whether what a daemon before this one saved was looked for.
    read_saved: bool,
}

impl SemanticSource for Tsrs {
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<SemanticIndex> {
        let scripts: Vec<&ProjectFile> = files
            .iter()
            .filter(|f| Language::from_path(&f.path).family() == Language::TypeScript)
            .collect();
        if scripts.is_empty() {
            return Vec::new();
        }
        if refresh == Refresh::Saved {
            // Reading what is known must not wait for a server that is busy.
            let Ok(mut state) = self.state.try_lock() else {
                return Vec::new();
            };
            state.read_saved(cache_dir);
            return vec![state.index(&scripts)];
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.read_saved(cache_dir);
        if refresh == Refresh::Full {
            state.known.clear();
        }
        match state.bring_up_to_date(project, cache_dir, files, &scripts) {
            Ok(()) => vec![state.index(&scripts)],
            Err(LspError::Start { program, source }) => {
                warn!("No {program}, using syntax rules for TypeScript and JavaScript: {source}");
                Vec::new()
            }
            Err(e) => {
                warn!("tsrs failed, using syntax rules for what it did not answer: {e}");
                state.server = None;
                vec![state.index(&scripts)]
            }
        }
    }

    fn stop_if_idle(&self, idle: Duration) {
        if let Ok(mut state) = self.state.try_lock()
            && state.last_used.is_some_and(|used| used.elapsed() >= idle)
            && state.server.is_some()
        {
            info!("Stopping an idle tsrs");
            state.server = None;
            state.last_used = None;
        }
    }
}

impl State {
    fn read_saved(&mut self, cache_dir: &Path) {
        if !std::mem::replace(&mut self.read_saved, true)
            && let Some((known, project_files)) = saved::read(cache_dir)
        {
            self.known = known;
            self.project_files = project_files;
        }
    }

    /// What is known about these files now: a file changed since is left to syntax rules.
    fn index(&self, scripts: &[&ProjectFile]) -> SemanticIndex {
        let mut index = SemanticIndex::default();
        for file in scripts {
            let hash = ContentHash::compute(file.source.as_bytes());
            match self.known.get(&file.path) {
                Some((known, document)) if *known == hash => {
                    index.answers.insert(file.path.clone(), document.clone());
                }
                _ => {
                    index.unanswered.insert(file.path.clone());
                }
            }
        }
        index
    }

    fn bring_up_to_date(
        &mut self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        scripts: &[&ProjectFile],
    ) -> Result<(), LspError> {
        let project = project.canonicalize().map_err(|source| LspError::Root {
            path: project.to_path_buf(),
            source,
        })?;
        let now = project_files(&project);
        if now != self.project_files {
            self.server = None;
            self.known.clear();
            self.project_files = now;
        }
        let changed: HashSet<&Path> = scripts
            .iter()
            .filter(|f| {
                let hash = ContentHash::compute(f.source.as_bytes());
                self.known
                    .get(&f.path)
                    .is_none_or(|(known, _)| *known != hash)
            })
            .map(|f| f.path.as_path())
            .collect();
        if changed.is_empty() {
            return Ok(());
        }
        let stale = ask::with_importers(scripts, &changed, &self.known);
        self.last_used = Some(Instant::now());
        let first_start = self.server.is_none();
        if first_start {
            self.server = Some(start(&project)?);
        }
        let Some(server) = self.server.as_mut() else {
            return Ok(());
        };
        if !first_start {
            let on_disk: Vec<PathBuf> = changed.iter().map(|p| project.join(p)).collect();
            server.files_changed(&on_disk)?;
        }
        let answered = ask::answers(server, &project, files, &stale)?;
        for (path, document) in answered {
            let Some(file) = scripts.iter().find(|f| f.path == path) else {
                continue;
            };
            let hash = ContentHash::compute(file.source.as_bytes());
            self.known.insert(path, (hash, Arc::new(document)));
        }
        let held: HashSet<&Path> = scripts.iter().map(|f| f.path.as_path()).collect();
        self.known.retain(|path, _| held.contains(path.as_path()));
        saved::write(cache_dir, &self.known, &self.project_files);
        self.last_used = Some(Instant::now());
        Ok(())
    }
}

fn start(project: &Path) -> Result<Server, LspError> {
    let started = Instant::now();
    let version = Command::new("tsrs").arg("--version").output();
    let tested = format!("tsrs {TESTED_VERSION}-");
    if let Ok(output) = version
        && !String::from_utf8_lossy(&output.stdout).contains(&tested)
    {
        warn!("tsrs is not the tested version, {TESTED_VERSION}");
    }
    let server = Server::start(
        ServerSpec {
            program: "tsrs".to_string(),
            args: vec!["--lsp".to_string(), "-stdio".to_string()],
            configuration: json!({}),
            capabilities: json!({
                "general": {"positionEncodings": ["utf-8"]},
                "textDocument": {"definition": {"linkSupport": false}},
            }),
            status_method: None,
        },
        project,
    )?;
    if server.capabilities()["positionEncoding"] != "utf-8" {
        warn!("tsrs does not offer byte columns, so columns after other than ASCII will be off");
    }
    info!(
        "tsrs ready for {} in {:?}",
        project.display(),
        started.elapsed()
    );
    Ok(server)
}

/// The configuration and lock files at the top of a project, and what they hold.
fn project_files(project: &Path) -> Vec<(PathBuf, ContentHash)> {
    PROJECT_FILES
        .iter()
        .filter_map(|name| {
            let path = project.join(name);
            let bytes = std::fs::read(&path).ok()?;
            Some((path, ContentHash::compute(&bytes)))
        })
        .collect()
}
