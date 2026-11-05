//! Reading the graph from a file's bytes with no copy into other structures: docs/GRAPH-STORE.md.
//! Nothing here touches the disk.

use crate::encode::{encode_edges, encode_symbols};
use crate::format::{
    BUILT_BY, VERSION, confidence_of, edge_of, kind_of, language_of, visibility_of,
};
use crate::relations::EdgeData;
use mimir_graph_fb as fb;
use mimir_types::{
    ContentHash, FileId, Parameter, QualifiedName, Range, Signature, Span, Symbol, SymbolId,
};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

/// A file of the project, as the graph file records it.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// The file's id.
    pub id: FileId,
    /// Its path relative to the project.
    pub path: PathBuf,
    /// A hash of its text.
    pub hash: ContentHash,
    /// Its size in bytes, when it was read.
    pub size: u64,
    /// When it last changed, in milliseconds, when it was read; 0 if not known.
    pub modified: i64,
}

/// What was wrong with the bytes of a graph file.
#[derive(Debug)]
pub enum FileError {
    /// An offset points outside the bytes.
    Invalid(fb::InvalidFlatbuffer),
    /// Written by another version of the format.
    Version(u32),
    /// The offsets are right, but what they lead to is not a graph.
    Broken(&'static str),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "not a graph file: {reason}"),
            Self::Version(found) => {
                write!(
                    f,
                    "graph file of version {found} or of another Mimir, not {VERSION} of {BUILT_BY}"
                )
            }
            Self::Broken(what) => write!(f, "broken graph file: {what}"),
        }
    }
}

impl std::error::Error for FileError {}

/// A graph in two buffers, as read from two files: symbols and edges, apart, so that a link does not
/// write every symbol again. Every position in them has been checked, so a query cannot fail.
pub struct GraphFile {
    symbols: Arc<fb::GraphBuffer>,
    edges: fb::EdgesBuffer,
}

impl GraphFile {
    /// Check both files as a whole, and take them. Nothing is copied. Edges that are not of these
    /// symbols are refused.
    pub fn open(symbols: Vec<u8>, edges: Vec<u8>) -> Result<Self, FileError> {
        let symbols = Arc::new(open_symbols(symbols)?);
        let edges = open_edges(&symbols, edges)?;
        Ok(Self { symbols, edges })
    }

    /// A graph with these symbols and no edges: what is left when edges cannot be read.
    pub fn open_without_edges(symbols: Vec<u8>) -> Result<Self, FileError> {
        let symbols = Arc::new(open_symbols(symbols)?);
        let edges = open_edges(&symbols, encode_edges(symbols.graph(), &[]))?;
        Ok(Self { symbols, edges })
    }

    /// A graph of these symbols, edges and files.
    pub fn build(
        symbols: Vec<Symbol>,
        edges: &[(SymbolId, SymbolId, EdgeData)],
        files: &[FileEntry],
    ) -> Result<Self, FileError> {
        let symbols = Arc::new(open_symbols(encode_symbols(symbols, files))?);
        let edges = open_edges(&symbols, encode_edges(symbols.graph(), edges))?;
        Ok(Self { symbols, edges })
    }

    /// The same symbols with other edges, which writes no symbol.
    pub fn with_edges(&self, edges: &[(SymbolId, SymbolId, EdgeData)]) -> Result<Self, FileError> {
        let bytes = encode_edges(self.symbols.graph(), edges);
        Ok(Self {
            symbols: self.symbols.clone(),
            edges: open_edges(&self.symbols, bytes)?,
        })
    }

    /// The bytes of the symbols, as a file holds them.
    pub fn symbol_bytes(&self) -> &[u8] {
        self.symbols.bytes()
    }

    /// The bytes of the edges, as a file holds them.
    pub fn edge_bytes(&self) -> &[u8] {
        self.edges.bytes()
    }

    /// Whether another graph holds the very same symbols, so that they need no writing.
    pub fn has_symbols_of(&self, other: &GraphFile) -> bool {
        Arc::ptr_eq(&self.symbols, &other.symbols)
    }

