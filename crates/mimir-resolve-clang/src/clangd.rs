//! What the index leaves out, from `clangd` (`docs/CPP.md`, section 4). It says which symbol a site is, by
//! its id, and never where that is: its own answers depend on what it has read.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mimir_lsp::{LspError, Server, ServerSpec};
use mimir_resolve::{LiveDocument, ProjectFile, Target};
use mimir_resolve_scip::{Answered, IndexFiles, Running, decode};
use mimir_types::Language;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::dex::{Dex, Place, Reference};
use crate::scip::{byte_column, utf16_column};
use crate::sites::{Site, ids_at, is_site, runs_at_variable, site_of};
use crate::tools;

const SYMBOL_INFO: &str = "textDocument/symbolInfo";
const DOCUMENT_SYMBOL: &str = "textDocument/documentSymbol";

/// One `clangd`. Dropping it ends the process.
pub struct Clangd {
    server: Server,
}

impl Clangd {
    /// With no index of its own, it writes nothing into the project.
    fn start(project: &Path, database: &Path) -> Result<Self, LspError> {
        let directory = database.parent().unwrap_or(project);
        let spec = ServerSpec {
            program: tools::CLANGD.to_string(),
            args: vec![
                "--background-index=false".to_string(),
                "--enable-config=false".to_string(),
                "--log=error".to_string(),
                "--header-insertion=never".to_string(),
                format!("--compile-commands-dir={}", directory.display()),
            ],
            configuration: json!({}),
            capabilities: json!({
                "textDocument": {
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                },
            }),
            status_method: None,
        };
        let server = Server::start(spec, project)?;
        info!("clangd ready for {}", project.display());
        Ok(Self { server })
    }
}

fn language_id(path: &Path) -> &'static str {
    match Language::from_path(path) {
        Language::C => "c",
        _ => "cpp",
    }
}

/// Give the index the uses of its local symbols, which `clangd-indexer` does not record. Only sites whose
/// name is such a symbol in that file are asked.
pub fn fill_local_symbols(
    index: &mut Dex,
    root: &Path,
    database: &Path,
    files: &[ProjectFile],
) -> Result<(), LspError> {
    let mut local: HashMap<&str, HashSet<&str>> = HashMap::new();
    for symbol in index.symbols.values().filter(|s| s.local) {
        local
            .entry(symbol.at.path.as_str())
            .or_default()
            .insert(symbol.name.as_str());
    }
    let wanted: Vec<(&ProjectFile, &HashSet<&str>)> = files
        .iter()
        .filter_map(|file| Some((file, local.get(file.path.to_str()?)?)))
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let project = root.canonicalize().map_err(|source| LspError::Root {
        path: root.to_path_buf(),
        source,
    })?;
    let mut clangd = Clangd::start(&project, database)?;
    let mut found = Vec::new();
    for (file, names) in wanted {
        let references: Vec<_> = file
            .references
            .iter()
            .filter(|r| is_site(r.kind) && (names.contains(r.name.as_str()) || runs_at_variable(r)))
            .collect();
        if references.is_empty() {
            continue;
        }
        let path = project.join(&file.path);
        clangd
            .server
            .update(&path, language_id(&file.path), &file.source)?;
        let sites: Vec<Site<'_>> = references.iter().map(|r| site_of(r)).collect();
        let ids = ids_at(&mut clangd.server, &path, &file.source, &sites)?;
        clangd.server.close(&path)?;
        let lines: Vec<&str> = file.source.split('\n').collect();
        for (reference, ids) in references.iter().zip(ids) {
            let line = reference.range.start_line - 1;
            let text = lines.get(line as usize).copied().unwrap_or("");
            for symbol in ids.named.into_iter().chain(ids.built) {
                found.push(Reference {
                    symbol,
                    at: Place {
                        path: file.path.to_string_lossy().into_owned(),
                        line,
                        column: utf16_column(text, reference.range.start_col),
                        end_column: utf16_column(text, reference.range.end_col),
                    },
                });
            }
        }
    }
    index.references.extend(
        found
            .into_iter()
            .filter(|r| index.symbols.contains_key(&r.symbol)),
    );
    Ok(())
}

