//! Daemon client - connects to running daemon for IPC.

use anyhow::{Context, Result};
use mimir_protocol::{
    socket_path, FindOptions, IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult,
    TraceOptions,
};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

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
    pub fn is_daemon_running(agent_dir: &Path) -> bool {
        let socket = socket_path(agent_dir);
        if !socket.exists() {
            return false;
        }

        // Try to connect
        let rt = tokio::runtime::Handle::try_current();
        if rt.is_err() {
            // No runtime, can't check properly
            return socket.exists();
        }

        // Quick ping check would be nice but for now just check socket exists
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

        let response: IpcResponse = serde_json::from_str(&line)
            .context("Failed to parse daemon response")?;

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

/// Check if daemon is running for a project (non-async version).
pub fn check_daemon_running(agent_dir: &Path) -> bool {
    let socket = socket_path(agent_dir);
    let pid_file = agent_dir.join("daemon.pid");

    if !socket.exists() || !pid_file.exists() {
        return false;
    }

    // Check if PID is still running
    if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            // On Unix, check if process exists
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
                return true; // Assume running on non-Unix
            }
        }
    }

    false
}