    /// How many symbols it holds.
    pub fn symbol_count(&self) -> usize {
        self.symbols
            .graph()
            .symbols()
            .map_or(0, |symbols| symbols.len())
    }

    /// How many edges it holds.
    pub fn edge_count(&self) -> usize {
        self.edges.edges().edges().map_or(0, |edges| edges.len())
    }

    /// Every symbol of every file, in the order each file has them.
    pub fn all_symbols(&self) -> Vec<Symbol> {
        let files = self.symbols.graph().files();
        files
            .iter()
            .flat_map(|files| files.iter())
            .filter_map(|file| file.symbols())
            .flat_map(|at| self.symbols_at(at))
            .collect()
    }

    /// Every edge.
    pub fn all_edges(&self) -> Vec<(SymbolId, SymbolId, EdgeData)> {
        let (Some(symbols), Some(edges)) =
            (self.symbols.graph().symbols(), self.edges.edges().edges())
        else {
            return Vec::new();
        };
        let id_at = |at: u32| SymbolId::new(symbols.get(at as usize).id());
        edges
            .iter()
            .filter_map(|edge| Some((id_at(edge.from()), id_at(edge.to()), read_edge(edge)?)))
            .collect()
    }

    /// Every file of the project.
    pub fn files(&self) -> Vec<FileEntry> {
        let files = self.symbols.graph().files();
        files
            .iter()
            .flat_map(|files| files.iter())
            .filter_map(|file| {
                Some(FileEntry {
                    id: FileId::new(file.id()),
                    path: PathBuf::from(file.path()?),
                    hash: ContentHash::from_bytes(file.hash()?.bytes().try_into().ok()?),
                    size: file.size(),
                    modified: file.modified(),
                })
            })
            .collect()
    }

    /// The symbol with this id.
    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        let symbols = self.symbols.graph().symbols()?;
        read_symbol(symbols.get(position_of(symbols, id)?))
    }

    /// The symbols with this name.
    pub fn find_symbol(&self, name: &str) -> Vec<Symbol> {
        self.named(self.symbols.graph().names(), name)
    }

    /// The symbols with this qualified name.
    pub fn find_by_qualified_name(&self, name: &str) -> Vec<Symbol> {
        self.named(self.symbols.graph().qualified(), name)
    }

    fn named(&self, names: Option<fb::Tables<'_, fb::Name<'_>>>, name: &str) -> Vec<Symbol> {
        names
            .and_then(|names| {
                names.lookup_by_key(name, |entry, name| entry.key_compare_with_value(name))
            })
            .and_then(|entry| entry.symbols())
            .map(|at| self.symbols_at(at))
            .unwrap_or_default()
    }

    /// The symbols of a file.
    pub fn symbols_in_file(&self, file: FileId) -> Vec<Symbol> {
        self.positions_in_file(file)
            .map(|at| self.symbols_at(at))
            .unwrap_or_default()
    }

    /// How many symbols a file has.
    pub fn count_in_file(&self, file: FileId) -> usize {
        self.positions_in_file(file).map_or(0, |at| at.len())
    }

    fn positions_in_file(&self, file: FileId) -> Option<fb::Vector<'_, u32>> {
        self.symbols
            .graph()
            .files()?
            .lookup_by_key(file.value(), |entry, id| entry.key_compare_with_value(*id))?
            .symbols()
    }

    fn symbols_at(&self, at: fb::Vector<'_, u32>) -> Vec<Symbol> {
        let symbols = self.symbols.graph().symbols();
        at.iter()
            .filter_map(|position| read_symbol(symbols?.get(position as usize)))
            .collect()
    }

    /// What a symbol's edges lead to.
    pub fn outgoing(&self, id: SymbolId) -> Vec<(SymbolId, EdgeData)> {
        let stored = self.edges.edges();
        let (Some(symbols), Some(edges), Some(offsets)) = (
            self.symbols.graph().symbols(),
            stored.edges(),
            stored.out_offsets(),
        ) else {
            return Vec::new();
        };
        let Some(position) = position_of(symbols, id) else {
            return Vec::new();
        };
        (offsets.get(position)..offsets.get(position + 1))
            .filter_map(|at| {
                let edge = edges.get(at as usize);
                let target = SymbolId::new(symbols.get(edge.to() as usize).id());
                Some((target, read_edge(edge)?))
            })
            .collect()
    }

    /// What leads to a symbol by an edge.
    pub fn incoming(&self, id: SymbolId) -> Vec<(SymbolId, EdgeData)> {
        let stored = self.edges.edges();
        let (Some(symbols), Some(edges), Some(incoming), Some(offsets)) = (
            self.symbols.graph().symbols(),
            stored.edges(),
            stored.incoming(),
            stored.in_offsets(),
        ) else {
            return Vec::new();
        };
        let Some(position) = position_of(symbols, id) else {
            return Vec::new();
        };
        (offsets.get(position)..offsets.get(position + 1))
            .filter_map(|at| {
                let edge = edges.get(incoming.get(at as usize) as usize);
                let source = SymbolId::new(symbols.get(edge.from() as usize).id());
                Some((source, read_edge(edge)?))
            })
            .collect()
    }
}

