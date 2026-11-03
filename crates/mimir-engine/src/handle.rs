//! Client handle for interacting with the graph actor.

use crate::error::GraphError;
use mimir_protocol::{
    Edit, EditReport, FindOptions, FindResponse, GrepOptions, GrepResponse, SearchOptions,
    SearchResponse, StatsResponse, SymbolSource, SyncResult, TraceOptions, TraceResponse,
};
use mimir_types::{FileId, Language, SymbolId};
use std::path::PathBuf;
use tokio::sync::oneshot;
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

    /// Scan for changes, wait until no edge is provisional, and save the graph.
    pub async fn sync_complete(&self, full: bool) -> Result<SyncResult, GraphError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(GraphCommand::SyncComplete { full, reply })
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
mod tests {
    use super::*;
    use crate::actor::GraphActor;
    use crate::channel::command_channel;
    use mimir_parse_core::AdapterRegistry;
    use mimir_parse_rust::RustAdapter;
    use std::sync::Arc;

    fn create_test_registry() -> Arc<AdapterRegistry> {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(RustAdapter::new()));
        Arc::new(registry)
    }

    #[tokio::test]
    async fn test_handle_add_and_find() {
        let (cmd_tx, cmd_rx) = command_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve::NoSemanticSource),
        );

        let actor_handle = tokio::spawn(async move { actor.run(cmd_rx).await });

        let handle = GraphHandle::new(cmd_tx);

        // Add file
        let file_id = handle
            .add_file(
                PathBuf::from("test.rs"),
                "fn my_func() {}".to_string(),
                Language::Rust,
            )
            .await
            .unwrap();

        assert_eq!(file_id, FileId::new(1));

        // Find symbol
        let response = handle.find_symbol("my_func").await.unwrap();
        assert!(response.has_results());

        // Shutdown
        handle.shutdown().await.unwrap();
        actor_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_handle_stats() {
        let (cmd_tx, cmd_rx) = command_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve::NoSemanticSource),
        );

        let actor_handle = tokio::spawn(async move { actor.run(cmd_rx).await });

        let handle = GraphHandle::new(cmd_tx);

        // Initial stats - files_count is not tracked in current impl
        let stats = handle.stats().await.unwrap();
        assert_eq!(stats.symbols_count, 0);

        // Add file and check stats
        handle
            .add_file(
                PathBuf::from("test.rs"),
                "fn a() {} fn b() {}".to_string(),
                Language::Rust,
            )
            .await
            .unwrap();

        let stats = handle.stats().await.unwrap();
        assert_eq!(stats.symbols_count, 2);

        handle.shutdown().await.unwrap();
        actor_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_handle_clone() {
        let (cmd_tx, cmd_rx) = command_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve::NoSemanticSource),
        );

        let actor_handle = tokio::spawn(async move { actor.run(cmd_rx).await });

        let handle1 = GraphHandle::new(cmd_tx);
        let handle2 = handle1.clone();

        // Both handles should work
        handle1
            .add_file(
                PathBuf::from("file1.rs"),
                "fn a() {}".to_string(),
                Language::Rust,
            )
            .await
            .unwrap();

        handle2
            .add_file(
                PathBuf::from("file2.rs"),
                "fn b() {}".to_string(),
                Language::Rust,
            )
            .await
            .unwrap();

        let stats = handle1.stats().await.unwrap();
        assert_eq!(stats.symbols_count, 2); // 1 symbol per file

        handle1.shutdown().await.unwrap();
        actor_handle.await.unwrap().unwrap();
    }
}
