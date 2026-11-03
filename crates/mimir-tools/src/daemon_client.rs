//! Daemon client - connects to running daemon for IPC.
//!
//! This module provides both the low-level DaemonClient and the
//! high-level DaemonGraphClient that implements the GraphClient trait.

use crate::{GraphClient, GraphError, SearchResult, SymbolInfo, TraceResult};
use anyhow::{Context, Result};
use async_trait::async_trait;
use mimir_protocol::{
    socket_path, FindOptions, IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult,
    SearchOptions, TraceOptions,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

/// Client for communicating with the Mimir daemon.
pub struct DaemonClient {
    stream: UnixStream,
    next_id: AtomicU64,
}

impl DaemonClient {
    /// Connect to the daemon for the given project.
    pub async fn connect(agent_dir: &Path) -> Result<Self> {
        let socket = socket_path(agent_dir);

        if !socket.exists() {
            anyhow::bail!(
                "Daemon not running. Start it with 'mimir daemon start' or run 'mimir sync' directly."
            );
        }

        let stream = UnixStream::connect(&socket)
            .await
            .context("Failed to connect to daemon")?;

        Ok(Self {
            stream,
            next_id: AtomicU64::new(1),
        })
    }

    /// Check if daemon is running.
    #[allow(dead_code)]
    pub fn is_daemon_running(agent_dir: &Path) -> bool {
        let socket = socket_path(agent_dir);
        socket.exists()
    }

    /// Send a command and receive response.
    async fn send_command(&mut self, command: IpcCommand) -> Result<IpcResponse> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(id, command);

        // Send request
        let json = serde_json::to_string(&request)? + "\n";
        self.stream.write_all(json.as_bytes()).await?;

        // Read response
        let mut reader = BufReader::new(&mut self.stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: IpcResponse =
            serde_json::from_str(&line).context("Failed to parse daemon response")?;

        Ok(response)
    }

    /// Ping the daemon.
    pub async fn ping(&mut self) -> Result<bool> {
        let response = self.send_command(IpcCommand::Ping).await?;
        Ok(matches!(response.result, IpcResult::Ok(IpcData::Pong)))
    }

    /// Find symbols by name.
    pub async fn find_symbol(
        &mut self,
        name: &str,
        options: FindOptions,
    ) -> Result<mimir_protocol::FindResponse> {
        let response = self
            .send_command(IpcCommand::FindSymbol {
                name: name.to_string(),
                options,
            })
            .await?;

        match response.result {
            IpcResult::Ok(IpcData::FindResult(r)) => Ok(r),
            IpcResult::Ok(_) => anyhow::bail!("Unexpected response type"),
            IpcResult::Err(e) => anyhow::bail!("{}", e),
        }
    }

    /// Trace a symbol by name.
    pub async fn trace_by_name(
        &mut self,
        name: &str,
        options: TraceOptions,
    ) -> Result<mimir_protocol::TraceResponse> {
        let response = self
            .send_command(IpcCommand::TraceByName {
                name: name.to_string(),
                options,
            })
            .await?;

        match response.result {
            IpcResult::Ok(IpcData::TraceResult(r)) => Ok(r),
            IpcResult::Ok(_) => anyhow::bail!("Unexpected response type"),
            IpcResult::Err(e) => anyhow::bail!("{}", e),
        }
    }

    /// Get graph statistics.
    pub async fn stats(&mut self) -> Result<mimir_protocol::StatsResponse> {
        let response = self.send_command(IpcCommand::Stats).await?;

        match response.result {
            IpcResult::Ok(IpcData::Stats(s)) => Ok(s),
            IpcResult::Ok(_) => anyhow::bail!("Unexpected response type"),
            IpcResult::Err(e) => anyhow::bail!("{}", e),
        }
    }

    /// Trigger a sync.
    pub async fn sync(&mut self) -> Result<mimir_protocol::SyncResult> {
        let response = self.send_command(IpcCommand::Sync).await?;

        match response.result {
            IpcResult::Ok(IpcData::SyncResult(r)) => Ok(r),
            IpcResult::Ok(_) => anyhow::bail!("Unexpected response type"),
            IpcResult::Err(e) => anyhow::bail!("{}", e),
        }
    }

    /// Search for symbols by pattern.
    pub async fn search(&mut self, options: SearchOptions) -> Result<mimir_protocol::SearchResponse> {
        let response = self.send_command(IpcCommand::Search { options }).await?;

        match response.result {
            IpcResult::Ok(IpcData::SearchResult(r)) => Ok(r),
            IpcResult::Ok(_) => anyhow::bail!("Unexpected response type"),
            IpcResult::Err(e) => anyhow::bail!("{}", e),
        }
    }

    /// Request daemon shutdown.
    pub async fn shutdown(&mut self) -> Result<()> {
        let response = self.send_command(IpcCommand::Shutdown).await?;

        match response.result {
            IpcResult::Ok(IpcData::ShutdownAck) => Ok(()),
            IpcResult::Ok(_) => anyhow::bail!("Unexpected response type"),
            IpcResult::Err(e) => anyhow::bail!("{}", e),
        }
    }
}

/// Check if daemon is running for a project.
pub fn check_daemon_running(agent_dir: &Path) -> bool {
    let socket = socket_path(agent_dir);
    let pid_file = agent_dir.join("daemon.pid");

    if !socket.exists() || !pid_file.exists() {
        return false;
    }

    // Check if PID is still running
    if let Ok(pid_str) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = pid_str.trim().parse::<u32>() {
            #[cfg(unix)]
            {
                use std::process::Command;
                let output = Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .output();
                return output.map(|o| o.status.success()).unwrap_or(false);
            }
            #[cfg(not(unix))]
            {
                return true;
            }
        }

    false
}

/// GraphClient implementation using the daemon.
pub struct DaemonGraphClient {
    client: Arc<Mutex<DaemonClient>>,
    project_root: PathBuf,
}

impl DaemonGraphClient {
    /// Create a new DaemonGraphClient.
    pub fn new(client: DaemonClient, project_root: PathBuf) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            project_root,
        }
    }

    /// Try to connect to the daemon.
    pub async fn connect(agent_dir: &Path, project_root: PathBuf) -> Result<Self, GraphError> {
        let client = DaemonClient::connect(agent_dir)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;
        Ok(Self::new(client, project_root))
    }
}

