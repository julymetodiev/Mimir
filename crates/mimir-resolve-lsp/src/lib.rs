//! A running language server as a semantic source, asked about each site the parser found, one open
//! file at a time, with its answers saved beside the graph (`docs/TYPESCRIPT.md`, section 4). Each
//! server has a module that says only what is its own.

mod ask;
mod pyrefly;
mod saved;
mod tsrs;

pub use pyrefly::pyrefly;
pub use tsrs::tsrs;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mimir_lsp::{LspError, Server, ServerSpec};
use mimir_resolve::{ProjectFile, Refresh, SemanticIndex, SemanticSource};
use mimir_types::{ContentHash, Language};
use serde_json::json;
use tracing::{info, warn};

use crate::saved::{Known, ProjectFiles};

/// What is particular to one language server.
struct AskedLanguage {
    /// Found on `PATH`, never in a project, which must not choose what the daemon runs.
    program: &'static str,
    args: &'static [&'static str],
    /// The version this was measured with. Another one still runs, and the daemon's log says so.
    tested_version: &'static str,
    /// The family of languages it answers for.
    family: Language,
    /// Their names, for the log.
    languages: &'static str,
    /// Files at the top of a project whose change can change any answer.
    project_files: &'static [&'static str],
    /// Where its answers are saved, in the cache directory.
    answers_file: &'static str,
    /// What a file is called in the protocol.
    language_id: fn(&Path) -> &'static str,
    /// Says so if a project lists packages that are not installed, where less can be answered.
    missing_packages: fn(&Path) -> Option<&'static str>,
}

/// A language server as a semantic source.
pub struct AskedSource {
    language: AskedLanguage,
    state: Mutex<State>,
    /// Whether a project had files of this language, which only then concerns a person.
    has_files: AtomicBool,
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

impl AskedSource {
    fn new(language: AskedLanguage) -> Self {
        Self {
            language,
            state: Mutex::default(),
            has_files: AtomicBool::new(false),
        }
    }
}

impl SemanticSource for AskedSource {
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<SemanticIndex> {
        let language = &self.language;
        let own: Vec<&ProjectFile> = files
            .iter()
            .filter(|f| Language::from_path(&f.path).family() == language.family)
            .collect();
        if own.is_empty() {
            return Vec::new();
        }
        self.has_files.store(true, Ordering::Relaxed);
        if refresh == Refresh::Saved {
            // Reading what is known must not wait for a server that is busy.
            let Ok(mut state) = self.state.try_lock() else {
                return Vec::new();
            };
            state.read_saved(language, cache_dir);
            return vec![state.index(&own)];
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.read_saved(language, cache_dir);
        if refresh == Refresh::Full {
            state.known.clear();
        }
        match state.bring_up_to_date(language, project, cache_dir, files, &own) {
            Ok(()) => vec![state.index(&own)],
            Err(LspError::Start { program, source }) => {
                warn!(
                    "No {program}, using syntax rules for {}: {source}",
                    language.languages
                );
                Vec::new()
            }
            Err(e) => {
                warn!(
                    "{} failed, using syntax rules for what it did not answer: {e}",
                    language.program
                );
                state.server = None;
                vec![state.index(&own)]
            }
        }
    }

    fn stop_if_idle(&self, idle: Duration) {
        if let Ok(mut state) = self.state.try_lock()
            && state.last_used.is_some_and(|used| used.elapsed() >= idle)
            && state.server.is_some()
        {
            info!("Stopping an idle {}", self.language.program);
            state.server = None;
            state.last_used = None;
        }
    }

    fn notes(&self, project: &Path) -> Vec<String> {
        if !self.has_files.load(Ordering::Relaxed) {
            return Vec::new();
        }
        (self.language.missing_packages)(project)
            .map(|note| format!("{}: {note}", self.language.languages))
            .into_iter()
            .collect()
    }
}

impl State {
    fn read_saved(&mut self, language: &AskedLanguage, cache_dir: &Path) {
        if !std::mem::replace(&mut self.read_saved, true)
            && let Some((known, project_files)) = saved::read(cache_dir, language.answers_file)
        {
            self.known = known;
            self.project_files = project_files;
        }
    }

