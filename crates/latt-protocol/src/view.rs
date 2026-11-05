//! What a view of the graph is told (docs/ARCHITECTURE.md section 7).

use latt_types::{SymbolId, SymbolKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The whole graph at one moment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    /// Rises each time the edges are resolved anew.
    pub generation: u64,
    /// Semantic resolution is still running, so edges may be missing.
    pub provisional: bool,
    /// The project's files, which symbols name by their place in this list.
    pub files: Vec<PathBuf>,
    pub symbols: Vec<SnapshotSymbol>,
    pub edges: Vec<SnapshotEdge>,
}

/// A symbol in a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSymbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    /// Its place in the snapshot's files.
    pub file: u32,
    pub line: u32,
}

/// An edge between two symbols, by their places in the snapshot's symbols.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SnapshotEdge {
    pub from: u32,
    pub to: u32,
    pub kind: SnapshotEdgeKind,
}

/// How sure an edge is, and what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotEdgeKind {
    /// An exact call.
    Call,
    /// May run or not: a trait's implementation, a function named as a value.
    Possible,
    /// Uses a type, field or constant.
    Use,
    /// An import.
    Import,
}

/// What the engine announces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphEvent {
    /// The graph is no longer what the last snapshot showed.
    GraphChanged { generation: u64, provisional: bool },
    /// A query or an edit touched these symbols. Says nothing of what it asked or wrote.
    Activity {
        symbols: Vec<SymbolId>,
        kind: ActivityKind,
    },
    /// Events went by while this subscriber was behind, so what it knows may be old.
    Lagged,
}

/// What touched a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityKind {
    Read,
    Edit,
}