/// What `clangd` says of edited files. Without it they have syntax rules until the next index.
pub fn answer(
    servers: &mut Running<Clangd>,
    project: &Path,
    database: &Path,
    paths: &IndexFiles,
    files: &[ProjectFile],
    asked: &[&ProjectFile],
) -> Result<Answered, String> {
    let project = project.canonicalize().map_err(|e| e.to_string())?;
    let places = kept_definitions(paths)?;
    let result = servers.with(
        &project,
        || Clangd::start(&project, database),
        |clangd| ask_all(&mut clangd.server, &project, &places, files, asked),
    );
    match result {
        Ok(answered) => Ok(answered),
        Err(LspError::Start { program, source }) => {
            warn!("No {program}, so edited files have syntax rules until the next index: {source}");
            Ok(Answered {
                unanswered: asked.iter().map(|f| f.path.clone()).collect(),
                ..Answered::default()
            })
        }
        Err(e) => Err(e.to_string()),
    }
}

/// A place in a file: a path relative to the project, a 0-based line and byte columns.
type Defined = (PathBuf, u32, u32, u32);

/// What the kept index has of a symbol.
#[derive(Default)]
struct Kept {
    written: String,
    places: Vec<Defined>,
}

/// Where the kept index has each symbol defined, by id.
fn kept_definitions(paths: &IndexFiles) -> Result<HashMap<String, Kept>, String> {
    let bytes = fs::read(&paths.index).map_err(|e| e.to_string())?;
    let index = decode(&bytes)?;
    let mut kept: HashMap<String, Kept> = HashMap::new();
    for (path, occurrences) in &index.documents {
        for o in occurrences.iter().filter(|o| o.roles & 1 != 0) {
            if let Some(id) = id_of(&o.symbol) {
                let symbol = kept.entry(id.to_string()).or_default();
                symbol.written.clone_from(&o.symbol);
                symbol
                    .places
                    .push((path.clone(), o.line, o.column, o.end_column));
            }
        }
    }
    Ok(kept)
}

/// The id in a symbol of this index: `A1` in `cxx A1/T#m().` and in `cxx A1 f().`.
fn id_of(symbol: &str) -> Option<&str> {
    symbol.strip_prefix("cxx ")?.split(['/', ' ']).next()
}

fn ask_all(
    server: &mut Server,
    project: &Path,
    kept: &HashMap<String, Kept>,
    files: &[ProjectFile],
    asked: &[&ProjectFile],
) -> Result<Answered, LspError> {
    let sources: HashMap<&Path, &str> = files
        .iter()
        .map(|f| (f.path.as_path(), &*f.source))
        .collect();
    for file in asked {
        server.update(
            &project.join(&file.path),
            language_id(&file.path),
            &file.source,
        )?;
    }
    // Definitions in the files asked about are where they are now.
    let mut edited: HashMap<String, Defined> = HashMap::new();
    let mut own: HashMap<&Path, HashSet<String>> = HashMap::new();
    for file in asked {
        let path = project.join(&file.path);
        let symbols = definitions_in(server, &path, &file.source)?;
        let ids = own.entry(file.path.as_path()).or_default();
        for (id, (line, column, end_column)) in symbols {
            ids.insert(id.clone());
            edited.insert(id, (file.path.clone(), line, column, end_column));
        }
    }
    let target = |place: &Defined| -> Option<Target> {
        let (path, line, column, end_column) = place;
        let text = sources
            .get(path.as_path())?
            .split('\n')
            .nth(*line as usize)?;
        Some(Target {
            path: Arc::from(path.as_path()),
            line: line + 1,
            name: text.get(*column as usize..*end_column as usize)?.into(),
        })
    };
    let mut answered = Answered::default();
    for file in asked {
        let path = project.join(&file.path);
        let sites: Vec<Site<'_>> = file
            .references
            .iter()
            .filter(|r| is_site(r.kind))
            .map(site_of)
            .collect();
        let ids = ids_at(server, &path, &file.source, &sites)?;
        let mut document = LiveDocument::default();
        for id in own.get(file.path.as_path()).into_iter().flatten() {
            if let (Some(symbol), Some(place)) = (kept.get(id), edited.get(id))
                && let Some(found) = target(place)
            {
                document
                    .definitions
                    .insert(symbol.written.clone(), (found.line, found.name));
            }
        }
        for (site, ids) in sites.iter().zip(ids) {
            let targets_of = |ids: &[String]| -> Vec<Target> {
                ids.iter()
                    .filter_map(|id| {
                        let place =
                            edited
                                .get(id)
                                .or_else(|| match kept.get(id)?.places.as_slice() {
                                    [one] => Some(one),
                                    several => several.iter().find(|place| place.0 == file.path),
                                })?;
                        target(place)
                    })
                    .collect()
            };
            let at = (site.line, site.column);
            document.all_sites.insert(at, targets_of(&ids.named));
            document.built_at.insert(at, targets_of(&ids.built));
        }
        answered.documents.insert(file.path.clone(), document);
    }
    Ok(answered)
}

