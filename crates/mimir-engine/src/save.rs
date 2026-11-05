//! The graph on disk: `graph.fb`, docs/GRAPH-STORE.md, and on request `graph.json`, for measuring it
//! from outside (`scripts/bench`). Each file changes all at once.

use mimir_graph::{CodeGraph, GraphFile, ReferencesFile, encode_references};
use mimir_types::{ContentHash, ExtractedReference, FileId, Symbol};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
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

/// Write `bytes` to the file `name` in `dir`, which changes all at once.
fn write_whole(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let unfinished = dir.join(format!("{name}.new"));
    fs::create_dir_all(dir)?;
    fs::write(&unfinished, bytes)?;
    fs::rename(&unfinished, dir.join(name))
}

/// The references of a file, with its id and a hash of its text.
pub type StoredReferences = (FileId, ContentHash, Arc<[ExtractedReference]>);

/// What a finished link leaves to keep. References only when they changed, as encoding them all is
/// the larger part of an edit's cost.
pub struct Stored {
    /// The graph as it is.
    pub graph: Arc<GraphFile>,
    /// The references of every file, with a hash of its text.
    pub references: Option<Vec<StoredReferences>>,
}

impl Stored {
    fn write(self, dir: &Path) {
        if let Err(e) = write_whole(dir, "graph.fb", self.graph.bytes()) {
            warn!("The graph could not be saved: {e}");
        }
        if let Some(references) = self.references {
            let files: Vec<(FileId, ContentHash, &[ExtractedReference])> = references
                .iter()
                .map(|(id, hash, references)| (*id, *hash, &**references))
                .collect();
            if let Err(e) = write_whole(dir, "references.fb", &encode_references(&files)) {
                warn!("The references could not be saved: {e}");
            }
        }
    }
}

/// Keeps what links leave, on a thread of its own, so that no query waits for it. Of several waiting
/// only the last is written, with the last references among them. What a crash loses, the next start
/// sees by the hash of each file. Dropping it waits for what is left to write.
pub struct Writer {
    sender: Option<mpsc::Sender<Stored>>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    /// Start writing to `dir`.
    pub fn new(dir: PathBuf) -> Self {
        let (sender, waiting) = mpsc::channel::<Stored>();
        let thread = std::thread::spawn(move || {
            while let Ok(mut stored) = waiting.recv() {
                while let Ok(newer) = waiting.try_recv() {
                    let references = newer.references.or(stored.references);
                    stored = Stored {
                        graph: newer.graph,
                        references,
                    };
                }
                stored.write(&dir);
            }
        });
        Self {
            sender: Some(sender),
            thread: Some(thread),
        }
    }

    /// Keep this, in time.
    pub fn save(&self, stored: Stored) {
        if let Some(sender) = &self.sender
            && sender.send(stored).is_err()
        {
            warn!("The thread that saves the graph has stopped");
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            warn!("The thread that saves the graph failed");
        }
    }
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
    write_whole(dir, "graph.json", encoded.as_bytes())?;
    Ok(dir.join("graph.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::{
        Language, QualifiedName, Range, ReferenceKind, Span, SymbolId, SymbolKind, Visibility,
    };

    fn graph_of(names: &[&str]) -> CodeGraph {
        let graph = CodeGraph::new();
        graph.add_symbols(
            names
                .iter()
                .enumerate()
                .map(|(at, name)| Symbol {
                    id: SymbolId::new(at as u64 + 1),
                    name: name.to_string(),
                    qualified_name: QualifiedName::simple(*name),
                    kind: SymbolKind::Function,
                    language: Language::Rust,
                    file_id: FileId::new(1),
                    range: Range::new(1, 0, 1, 1),
                    span: Span::new(0, 1),
                    signature: None,
                    visibility: Visibility::Public,
                    doc: None,
                })
                .collect(),
        );
        graph
    }

    #[test]
    fn what_is_left_to_write_is_on_disk_after_the_writer_stops() {
        let dir = tempfile::TempDir::new().unwrap();
        let hash = ContentHash::compute(b"text");
        let reference = ExtractedReference::new(
            "used".to_string(),
            ReferenceKind::Call,
            Range::new(1, 0, 1, 1),
            Span::new(0, 1),
        );
        let writer = Writer::new(dir.path().to_path_buf());
        // The last graph wins, and references given earlier are not lost to one with none.
        writer.save(Stored {
            graph: graph_of(&["a"]).snapshot(),
            references: Some(vec![(FileId::new(1), hash, vec![reference].into())]),
        });
        for names in [&["a", "b"][..], &["a", "b", "c"][..]] {
            writer.save(Stored {
                graph: graph_of(names).snapshot(),
                references: None,
            });
        }
        drop(writer);

        let graph = load(dir.path()).unwrap();
        assert_eq!(graph.symbol_count(), 3);
        let references = load_references(dir.path()).unwrap();
        let stored = references.references_of(FileId::new(1), hash).unwrap();
        assert_eq!(stored[0].name, "used");
    }
}
