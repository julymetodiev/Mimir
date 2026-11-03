//! Commands for the graph actor.

use mimir_error::GraphError;
use mimir_protocol::{
    FindOptions, FindResponse, SearchOptions, SearchResponse, StatsResponse, SyncResult,
    TraceOptions, TraceResponse,
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

    /// Get graph statistics.
    Stats {
        reply: oneshot::Sender<StatsResponse>,
    },

    /// Trigger a sync operation.
    Sync {
        reply: oneshot::Sender<Result<SyncResult, GraphError>>,
    },

    /// Search for symbols by pattern.
    Search {
        options: SearchOptions,
        reply: oneshot::Sender<SearchResponse>,
    },

    /// Shutdown the actor.
    Shutdown,
}
