//! IPC protocol for daemon communication.
//!
//! This module defines serializable request/response types for
//! communicating with the Mimir daemon over Unix sockets.

use mimir_types::{FileId, Language, SymbolId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::command::{FindOptions, SearchOptions, SyncResult, TraceOptions};
use crate::response::{FindResponse, SearchResponse, StatsResponse, TraceResponse};

/// Request ID for correlating responses.
pub type RequestId = u64;

/// IPC request sent to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    /// Unique request ID.
    pub id: RequestId,
    /// The command to execute.
    pub command: IpcCommand,
}

impl IpcRequest {
    /// Create a new request with the given command.
    pub fn new(id: RequestId, command: IpcCommand) -> Self {
        Self { id, command }
    }
}

/// Commands that can be sent to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcCommand {
    /// Ping the daemon to check if it's alive.
    Ping,

    /// Add or update a file in the graph.
    AddFile {
        path: PathBuf,
        content: String,
        language: Language,
    },

    /// Remove a file from the graph.
    RemoveFile { file_id: FileId },

    /// Find symbols by name.
    FindSymbol { name: String, options: FindOptions },

    /// Trace a symbol's callers and callees.
    Trace {
        symbol_id: SymbolId,
        options: TraceOptions,
    },

    /// Find and trace a symbol by name.
    TraceByName { name: String, options: TraceOptions },

    /// Get graph statistics.
    Stats,

    /// Trigger a sync operation.
    /// Trigger a sync operation.
    Sync,

    /// Search for symbols by pattern.
    Search { options: SearchOptions },

    /// Request daemon shutdown.
    Shutdown,
}

/// IPC response from the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    /// Request ID this is responding to.
    pub id: RequestId,
    /// The result.
    pub result: IpcResult,
}

impl IpcResponse {
    /// Create a new response.
    pub fn new(id: RequestId, result: IpcResult) -> Self {
        Self { id, result }
    }

    /// Create a success response.
    pub fn ok(id: RequestId, data: IpcData) -> Self {
        Self::new(id, IpcResult::Ok(data))
    }

    /// Create an error response.
    pub fn err(id: RequestId, error: String) -> Self {
        Self::new(id, IpcResult::Err(error))
    }
}

/// Result of an IPC command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResult {
    /// Command succeeded with data.
    Ok(IpcData),
    /// Command failed with error message.
    Err(String),
}

impl IpcResult {
    /// Check if the result is ok.
    pub fn is_ok(&self) -> bool {
        matches!(self, IpcResult::Ok(_))
    }

    /// Check if the result is an error.
    pub fn is_err(&self) -> bool {
        matches!(self, IpcResult::Err(_))
    }
}

/// Data returned from IPC commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcData {
    /// Pong response.
    Pong,

    /// File was added.
    FileAdded { file_id: FileId },

    /// File was removed.
    FileRemoved { removed: bool },

    /// Find symbol results.
    FindResult(FindResponse),

    /// Trace results.
    TraceResult(TraceResponse),

    /// Graph statistics.
    Stats(StatsResponse),

    /// Sync completed.
    /// Sync completed.
    SyncResult(SyncResult),

    /// Search results.
    SearchResult(SearchResponse),

    /// Shutdown acknowledged.
    ShutdownAck,
}

/// Daemon status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    /// Process ID of the daemon.
    pub pid: u32,
    /// Project root the daemon is watching.
    pub project_root: PathBuf,
    /// Number of files indexed.
    pub files_count: usize,
    /// Number of symbols indexed.
    pub symbols_count: usize,
    /// Uptime in seconds.
    pub uptime_secs: u64,
}

/// Default socket path within the project.
pub const SOCKET_NAME: &str = "daemon.sock";

/// Get the socket path for a project.
pub fn socket_path(agent_dir: &std::path::Path) -> PathBuf {
    agent_dir.join(SOCKET_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = IpcRequest::new(1, IpcCommand::Ping);
        let json = serde_json::to_string(&req).unwrap();
        let parsed: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert!(matches!(parsed.command, IpcCommand::Ping));
    }

    #[test]
    fn test_response_serialization() {
        let resp = IpcResponse::ok(1, IpcData::Pong);
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert!(parsed.result.is_ok());
    }

    #[test]
    fn test_error_response() {
        let resp = IpcResponse::err(42, "Something went wrong".to_string());
        assert!(resp.result.is_err());
    }

    #[test]
    fn test_find_command() {
        let cmd = IpcCommand::FindSymbol {
            name: "process_data".to_string(),
            options: FindOptions::default(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("FindSymbol"));
        assert!(json.contains("process_data"));
    }

    #[test]
    fn test_trace_by_name_command() {
        let cmd = IpcCommand::TraceByName {
            name: "my_function".to_string(),
            options: TraceOptions::default(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: IpcCommand = serde_json::from_str(&json).unwrap();
        match parsed {
            IpcCommand::TraceByName { name, .. } => assert_eq!(name, "my_function"),
            _ => panic!("Wrong command type"),
        }
    }
}
