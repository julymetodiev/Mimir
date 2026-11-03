//! Client handle for interacting with the graph actor.

use mimir_error::GraphError;
use mimir_protocol::{
    CommandSender, EventReceiver, EventSender, FindOptions, FindResponse, GraphCommand,
    SearchOptions, SearchResponse, StatsResponse, SyncResult, TraceOptions, TraceResponse,
};
use mimir_types::{FileId, Language, SymbolId};
use std::path::PathBuf;
use tokio::sync::oneshot;

/// A handle to communicate with the graph actor.
///
/// This is a cheap-to-clone client that can be shared across tasks.
#[derive(Clone)]
pub struct GraphHandle {
    commands: CommandSender,
    events: EventSender,
}

impl GraphHandle {
    /// Create a new handle with the given command and event channels.
    pub fn new(commands: CommandSender, events: EventSender) -> Self {
        Self { commands, events }
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
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })?;

        reply_rx.await.map_err(|_| GraphError::SymbolNotFound {
            name: "reply channel closed".to_string(),
        })?
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
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })?;

        reply_rx.await.map_err(|_| GraphError::SymbolNotFound {
            name: "reply channel closed".to_string(),
        })?
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
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })?;

        reply_rx.await.map_err(|_| GraphError::SymbolNotFound {
            name: "reply channel closed".to_string(),
        })
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
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })?;

        reply_rx.await.map_err(|_| GraphError::SymbolNotFound {
            name: "reply channel closed".to_string(),
        })
    }

    /// Get graph statistics.
    pub async fn stats(&self) -> Result<StatsResponse, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::Stats { reply: reply_tx })
            .await
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })?;

        reply_rx.await.map_err(|_| GraphError::SymbolNotFound {
            name: "reply channel closed".to_string(),
        })
    }

    /// Trigger a sync operation.
    pub async fn sync(&self) -> Result<SyncResult, GraphError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.commands
            .send(GraphCommand::Sync { reply: reply_tx })
            .await
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })?;

        reply_rx.await.map_err(|_| GraphError::SymbolNotFound {
            name: "reply channel closed".to_string(),
        })?
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
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })?;

        reply_rx.await.map_err(|_| GraphError::SymbolNotFound {
            name: "reply channel closed".to_string(),
        })
    }

    /// Request shutdown.
    pub async fn shutdown(&self) -> Result<(), GraphError> {
        self.commands
            .send(GraphCommand::Shutdown)
            .await
            .map_err(|_| GraphError::SymbolNotFound {
                name: "channel closed".to_string(),
            })
    }

    /// Subscribe to graph events.
    pub fn subscribe(&self) -> EventReceiver {
        self.events.subscribe()
    }

    /// Get the event sender for creating new subscriptions.
    pub fn event_sender(&self) -> &EventSender {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::GraphActor;
    use mimir_parse_core::AdapterRegistry;
    use mimir_parse_rust::RustAdapter;
    use mimir_protocol::{command_channel, event_channel};
    use std::sync::Arc;

    fn create_test_registry() -> Arc<AdapterRegistry> {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(RustAdapter::new()));
        Arc::new(registry)
    }

    #[tokio::test]
    async fn test_handle_add_and_find() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, _event_rx) = event_channel(16);
        let event_tx_clone = event_tx.clone();

        let actor = GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

        let actor_handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        let handle = GraphHandle::new(cmd_tx, event_tx_clone);

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
        let (event_tx, _event_rx) = event_channel(16);
        let event_tx_clone = event_tx.clone();

        let actor = GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

        let actor_handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        let handle = GraphHandle::new(cmd_tx, event_tx_clone);

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
        let (event_tx, _event_rx) = event_channel(16);
        let event_tx_clone = event_tx.clone();

        let actor = GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

        let actor_handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        let handle1 = GraphHandle::new(cmd_tx, event_tx_clone);
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
