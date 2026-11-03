//! What goes over the socket between a client and the daemon: requests, responses and the
//! options and results they carry.

mod command;
pub mod ipc;
mod response;

pub use command::{FindOptions, SearchOptions, SyncResult, TraceOptions};

pub use response::{
    FindResponse, SearchResponse, StatsResponse, SymbolInfo, TraceNode, TraceRelation,
    TraceResponse,
};

pub use ipc::{
    DaemonStatus, IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult, RequestId, SOCKET_NAME,
    socket_path,
};