#[async_trait]
impl GraphClient for DaemonGraphClient {
    async fn search(
        &self,
        pattern: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<SearchResult, GraphError> {
        let mut client = self.client.lock().await;

        let options = SearchOptions {
            pattern: pattern.to_string(),
            kind_filter: kind.map(String::from),
            limit: Some(limit as u32),
        };

        let response = client
            .search(options)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;

        let symbols: Vec<SymbolInfo> = response
            .symbols
            .into_iter()
            .map(|s| SymbolInfo {
                name: s.name,
                qualified_name: s.qualified_name.unwrap_or_default(),
                kind: format!("{:?}", s.kind),
                file: s.file_path.to_string_lossy().to_string(),
                start_line: s.start_line,
                end_line: s.end_line,
                code: None,
            })
            .collect();

        Ok(SearchResult {
            total: response.total_count,
            symbols,
        })
    }

    async fn trace(&self, symbol: &str, depth: u32) -> Result<TraceResult, GraphError> {
        let mut client = self.client.lock().await;

        let options = TraceOptions {
            max_depth: Some(depth),
            callers_only: false,
            callees_only: false,
            cross_language: false,
        };

        let response = client
            .trace_by_name(symbol, options)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;

        let root = response.root;
        let symbol_info = SymbolInfo {
            name: root.name,
            qualified_name: root.qualified_name.unwrap_or_default(),
            kind: format!("{:?}", root.kind),
            file: root.file_path.to_string_lossy().to_string(),
            start_line: root.start_line,
            end_line: root.end_line,
            code: None,
        };

        let callers: Vec<SymbolInfo> = response
            .callers
            .into_iter()
            .map(|node| SymbolInfo {
                name: node.symbol.name,
                qualified_name: node.symbol.qualified_name.unwrap_or_default(),
                kind: format!("{:?}", node.symbol.kind),
                file: node.symbol.file_path.to_string_lossy().to_string(),
                start_line: node.symbol.start_line,
                end_line: node.symbol.end_line,
                code: None,
            })
            .collect();

        let callees: Vec<SymbolInfo> = response
            .callees
            .into_iter()
            .map(|node| SymbolInfo {
                name: node.symbol.name,
                qualified_name: node.symbol.qualified_name.unwrap_or_default(),
                kind: format!("{:?}", node.symbol.kind),
                file: node.symbol.file_path.to_string_lossy().to_string(),
                start_line: node.symbol.start_line,
                end_line: node.symbol.end_line,
                code: None,
            })
            .collect();

        Ok(TraceResult {
            symbol: symbol_info,
            callers,
            callees,
        })
    }

    async fn find_symbol(&self, name: &str) -> Result<Option<SymbolInfo>, GraphError> {
        let mut client = self.client.lock().await;

        let options = FindOptions {
            file_pattern: None,
            language: None,
            limit: Some(100),
            include_deps: false,
        };

        let response = client
            .find_symbol(name, options)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;

        // Priority: impl > struct > function > enum/trait > enum_variant
        let best = response
            .symbols
            .into_iter()
            .filter(|s| s.name == name)
            .max_by_key(|s| {
                let kind = format!("{:?}", s.kind).to_lowercase();
                match kind.as_str() {
                    "impl" => 100,
                    "struct" | "class" => 90,
                    "function" => 80,
                    "enum" | "trait" | "interface" => 70,
                    "enumvariant" | "enum_variant" => 10,
                    _ => 60,
                }
            });

        Ok(best.map(|s| SymbolInfo {
            name: s.name,
            qualified_name: s.qualified_name.unwrap_or_default(),
            kind: format!("{:?}", s.kind),
            file: s.file_path.to_string_lossy().to_string(),
            start_line: s.start_line,
            end_line: s.end_line,
            code: None,
        }))
    }

    async fn read_symbol(&self, name: &str) -> Result<Option<(SymbolInfo, String)>, GraphError> {
        let info = match self.find_symbol(name).await? {
            Some(i) => i,
            None => return Ok(None),
        };

        let file_path = self.project_root.join(&info.file);
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| GraphError::new(format!("Failed to read file: {}", e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let start = (info.start_line as usize).saturating_sub(1);
        let end = (info.end_line as usize).min(lines.len());

        if start >= lines.len() {
            return Err(GraphError::new("Invalid line range"));
        }

        let code = lines[start..end].join("\n");
        Ok(Some((info, code)))
    }
}
