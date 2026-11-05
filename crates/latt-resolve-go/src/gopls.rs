//! Edited files, resolved by a running `gopls` instead of a new index (`docs/GO.md`, section 4).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use latt_lsp::{Location, LspError, Server, ServerSpec, Stopping};
use latt_resolve::{LiveDocument, ProjectFile, Target};
use latt_resolve_scip::{Answered, Running};
use latt_types::ReferenceKind;
use serde_json::json;
use tracing::{info, warn};

use crate::tools;

/// What `gopls` says of a file it is still loading, and of one outside the build.
const NO_METADATA: &str = "no package metadata";
/// How long files with that error wait before they are asked once more.
const SECOND_TRY_AFTER: Duration = Duration::from_secs(5);

/// One `gopls`. Dropping it ends the process.
pub struct Gopls {
    server: Server,
}

impl Gopls {
    /// `zeroConfig: false`, since otherwise opening a file for another system makes a view for that
    /// system and more memory is held.
    fn start(root: &Path, stopping: &Stopping) -> Result<Self, LspError> {
        tools::check_gopls();
        let spec = ServerSpec {
            program: tools::GOPLS.to_string(),
            args: Vec::new(),
            configuration: json!({"zeroConfig": false}),
            capabilities: json!({
                "workspace": {"configuration": true},
                "textDocument": {
                    "definition": {"linkSupport": false},
                    "implementation": {"linkSupport": false},
                },
            }),
            status_method: None,
            stopping: stopping.clone(),
        };
        let server = Server::start(spec, root)?;
        info!("gopls ready for {}", root.display());
        Ok(Self { server })
    }
}

/// What `gopls` says of the files in `asked`. Without it, those files have syntax rules until the
/// next index. An error is not an answer: a file that still has one has syntax edges as well.
pub fn answer(
    servers: &mut Running<Gopls>,
    project: &Path,
    root: &Path,
    files: &[ProjectFile],
    asked: &[&ProjectFile],
    stopping: &Stopping,
) -> Result<Answered, String> {
    let project = project.canonicalize().map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let sources: HashMap<&Path, &str> = files
        .iter()
        .map(|f| (f.path.as_path(), &*f.source))
        .collect();
    let result = servers.with(
        &root,
        || Gopls::start(&root, stopping),
        |gopls| ask_all(&mut gopls.server, &project, &sources, asked),
    );
    match result {
        Ok(answered) => Ok(answered),
        Err(LspError::Start { program, source }) => {
            warn!(
                "No {program}, so edited Go files have syntax rules until the next index: {source}"
            );
            Ok(Answered {
                unanswered: asked.iter().map(|f| f.path.clone()).collect(),
                ..Answered::default()
            })
        }
        Err(e) => Err(e.to_string()),
    }
}

fn ask_all(
    server: &mut Server,
    project: &Path,
    sources: &HashMap<&Path, &str>,
    asked: &[&ProjectFile],
) -> Result<Answered, LspError> {
    for file in asked {
        server.update(&project.join(&file.path), "go", &file.source)?;
    }
    let mut answered = Answered::default();
    let mut later = Vec::new();
    for file in asked {
        match ask_file(server, project, sources, file)? {
            Some(document) => {
                answered.documents.insert(file.path.clone(), document);
            }
            None => later.push(*file),
        }
    }
    if !later.is_empty() {
        // Opening a file it has not read counts as a new file, and it drops what it knows of that
        // package and of every package that imports it until it has loaded them again.
        std::thread::sleep(SECOND_TRY_AFTER);
    }
    for file in later {
        match ask_file(server, project, sources, file)? {
            Some(document) => {
                answered.documents.insert(file.path.clone(), document);
            }
            None => answered.unanswered.push(file.path.clone()),
        }
    }
    Ok(answered)
}

