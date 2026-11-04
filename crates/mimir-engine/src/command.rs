//! Commands for the graph actor.

use crate::error::GraphError;
use mimir_protocol::{
    Edit, EditReport, FileText, FilesResponse, FindOptions, FindResponse, GraphEvent,
    GraphSnapshot, GrepOptions, GrepResponse, ImpactResponse, SearchOptions, SearchResponse,
    StatsResponse, SymbolSource, SyncResult, TraceOptions, TraceResponse,
};
use mimir_types::{FileId, Language, SymbolId};
use std::path::PathBuf;
use tokio::sync::{broadcast, oneshot};

/// Commands that can be sent to the graph actor.
#[derive(Debug)]
pub enum GraphCommand {
    /// Add or update a file in the graph.
    AddFile {
        path: PathBuf,
        content: String,
        language: Language,
        reply: oneshot::Sender<Result<FileId, GraphError>>,
    },

    /// Remove a file from the graph.
    RemoveFile {
        file_id: FileId,
        reply: oneshot::Sender<Result<bool, GraphError>>,
    },

    /// Forget a file, or everything under a directory, that is gone from the disk.
    RemovePath { path: PathBuf },

    /// The whole graph, for a view.
    Snapshot {
        reply: oneshot::Sender<GraphSnapshot>,
    },

    /// Start hearing what the engine announces.
    Watch {
        reply: oneshot::Sender<broadcast::Receiver<GraphEvent>>,
    },

    /// Find symbols by name.
    FindSymbol {
        name: String,
        options: FindOptions,
        reply: oneshot::Sender<FindResponse>,
    },

    /// Trace a symbol's callers and callees.
    Trace {
        symbol_id: SymbolId,
        options: TraceOptions,
        reply: oneshot::Sender<TraceResponse>,
    },

    /// What changed since a commit, and what depends on it.
    Impact {
        base: Option<String>,
        depth: Option<u32>,
        reply: oneshot::Sender<Result<ImpactResponse, GraphError>>,
    },

    /// Find a symbol by name and trace it.
    TraceByName {
        name: String,
        options: TraceOptions,
        reply: oneshot::Sender<Result<TraceResponse, GraphError>>,
    },

    /// The source of the symbol a name most likely means, with context.
    Read {
        name: String,
        file: Option<PathBuf>,
        before: u32,
        after: u32,
        reply: oneshot::Sender<Result<SymbolSource, GraphError>>,
    },

    /// The first `lines` lines of a symbol, or all of them.
    Source {
        symbol_id: SymbolId,
        lines: Option<u32>,
        reply: oneshot::Sender<Result<SymbolSource, GraphError>>,
    },

    /// Apply edits together, or with `preview` only show them.
    Edit {
        edits: Vec<Edit>,
        preview: bool,
        backup: bool,
        reply: oneshot::Sender<Result<EditReport, GraphError>>,
    },

    /// Get graph statistics.
    Stats {
        reply: oneshot::Sender<StatsResponse>,
    },

    /// Scan for changes; a query that follows may find edges still provisional.
    Sync {
        reply: oneshot::Sender<Result<SyncResult, GraphError>>,
    },

    /// Scan for changes, wait until no edge is provisional, and save the graph.
    SyncComplete {
        full: bool,
        reply: oneshot::Sender<Result<SyncResult, GraphError>>,
    },

    /// Search for symbols by pattern.
    Search {
        options: SearchOptions,
        reply: oneshot::Sender<Result<SearchResponse, GraphError>>,
    },

    /// Search the text of the project's files.
    Grep {
        options: GrepOptions,
        reply: oneshot::Sender<Result<GrepResponse, GraphError>>,
    },

    /// The project's files, only those matching a glob if given.
    Files {
        glob: Option<String>,
        limit: Option<u32>,
        reply: oneshot::Sender<Result<FilesResponse, GraphError>>,
    },

    /// Lines of a file of the project.
    ReadFile {
        path: PathBuf,
        first: Option<u32>,
        last: Option<u32>,
        reply: oneshot::Sender<Result<FileText, GraphError>>,
    },

    /// Write a whole file of the project.
    WriteFile {
        path: PathBuf,
        content: String,
        overwrite: bool,
        reply: oneshot::Sender<Result<EditReport, GraphError>>,
    },

    /// Shutdown the actor.
    Shutdown,
}
