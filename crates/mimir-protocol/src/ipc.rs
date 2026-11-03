//! IPC protocol for daemon communication.
//!
//! This module defines serializable request/response types for
//! communicating with the Mimir daemon over Unix sockets.

use mimir_types::{ContentHash, SymbolId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::command::{Edit, EditReport, FindOptions, SearchOptions, SyncResult, TraceOptions};
use crate::response::{FindResponse, SearchResponse, StatsResponse, SymbolSource, TraceResponse};

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

    /// Find symbols by name.
    FindSymbol { name: String, options: FindOptions },

    /// Find and trace a symbol by name.
    TraceByName { name: String, options: TraceOptions },

    /// The source of the symbol a name most likely means, with lines of context around it.
    Read {
        name: String,
        before: u32,
        after: u32,
    },

    /// The first `lines` lines of a symbol, or all of them.
    Source {
        symbol_id: SymbolId,
        lines: Option<u32>,
    },

    /// Apply edits together, or with `preview` only show them.
    Edit {
        edits: Vec<Edit>,
        preview: bool,
        backup: bool,
    },

    /// Get graph statistics.
    Stats,

    /// Bring the graph up to date and wait until nothing about it is provisional. `full` writes
    /// a new semantic index.
    Sync { full: bool },

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
    /// Pong response, with the daemon's version.
    Pong { version: String },

    /// The source of a symbol.
    Source(SymbolSource),

    /// What edits did.
    Edited(EditReport),

    /// Find symbol results.
    FindResult(FindResponse),

    /// Trace results.
    TraceResult(TraceResponse),

    /// Graph statistics.
    Stats(StatsResponse),

    /// Sync completed.
    SyncResult(SyncResult),

    /// Search results.
    SearchResult(SearchResponse),

    /// Shutdown acknowledged.
    ShutdownAck,
}

/// The version a client and a daemon must share to understand each other.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The daemon's socket for a project. It lives outside the project, in a directory only this user
/// can enter: a Unix socket's path is limited to about 100 characters, and a project's can be longer.
pub fn socket_path(root: &Path) -> io::Result<PathBuf> {
    let owner = fs::metadata(root)?.uid();
    let dir = std::env::temp_dir().join(format!("mimir-{owner}"));
    if !dir.exists() {
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
    }
    let found = fs::metadata(&dir)?;
    if found.uid() != owner || found.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is open to other users", dir.display()),
        ));
    }
    let hash = ContentHash::compute(root.as_os_str().as_encoded_bytes()).to_hex();
    Ok(dir.join(format!("{}.sock", &hash[..16])))
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
        let resp = IpcResponse::ok(
            1,
            IpcData::Pong {
                version: VERSION.to_string(),
            },
        );
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

    #[test]
    fn test_socket_path_is_short_and_private() {
        let root = std::env::current_dir().unwrap();
        let socket = socket_path(&root).unwrap();
        assert!(socket.as_os_str().len() < 100, "{}", socket.display());
        let dir = socket.parent().unwrap();
        assert_eq!(fs::metadata(dir).unwrap().mode() & 0o077, 0);
        assert_eq!(socket, socket_path(&root).unwrap());
    }
}
