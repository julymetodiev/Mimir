//! The graph on disk: `graph.fb`, docs/GRAPH-STORE.md, and on request `graph.json`, for measuring it
//! from outside (`scripts/bench`). Each file changes all at once.

use mimir_graph::{CodeGraph, GraphFile, ReferencesFile};
use mimir_types::{FileId, Symbol};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::warn;

#[derive(Serialize)]
struct SavedGraph {
    symbols: Vec<Symbol>,
    /// (from, to, kind, span start, span end, confidence)
    edges: Vec<(u64, u64, String, u32, u32, String)>,
    file_ids: HashMap<String, u64>,
}

/// The graph `graph.fb` in `dir` holds. No file is no graph; a file of another version or a broken
/// one is the same, and the next link writes it again.
pub fn load(dir: &Path) -> Option<CodeGraph> {
    let bytes = fs::read(dir.join("graph.fb")).ok()?;
    match GraphFile::open(bytes) {
        Ok(file) => Some(CodeGraph::from_file(file)),
        Err(reason) => {
            warn!("Building the graph again: {reason}");
            None
        }
    }
}

/// The references `references.fb` in `dir` holds. Without a usable file, files are parsed.
pub fn load_references(dir: &Path) -> Option<ReferencesFile> {
    let bytes = fs::read(dir.join("references.fb")).ok()?;
    match ReferencesFile::open(bytes) {
        Ok(file) => Some(file),
        Err(reason) => {
            warn!("Parsing every file again: {reason}");
            None
        }
    }
}

/// Write `references.fb` in `dir`.
pub fn save_references(bytes: &[u8], dir: &Path) -> io::Result<()> {
    let unfinished = dir.join("references.fb.new");
    fs::create_dir_all(dir)?;
    fs::write(&unfinished, bytes)?;
    fs::rename(&unfinished, dir.join("references.fb"))
}

/// Write `graph.fb` in `dir` and return its path.
pub fn save_file(graph: &CodeGraph, dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join("graph.fb");
    let unfinished = dir.join("graph.fb.new");
    fs::create_dir_all(dir)?;
    fs::write(&unfinished, graph.bytes())?;
    fs::rename(&unfinished, &path)?;
    Ok(path)
}

/// Write the graph to `graph.json` in `dir` and return its path. The file changes all at once.
pub fn save(
    graph: &CodeGraph,
    file_paths: &HashMap<FileId, PathBuf>,
    dir: &Path,
) -> io::Result<PathBuf> {
    let symbols: Vec<Symbol> = file_paths
        .keys()
        .flat_map(|file_id| graph.symbols_in_file(*file_id))
        .map(|symbol| (*symbol).clone())
        .collect();
    let mut edges = Vec::new();
    for symbol in &symbols {
        for (target, edge) in graph.get_outgoing(symbol.id) {
            edges.push((
                symbol.id.as_u64(),
                target.id.as_u64(),
                edge.kind.as_str().to_string(),
                edge.span.start,
                edge.span.end,
                edge.confidence.to_string(),
            ));
        }
    }
    let file_ids = file_paths
        .iter()
        .map(|(id, path)| (path.to_string_lossy().into_owned(), id.as_u64()))
        .collect();
    let saved = SavedGraph {
        symbols,
        edges,
        file_ids,
    };

    let encoded = serde_json::to_string_pretty(&saved)?;
    let path = dir.join("graph.json");
    let unfinished = dir.join("graph.json.new");
    fs::create_dir_all(dir)?;
    fs::write(&unfinished, encoded)?;
    fs::rename(&unfinished, &path)?;
    Ok(path)
}