/// The id of each symbol a file defines, and where its name is: a 0-based line and byte columns.
fn definitions_in(
    server: &mut Server,
    path: &Path,
    source: &str,
) -> Result<HashMap<String, (u32, u32, u32)>, LspError> {
    let answer = server.about_file(DOCUMENT_SYMBOL, path)?;
    let mut flat = Vec::new();
    collect(&answer, &mut flat);
    let lines: Vec<&str> = source.split('\n').collect();
    let number = |v: &Value| v.as_u64().map(|n| n as u32);
    let mut named = Vec::new();
    for symbol in &flat {
        let selection = &symbol["selectionRange"];
        let range = &symbol["range"];
        if let (Some(line), Some(column), Some(end_column), Some(first), Some(last)) = (
            number(&selection["start"]["line"]),
            number(&selection["start"]["character"]),
            number(&selection["end"]["character"]),
            number(&range["start"]["line"]),
            number(&range["end"]["line"]),
        ) {
            let body = lines
                .get(first as usize..=(last as usize).min(lines.len().saturating_sub(1)))
                .unwrap_or(&[])
                .join("\n");
            if has_body(&body, symbol) {
                named.push((line, column, end_column));
            }
        }
    }
    let positions: Vec<(u32, u32)> = named.iter().map(|&(l, c, _)| (l, c)).collect();
    let mut found = HashMap::new();
    let answers = server.answers(SYMBOL_INFO, path, &positions)?;
    for (answer, (line, column, end_column)) in answers.iter().zip(named) {
        let text = lines.get(line as usize).copied().unwrap_or("");
        let symbols = answer.iter().filter_map(Value::as_array).flatten();
        for id in symbols.filter_map(|symbol| symbol["id"].as_str()) {
            found.entry(id.to_string()).or_insert((
                line,
                byte_column(text, column),
                byte_column(text, end_column),
            ));
        }
    }
    Ok(found)
}

/// Whether a symbol of `documentSymbol` is defined here, and not only declared: a function or a type by its
/// body. Anything else that has a name here counts, as a field is defined where it is written.
fn has_body(text: &str, symbol: &Value) -> bool {
    // LSP's numbers: `Class`, `Method`, `Constructor`, `Enum`, `Interface`, `Function`, `Struct`.
    let wants_body = matches!(symbol["kind"].as_u64(), Some(5 | 6 | 9 | 10 | 11 | 12 | 23));
    !wants_body
        || text.contains('{')
        || text.contains("= default")
        || text.contains("= delete")
        // A pure virtual has nothing else, and is its own definition in the graph.
        || text.contains("= 0")
}

fn collect<'v>(symbols: &'v Value, into: &mut Vec<&'v Value>) {
    for symbol in symbols.as_array().into_iter().flatten() {
        into.push(symbol);
        collect(&symbol["children"], into);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_in_each_form_of_a_symbol() {
        assert_eq!(id_of("cxx A1 f()."), Some("A1"));
        assert_eq!(id_of("cxx A1/T#m()."), Some("A1"));
        assert_eq!(id_of("cxx A1 T#"), Some("A1"));
        assert_eq!(id_of("local 3"), None);
    }

    #[test]
    fn a_function_only_declared_has_no_body() {
        let function = json!({"kind": 12});
        assert!(!has_body("int f(void);", &function));
        assert!(has_body("int f(void) { return 1; }", &function));
        assert!(has_body("A() = default;", &function));
        assert!(has_body("int x;", &json!({"kind": 8})));
    }

    #[test]
    fn a_type_only_declared_has_no_body() {
        assert!(!has_body("class Token;", &json!({"kind": 5})));
        assert!(has_body("class Token { int x; };", &json!({"kind": 5})));
        assert!(has_body("virtual void f() = 0;", &json!({"kind": 6})));
    }
}
