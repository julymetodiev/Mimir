//! Daemon server - runs GraphActor + FileWatcher and accepts IPC connections.

use anyhow::{Context, Result};
use mimir_graph_actor::{GraphActor, GraphHandle, FileWatcher, WatcherConfig, WatcherHandle};
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_protocol::{
    command_channel, event_channel, socket_path, IpcCommand,
    IpcData, IpcRequest, IpcResponse, IpcResult,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::app::Config;

/// Daemon server state.
pub struct DaemonServer {
    /// Project configuration.
    config: Config,
    /// Graph handle for queries.
    handle: GraphHandle,
    /// Watcher handle for file watching.
    #[allow(dead_code)]
    watcher_handle: Option<WatcherHandle>,
    /// Shutdown signal sender.
    shutdown_tx: broadcast::Sender<()>,
    /// Start time for uptime tracking.
    #[allow(dead_code)]
    start_time: Instant,
}

impl DaemonServer {
    /// Start a new daemon server.
    pub async fn start(config: Config) -> Result<Self> {
        info!("Starting Mimir daemon for: {}", config.root.display());

        // Setup adapter registry
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(RustAdapter::new()));
        let registry = Arc::new(registry);

        // Create channels
        let (cmd_tx, cmd_rx) = command_channel(256);
        let (event_tx, _event_rx) = event_channel(256);

        // Create actor
        let actor = GraphActor::with_registry(config.root.clone(), registry.clone());

        // Clone for watcher
        let cmd_tx_watcher = cmd_tx.clone();

        // Spawn actor task
        let event_tx_actor = event_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = actor.run(cmd_rx, event_tx_actor).await {
                error!("Graph actor error: {}", e);
            }
        });

        // Setup and start file watcher
        let watcher_config = WatcherConfig::default();
        let watcher = FileWatcher::new(config.root.clone(), watcher_config, cmd_tx_watcher);
        let watcher_handle = match watcher.start().await {
            Ok(handle) => {
                info!("File watcher started for: {}", config.root.display());
                Some(handle)
            }
            Err(e) => {
                warn!("Failed to start file watcher: {}. Continuing without auto-sync.", e);
                None
            }
        };

        // Create handle
        let handle = GraphHandle::new(cmd_tx, event_tx);

        // Initial sync
        info!("Performing initial sync...");
        match handle.sync().await {
            Ok(result) => {
                info!(
                    "Initial sync complete: {} files, {} symbols",
                    result.files_added, result.symbols_added
                );
            }
            Err(e) => {
                warn!("Initial sync error: {}", e);
            }
        }

        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            config,
            handle,
            watcher_handle,
            shutdown_tx,
            start_time: Instant::now(),
        })
    }

    /// Run the daemon server, listening on Unix socket.
    pub async fn run(&self) -> Result<()> {
        let socket_path = socket_path(&self.config.agent_dir);

        // Remove existing socket
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }

        // Create listener
        let listener = UnixListener::bind(&socket_path)
            .context("Failed to bind Unix socket")?;

        info!("Daemon listening on: {}", socket_path.display());

        // Write PID file
        let pid_path = self.config.agent_dir.join("daemon.pid");
        std::fs::write(&pid_path, std::process::id().to_string())?;

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let handle = self.handle.clone();
                            let shutdown_tx = self.shutdown_tx.clone();
                            let start_time = self.start_time;
                            let root = self.config.root.clone();

                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, handle, shutdown_tx, start_time, root).await {
                                    error!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Cleanup
        let _ = std::fs::remove_file(&socket_path);
        let _ = std::fs::remove_file(&pid_path);

        // Shutdown actor
        let _ = self.handle.shutdown().await;

        info!("Daemon stopped");
        Ok(())
    }

    /// Get shutdown sender for external shutdown.
    #[allow(dead_code)]
    pub fn shutdown_sender(&self) -> broadcast::Sender<()> {
        self.shutdown_tx.clone()
    }
}

/// Handle a single client connection.
async fn handle_connection(
    stream: UnixStream,
    handle: GraphHandle,
    shutdown_tx: broadcast::Sender<()>,
    start_time: Instant,
    project_root: PathBuf,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            // Connection closed
            break;
        }

        // Parse request
        let request: IpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let response = IpcResponse::err(0, format!("Invalid request: {}", e));
                let json = serde_json::to_string(&response)? + "\n";
                writer.write_all(json.as_bytes()).await?;
                continue;
            }
        };

        debug!("Received request: {:?}", request.command);

        // Process command
        let response = process_command(
            request.id,
            request.command,
            &handle,
            &shutdown_tx,
            start_time,
            &project_root,
        )
        .await;

        // Send response
        let json = serde_json::to_string(&response)? + "\n";
        writer.write_all(json.as_bytes()).await?;

        // Check for shutdown
        if matches!(response.result, IpcResult::Ok(IpcData::ShutdownAck)) {
            break;
        }
    }

    Ok(())
}

/// Process a single IPC command.
async fn process_command(
    id: u64,
    command: IpcCommand,
    handle: &GraphHandle,
    shutdown_tx: &broadcast::Sender<()>,
    _start_time: Instant,
    _project_root: &PathBuf,
) -> IpcResponse {
    match command {
        IpcCommand::Ping => IpcResponse::ok(id, IpcData::Pong),

        IpcCommand::AddFile {
            path,
            content,
            language,
        } => match handle.add_file(path, content, language).await {
            Ok(file_id) => IpcResponse::ok(id, IpcData::FileAdded { file_id }),
            Err(e) => IpcResponse::err(id, e.to_string()),
        },

        IpcCommand::RemoveFile { file_id } => match handle.remove_file(file_id).await {
            Ok(removed) => IpcResponse::ok(id, IpcData::FileRemoved { removed }),
            Err(e) => IpcResponse::err(id, e.to_string()),
        },

        IpcCommand::FindSymbol { name, options } => {
            match handle.find_symbol_with_options(&name, options).await {
                Ok(response) => IpcResponse::ok(id, IpcData::FindResult(response)),
                Err(e) => IpcResponse::err(id, e.to_string()),
            }
        }

        IpcCommand::Trace { symbol_id, options } => {
            match handle.trace_with_options(symbol_id, options).await {
                Ok(response) => IpcResponse::ok(id, IpcData::TraceResult(response)),
                Err(e) => IpcResponse::err(id, e.to_string()),
            }
        }

        IpcCommand::TraceByName { name, options } => {
            // First find the symbol
            match handle.find_symbol(&name).await {
                Ok(find_response) => {
                    if find_response.symbols.is_empty() {
                        IpcResponse::err(id, format!("Symbol '{}' not found", name))
                    } else {
                        let symbol_id = find_response.symbols[0].id;
                        match handle.trace_with_options(symbol_id, options).await {
                            Ok(response) => IpcResponse::ok(id, IpcData::TraceResult(response)),
                            Err(e) => IpcResponse::err(id, e.to_string()),
                        }
                    }
                }
                Err(e) => IpcResponse::err(id, e.to_string()),
            }
        }

        IpcCommand::Stats => match handle.stats().await {
            Ok(stats) => IpcResponse::ok(id, IpcData::Stats(stats)),
            Err(e) => IpcResponse::err(id, e.to_string()),
        },

        IpcCommand::Sync => match handle.sync().await {
            Ok(result) => IpcResponse::ok(id, IpcData::SyncResult(result)),
            Err(e) => IpcResponse::err(id, e.to_string()),
        },

        IpcCommand::Shutdown => {
            let _ = shutdown_tx.send(());
            IpcResponse::ok(id, IpcData::ShutdownAck)
        }
    }
}
