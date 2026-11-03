//! The graph as a file, for measuring it from outside (`scripts/bench`). Nothing reads it back:
//! the daemon builds the graph from the sources when it starts.

use mimir_graph::CodeGraph;
use mimir_types::{FileId, Symbol};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct SavedGraph {
    symbols: Vec<Symbol>,
    /// (from, to, kind, span start, span end, confidence)
    edges: Vec<(u64, u64, String, u32, u32, String)>,
    file_ids: HashMap<String, u64>,
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
    fs::write(&unfinished, encoded)?;
    fs::rename(&unfinished, &path)?;
    Ok(path)
}