    /// What is known about these files now: a file changed since is left to syntax rules.
    fn index(&self, own: &[&ProjectFile]) -> SemanticIndex {
        let mut index = SemanticIndex::default();
        for file in own {
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
        language: &AskedLanguage,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        own: &[&ProjectFile],
    ) -> Result<(), LspError> {
        let project = project.canonicalize().map_err(|source| LspError::Root {
            path: project.to_path_buf(),
            source,
        })?;
        let now = project_files(language, &project);
        if now != self.project_files {
            self.server = None;
            self.known.clear();
            self.project_files = now;
        }
        let changed: HashSet<&Path> = own
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
        let stale = ask::with_importers(own, &changed, &self.known);
        self.last_used = Some(Instant::now());
        let first_start = self.server.is_none();
        if first_start {
            self.server = Some(start(language, &project)?);
        }
        let Some(server) = self.server.as_mut() else {
            return Ok(());
        };
        if !first_start {
            let on_disk: Vec<PathBuf> = changed.iter().map(|p| project.join(p)).collect();
            server.files_changed(&on_disk)?;
        }
        let answered = ask::answers(server, language, &project, files, &stale)?;
        for (path, document) in answered {
            let Some(file) = own.iter().find(|f| f.path == path) else {
                continue;
            };
            let hash = ContentHash::compute(file.source.as_bytes());
            self.known.insert(path, (hash, Arc::new(document)));
        }
        let held: HashSet<&Path> = own.iter().map(|f| f.path.as_path()).collect();
        self.known.retain(|path, _| held.contains(path.as_path()));
        saved::write(
            cache_dir,
            language.answers_file,
            &self.known,
            &self.project_files,
        );
        self.last_used = Some(Instant::now());
        Ok(())
    }
}

fn start(language: &AskedLanguage, project: &Path) -> Result<Server, LspError> {
    let started = Instant::now();
    let program = language.program;
    let version = Command::new(program).arg("--version").output();
    if let Ok(output) = version
        && !says_version(
            &String::from_utf8_lossy(&output.stdout),
            program,
            language.tested_version,
        )
    {
        warn!(
            "{program} is not the tested version, {}",
            language.tested_version
        );
    }
    let server = Server::start(
        ServerSpec {
            program: program.to_string(),
            args: language.args.iter().map(ToString::to_string).collect(),
            configuration: json!({}),
            capabilities: json!({
                "general": {"positionEncodings": ["utf-8"]},
                "textDocument": {"definition": {"linkSupport": false}},
            }),
            status_method: None,
        },
        project,
    )?;
    info!(
        "{program} ready for {} in {:?}",
        project.display(),
        started.elapsed()
    );
    Ok(server)
}

/// Whether `--version` printed this version, and not one that starts with it.
fn says_version(output: &str, program: &str, version: &str) -> bool {
    let wanted = format!("{program} {version}");
    output.match_indices(&wanted).any(|(at, _)| {
        !output[at + wanted.len()..].starts_with(|c: char| c.is_ascii_digit() || c == '.')
    })
}

/// The configuration and lock files at the top of a project, and what they hold.
fn project_files(language: &AskedLanguage, project: &Path) -> Vec<(PathBuf, ContentHash)> {
    language
        .project_files
        .iter()
        .filter_map(|name| {
            let path = project.join(name);
            let bytes = std::fs::read(&path).ok()?;
            Some((path, ContentHash::compute(&bytes)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_told_from_one_that_starts_with_it() {
        assert!(says_version("tsrs 0.20.1-abc\n", "tsrs", "0.20.1"));
        assert!(says_version("pyrefly 1.4.0\n", "pyrefly", "1.4.0"));
        assert!(!says_version("tsrs 0.20.10\n", "tsrs", "0.20.1"));
        assert!(!says_version("tsrs 0.20.1.1\n", "tsrs", "0.20.1"));
        assert!(!says_version("other 0.20.1\n", "tsrs", "0.20.1"));
    }
}
