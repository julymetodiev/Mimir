//! Client handle for interacting with the graph actor.

use crate::error::GraphError;
use latt_protocol::{
    Edit, EditReport, FileText, FilesResponse, FindOptions, FindResponse, GraphEvent,
    GraphSnapshot, GrepOptions, GrepResponse, ImpactResponse, SearchOptions, SearchResponse,
    StatsResponse, SymbolSource, SyncResult, TraceOptions, TraceResponse,
};
use latt_types::{FileId, Language, SymbolId};
use std::path::PathBuf;
use tokio::sync::{broadcast, oneshot};
use tracing::{info, warn};

use crate::channel::CommandSender;
use crate::command::GraphCommand;

/// A handle to communicate with the graph actor.
///
/// This is a cheap-to-clone client that can be shared across tasks.
#[derive(Clone)]
pub struct GraphHandle {
    commands: CommandSender,
}

impl GraphHandle {
    /// Create a new handle that sends commands to the actor.
    pub fn new(commands: CommandSender) -> Self {
        Self { commands }
    }

    /// Add a file to the graph.
    pub async fn add_file(
        &self,
        path: PathBuf,
        content: String,
        language: Language,
    ) -> Result<FileId, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::AddFile {
                path,
                content,
                language,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;

        reply_rx.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Remove a file from the graph.
    pub async fn remove_file(&self, file_id: FileId) -> Result<bool, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::RemoveFile {
                file_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;

        reply_rx.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Find symbols by name.
    pub async fn find_symbol(&self, name: &str) -> Result<FindResponse, GraphError> {
        self.find_symbol_with_options(name, FindOptions::default())
            .await
    }

    /// Find symbols with options.
    pub async fn find_symbol_with_options(
        &self,
        name: &str,
        options: FindOptions,
    ) -> Result<FindResponse, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::FindSymbol {
                name: name.to_string(),
                options,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;

        reply_rx.await.map_err(|_| GraphError::EngineStopped)
    }

    /// What changed since the last commit, or since `base` split off, and what depends on it.
    pub async fn impact(
        &self,
        base: Option<String>,
        depth: Option<u32>,
    ) -> Result<ImpactResponse, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Impact { base, depth, reply })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Trace a symbol's relationships.
    pub async fn trace(&self, symbol_id: SymbolId) -> Result<TraceResponse, GraphError> {
        self.trace_with_options(symbol_id, TraceOptions::default())
            .await
    }

    /// Trace with options.
    pub async fn trace_with_options(
        &self,
        symbol_id: SymbolId,
        options: TraceOptions,
    ) -> Result<TraceResponse, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::Trace {
                symbol_id,
                options,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;

        reply_rx.await.map_err(|_| GraphError::EngineStopped)
    }

    /// Get graph statistics.
    pub async fn stats(&self) -> Result<StatsResponse, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::Stats { reply: reply_tx })
            .await
            .map_err(|_| GraphError::EngineStopped)?;

        reply_rx.await.map_err(|_| GraphError::EngineStopped)
    }

    /// Trigger a sync operation.
    pub async fn sync(&self) -> Result<SyncResult, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::Sync { reply: reply_tx })
            .await
            .map_err(|_| GraphError::EngineStopped)?;

        reply_rx.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Find a symbol by name and trace it.
    pub async fn trace_by_name(
        &self,
        name: String,
        options: TraceOptions,
    ) -> Result<TraceResponse, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::TraceByName {
                name,
                options,
                reply,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// The source of the symbol a name most likely means, with lines of context around it.
    pub async fn read(
        &self,
        name: String,
        file: Option<PathBuf>,
        before: u32,
        after: u32,
    ) -> Result<SymbolSource, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Read {
                name,
                file,
                before,
                after,
                reply,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// The first `lines` lines of a symbol, or all of them.
    pub async fn source(
        &self,
        symbol_id: SymbolId,
        lines: Option<u32>,
    ) -> Result<SymbolSource, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Source {
                symbol_id,
                lines,
                reply,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Apply edits together, or with `preview` only show them.
    pub async fn edit(
        &self,
        edits: Vec<Edit>,
        preview: bool,
        backup: bool,
    ) -> Result<EditReport, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Edit {
                edits,
                preview,
                backup,
                reply,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Scan for changes and wait until no edge is provisional. `json` writes the graph as JSON too.
    pub async fn sync_complete(&self, full: bool, json: bool) -> Result<SyncResult, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::SyncComplete { full, json, reply })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Queue a scan without waiting for it. Commands sent after this one wait for it in the actor.
    pub async fn queue_first_scan(&self) {
        let (reply, answer) = oneshot::channel();
        if self
            .commands
            .send(GraphCommand::Sync { reply })
            .await
            .is_err()
        {
            warn!("The graph actor stopped before the first scan");
            return;
        }
        tokio::spawn(async move {
            match answer.await {
                Ok(Ok(result)) => info!(
                    "First scan: {} files, {} symbols",
                    result.files_added, result.symbols_added
                ),
                Ok(Err(e)) => warn!("First scan failed: {e}"),
                Err(_) => warn!("The graph actor stopped during the first scan"),
            }
        });
    }

    /// Search the text of the project's files.
    pub async fn grep(&self, options: GrepOptions) -> Result<GrepResponse, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Grep { options, reply })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// The project's files, only those matching a glob if given.
    pub async fn files(
        &self,
        glob: Option<String>,
        limit: Option<u32>,
    ) -> Result<FilesResponse, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Files { glob, limit, reply })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Lines of a file of the project.
    pub async fn read_file(
        &self,
        path: PathBuf,
        first: Option<u32>,
        last: Option<u32>,
    ) -> Result<FileText, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::ReadFile {
                path,
                first,
                last,
                reply,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Write a whole file of the project.
    pub async fn write_file(
        &self,
        path: PathBuf,
        content: String,
        overwrite: bool,
    ) -> Result<EditReport, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::WriteFile {
                path,
                content,
                overwrite,
                reply,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// The whole graph, for a view.
    pub async fn snapshot(&self) -> Result<GraphSnapshot, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Snapshot { reply })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)
    }

    /// Start hearing what the engine announces.
    pub async fn watch(&self) -> Result<broadcast::Receiver<GraphEvent>, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::Watch { reply })
            .await
            .map_err(|_| GraphError::EngineStopped)?;
        answer.await.map_err(|_| GraphError::EngineStopped)
    }

    /// Search for symbols by pattern.
    pub async fn search(&self, options: SearchOptions) -> Result<SearchResponse, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::Search {
                options,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GraphError::EngineStopped)?;

        reply_rx.await.map_err(|_| GraphError::EngineStopped)?
    }

    /// Request shutdown.
    pub async fn shutdown(&self) -> Result<(), GraphError> {
        self.commands
            .send(GraphCommand::Shutdown)
            .await
            .map_err(|_| GraphError::EngineStopped)
    }
}

#[cfg(test)]
mod tests;