/// `None` if the file has `no package metadata`.
fn ask_file(
    server: &mut Server,
    project: &Path,
    sources: &HashMap<&Path, &str>,
    file: &ProjectFile,
) -> Result<Option<LiveDocument>, LspError> {
    let path = project.join(&file.path);
    let lines: Vec<&str> = file.source.split('\n').collect();
    let sites: Vec<(u32, u32)> = file
        .references
        .iter()
        .filter(|r| {
            matches!(
                r.kind,
                ReferenceKind::Call
                    | ReferenceKind::FunctionValue
                    | ReferenceKind::TypeUse
                    | ReferenceKind::FieldUse
                    | ReferenceKind::FieldWrite
            )
        })
        .map(|r| (r.range.start_line, r.range.start_col))
        .collect();
    let methods: Vec<(u32, u32)> = file
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::MethodDeclaration)
        .map(|r| (r.range.start_line, r.range.start_col))
        .collect();
    let names = Names::new(project, sources);
    let mut document = LiveDocument::default();
    let Some(definitions) = ask(server, "textDocument/definition", &path, &lines, &sites)? else {
        return Ok(None);
    };
    for (site, found) in sites.iter().zip(definitions) {
        document.all_sites.insert(*site, names.targets(found));
    }
    let Some(implementations) = ask(
        server,
        "textDocument/implementation",
        &path,
        &lines,
        &methods,
    )?
    else {
        return Ok(None);
    };
    for (method, found) in methods.iter().zip(implementations) {
        document
            .implementations
            .insert(*method, names.targets(found));
    }
    Ok(Some(document))
}

/// One method at 1-based sites with byte columns. `None` if `gopls` has no metadata for the file.
/// Any other error is about that site: it has no answer, as for a name it cannot place.
fn ask(
    server: &mut Server,
    method: &str,
    path: &Path,
    lines: &[&str],
    sites: &[(u32, u32)],
) -> Result<Option<Vec<Vec<Location>>>, LspError> {
    let positions: Vec<(u32, u32)> = sites
        .iter()
        .map(|&(line, column)| {
            let text = lines.get(line as usize - 1).copied().unwrap_or("");
            (line - 1, utf16_column(text, column))
        })
        .collect();
    let mut found = Vec::with_capacity(sites.len());
    for answer in server.each(method, path, &positions)? {
        match answer {
            Ok(locations) => found.push(locations),
            Err(message) if message.contains(NO_METADATA) => return Ok(None),
            Err(_) => found.push(Vec::new()),
        }
    }
    Ok(Some(found))
}

/// A column in UTF-16 units, the only encoding `gopls` offers, from one in bytes.
fn utf16_column(line: &str, bytes: u32) -> u32 {
    let before = line.get(..bytes as usize).unwrap_or(line);
    before.encode_utf16().count() as u32
}

/// A column in bytes, from one in UTF-16 units.
fn byte_column(line: &str, units: u32) -> usize {
    let mut seen = 0;
    for (at, c) in line.char_indices() {
        if seen >= units {
            return at;
        }
        seen += c.len_utf16() as u32;
    }
    line.len()
}

/// From a place `gopls` names to a symbol of the project: one elsewhere gives no edge.
struct Names<'a> {
    project: &'a Path,
    sources: &'a HashMap<&'a Path, &'a str>,
}

impl<'a> Names<'a> {
    fn new(project: &'a Path, sources: &'a HashMap<&'a Path, &'a str>) -> Self {
        Self { project, sources }
    }

    fn targets(&self, found: Vec<Location>) -> Vec<Target> {
        found.into_iter().filter_map(|l| self.target(l)).collect()
    }

    fn target(&self, found: Location) -> Option<Target> {
        let path: PathBuf = found.path.strip_prefix(self.project).ok()?.to_path_buf();
        let line = self
            .sources
            .get(path.as_path())?
            .split('\n')
            .nth(found.line as usize)?;
        let name =
            line.get(byte_column(line, found.column)..byte_column(line, found.end_column))?;
        Some(Target {
            path: Arc::from(path.as_path()),
            line: found.line + 1,
            name: name.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_go_out_and_back_in_utf16() {
        let line = "\tф := \"𝒳\"; g()";
        let at = line.find("g()").unwrap() as u32;
        let units = utf16_column(line, at);
        assert!(units < at);
        assert_eq!(byte_column(line, units), at as usize);
        assert_eq!(utf16_column(line, 0), 0);
        assert_eq!(byte_column("ab", 9), 2);
    }

    #[test]
    fn a_place_outside_the_project_gives_no_target() {
        let source = "package a\nfunc Fё() {}\n";
        let sources: HashMap<&Path, &str> = [(Path::new("a.go"), source)].into_iter().collect();
        let names = Names::new(Path::new("/p"), &sources);
        let at = |path: &str, line, column, end_column| Location {
            path: PathBuf::from(path),
            line,
            column,
            end_column,
        };
        let found = names.targets(vec![
            at("/p/a.go", 1, 5, 7),
            at("/usr/lib/go/src/builtin/builtin.go", 1, 5, 7),
            at("/p/vendor/x/x.go", 1, 5, 7),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(&*found[0].name, "Fё");
        assert_eq!(found[0].line, 2);
    }
}