/// Where a symbol is, by a binary search of ids.
pub fn position_of(symbols: fb::Tables<'_, fb::Symbol<'_>>, id: SymbolId) -> Option<usize> {
    let (mut low, mut high) = (0, symbols.len());
    while low < high {
        let middle = low + (high - low) / 2;
        match symbols.get(middle).id().cmp(&id.value()) {
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
            std::cmp::Ordering::Equal => return Some(middle),
        }
    }
    None
}

fn read_edge(edge: &fb::Edge) -> Option<EdgeData> {
    Some(EdgeData::new(
        edge_of(edge.kind())?,
        Span::new(edge.start(), edge.end()),
        confidence_of(edge.confidence())?,
    ))
}

fn read_symbol(symbol: fb::Symbol<'_>) -> Option<Symbol> {
    let range = symbol.range()?;
    let span = symbol.span()?;
    let segments = symbol.qualified()?.iter().map(str::to_string).collect();
    Some(Symbol {
        id: SymbolId::new(symbol.id()),
        name: symbol.name()?.to_string(),
        qualified_name: QualifiedName::new(segments),
        kind: kind_of(symbol.kind())?,
        language: language_of(symbol.language())?,
        file_id: FileId::new(symbol.file()),
        range: Range {
            start_line: range.start_line(),
            start_col: range.start_col(),
            end_line: range.end_line(),
            end_col: range.end_col(),
        },
        span: Span {
            start: span.start(),
            end: span.end(),
        },
        signature: symbol.signature().and_then(read_signature),
        visibility: visibility_of(symbol.visibility())?,
        doc: symbol.doc().map(str::to_string),
    })
}

fn read_signature(signature: fb::Signature<'_>) -> Option<Signature> {
    Some(Signature {
        parameters: signature
            .parameters()?
            .iter()
            .map(|parameter| {
                Some(Parameter {
                    name: parameter.name()?.to_string(),
                    type_annotation: parameter.type_annotation().map(str::to_string),
                    default_value: parameter.default_value().map(str::to_string),
                })
            })
            .collect::<Option<_>>()?,
        return_type: signature.return_type().map(str::to_string),
        type_parameters: signature
            .type_parameters()?
            .iter()
            .map(str::to_string)
            .collect(),
        raw: signature.raw()?.to_string(),
    })
}

/// Whether a symbol has all it needs to be read, without reading it into anything.
fn symbol_is_whole(symbol: fb::Symbol<'_>) -> bool {
    symbol.name().is_some()
        && symbol.range().is_some()
        && symbol.span().is_some()
        && symbol
            .qualified()
            .is_some_and(|segments| !segments.is_empty())
        && kind_of(symbol.kind()).is_some()
        && language_of(symbol.language()).is_some()
        && visibility_of(symbol.visibility()).is_some()
        && symbol.signature().is_none_or(|signature| {
            signature.raw().is_some()
                && signature.type_parameters().is_some()
                && signature
                    .parameters()
                    .is_some_and(|all| all.iter().all(|parameter| parameter.name().is_some()))
        })
}

