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
use std::time::UNIX_EPOCH;

use crate::command::{
    Edit, EditReport, FindOptions, GrepOptions, SearchOptions, SyncResult, TraceOptions,
};
use crate::response::{
    FileText, FilesResponse, FindResponse, GrepResponse, ImpactResponse, SearchResponse,
    StatsResponse, SymbolSource, TraceResponse,
};
use crate::view::{GraphEvent, GraphSnapshot};

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

    /// What changed since the last commit, or since `base` split off, and what depends on it.
    Impact {
        base: Option<String>,
        depth: Option<u32>,
    },

    /// The source of the symbol a name most likely means, with lines of context around it.
    Read {
        name: String,
        file: Option<PathBuf>,
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

    /// Search the text of the project's files.
    Grep { options: GrepOptions },

    /// The project's files, only those matching a glob if given.
    Files {
        glob: Option<String>,
        limit: Option<u32>,
    },

    /// Lines of a file of the project.
    ReadFile {
        path: PathBuf,
        first: Option<u32>,
        last: Option<u32>,
    },

    /// Write a whole file of the project.
    WriteFile {
        path: PathBuf,
        content: String,
        overwrite: bool,
    },

    /// The whole graph, for a view.
    Snapshot,

    /// Turn this connection into a stream of what the engine announces. The first answer says it
    /// has begun; each later one is an event, until the client goes away.
    Watch,

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

impl IpcResult {}

/// Data returned from IPC commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcData {
    /// The whole graph.
    Snapshot(GraphSnapshot),

    /// Watching has begun.
    Watching,

    /// Something the engine announced.
    Event(GraphEvent),

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

    /// What a change touches.
    ImpactResult(ImpactResponse),

    /// Graph statistics.
    Stats(StatsResponse),

    /// Sync completed.
    SyncResult(SyncResult),

    /// Search results.
    SearchResult(SearchResponse),

    /// Lines of text that matched.
    GrepResult(GrepResponse),

    /// Files of the project.
    FilesResult(FilesResponse),

    /// Lines of a file.
    FileText(FileText),

    /// Shutdown acknowledged.
    ShutdownAck,
}

/// What a client and a daemon must share to understand each other: the version and, because a
/// rebuild keeps the version, when this binary was built. A daemon reads it once, as it starts.
pub fn build_identity() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let built = std::env::current_exe()
        .and_then(fs::metadata)
        .and_then(|found| found.modified())
        .map(|time| {
            time.duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        });
    match built {
        Ok(built) => format!("{version}+{built}"),
        Err(_) => version.to_string(),
    }
}

/// Make a directory only `owner` can enter. Several clients can start at once, so one may find it
/// made by another.
fn ensure_private_dir(dir: &Path, owner: u32) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let found = fs::metadata(dir)?;
    if found.uid() != owner || found.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is open to other users", dir.display()),
        ));
    }
    Ok(())
}

/// The daemon's socket for a project. It lives outside the project, in a directory only this user
/// can enter: a Unix socket's path is limited to about 100 characters, and a project's can be longer.
pub fn socket_path(root: &Path) -> io::Result<PathBuf> {
    let owner = fs::metadata(root)?.uid();
    let dir = std::env::temp_dir().join(format!("mimir-{owner}"));
    ensure_private_dir(&dir, owner)?;
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
                version: build_identity(),
            },
        );
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
    }

    #[test]
    fn test_error_response() {
        let resp = IpcResponse::err(42, "Something went wrong".to_string());
        assert!(matches!(resp.result, IpcResult::Err(_)));
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

    #[test]
    fn test_clients_starting_at_once_all_get_the_directory() {
        let parent = tempfile::tempdir().unwrap();
        let owner = fs::metadata(parent.path()).unwrap().uid();
        let dir = parent.path().join("sockets");
        std::thread::scope(|threads| {
            let running: Vec<_> = (0..32)
                .map(|_| threads.spawn(|| ensure_private_dir(&dir, owner)))
                .collect();
            for thread in running {
                thread.join().unwrap().unwrap();
            }
        });
    }

    #[test]
    fn test_a_directory_open_to_others_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let owner = fs::metadata(parent.path()).unwrap().uid();
        fs::set_permissions(
            parent.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        assert!(ensure_private_dir(parent.path(), owner).is_err());
    }
}
