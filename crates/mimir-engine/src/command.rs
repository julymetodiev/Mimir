//! Commands for the graph actor.

use crate::error::GraphError;
use mimir_protocol::{
    Edit, EditReport, FindOptions, FindResponse, SearchOptions, SearchResponse, StatsResponse,
    SymbolSource, SyncResult, TraceOptions, TraceResponse,
};
use mimir_types::{FileId, Language, SymbolId};
use std::path::PathBuf;
use tokio::sync::oneshot;

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

    /// Find a symbol by name and trace it.
    TraceByName {
        name: String,
        options: TraceOptions,
        reply: oneshot::Sender<Result<TraceResponse, GraphError>>,
    },

    /// The source of the symbol a name most likely means, with context.
    Read {
        name: String,
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

    /// Shutdown the actor.
    Shutdown,
}