/// Check what the offsets lead to: a position out of range would otherwise stop a query. Nothing is
/// copied, so this costs a look at each symbol and edge, once.
fn check_symbols(graph: fb::Graph<'_>) -> Result<(), FileError> {
    if graph.version() != VERSION || graph.built_by() != Some(BUILT_BY) {
        return Err(FileError::Version(graph.version()));
    }
    let broken = FileError::Broken;
    let symbols = graph.symbols().ok_or(broken("no symbols"))?;
    let names = graph.names().ok_or(broken("no names"))?;
    let qualified = graph.qualified().ok_or(broken("no qualified"))?;
    let files = graph.files().ok_or(broken("no files"))?;

    let count = symbols.len();
    let mut last = None;
    for symbol in symbols.iter() {
        if last.is_some_and(|last| last >= symbol.id()) {
            return Err(broken("symbols not sorted by id"));
        }
        last = Some(symbol.id());
        if !symbol_is_whole(symbol) {
            return Err(broken("a symbol lacks a part"));
        }
    }
    let in_range = |at: Option<fb::Vector<'_, u32>>| {
        at.is_some_and(|at| at.iter().all(|position| (position as usize) < count))
    };
    let names_right = names
        .iter()
        .chain(qualified.iter())
        .all(|name| in_range(name.symbols()));
    if !names_right {
        return Err(broken("a name out of range"));
    }
    let files_right = files.iter().all(|file| {
        in_range(file.symbols())
            && file.path().is_some()
            && file.hash().is_some_and(|hash| hash.len() == 32)
    });
    if !files_right {
        return Err(broken("a file out of range"));
    }
    Ok(())
}

/// Check that edges are of these symbols, and what their offsets lead to.
fn check_edges(graph: fb::Graph<'_>, stored: fb::Edges<'_>) -> Result<(), FileError> {
    if stored.version() != VERSION || stored.built_by() != Some(BUILT_BY) {
        return Err(FileError::Version(stored.version()));
    }
    let broken = FileError::Broken;
    if stored.stamp() != graph.stamp() {
        return Err(broken("edges of other symbols"));
    }
    let count = graph.symbols().map_or(0, |symbols| symbols.len());
    let edges = stored.edges().ok_or(broken("no edges"))?;
    let out_offsets = stored.out_offsets().ok_or(broken("no out_offsets"))?;
    let incoming = stored.incoming().ok_or(broken("no incoming"))?;
    let in_offsets = stored.in_offsets().ok_or(broken("no in_offsets"))?;
    for edge in edges.iter() {
        if edge.from() as usize >= count
            || edge.to() as usize >= count
            || edge.start() > edge.end()
            || edge_of(edge.kind()).is_none()
            || confidence_of(edge.confidence()).is_none()
        {
            return Err(broken("an edge out of range"));
        }
    }
    let offsets_right = |offsets: fb::Vector<'_, u32>| {
        offsets.len() == count + 1
            && offsets.get(0) == 0
            && offsets.get(count) as usize == edges.len()
            && (0..count).all(|at| offsets.get(at) <= offsets.get(at + 1))
    };
    if !offsets_right(out_offsets) || !offsets_right(in_offsets) || incoming.len() != edges.len() {
        return Err(broken("offsets out of range"));
    }
    if incoming.iter().any(|at| at as usize >= edges.len()) {
        return Err(broken("incoming out of range"));
    }
    Ok(())
}

fn open_symbols(bytes: Vec<u8>) -> Result<fb::GraphBuffer, FileError> {
    let buffer = fb::GraphBuffer::verified(bytes).map_err(FileError::Invalid)?;
    check_symbols(buffer.graph())?;
    Ok(buffer)
}

fn open_edges(symbols: &fb::GraphBuffer, bytes: Vec<u8>) -> Result<fb::EdgesBuffer, FileError> {
    let buffer = fb::EdgesBuffer::verified(bytes).map_err(FileError::Invalid)?;
    check_edges(symbols.graph(), buffer.edges())?;
    Ok(buffer)
}

#[cfg(test)]
mod tests;
