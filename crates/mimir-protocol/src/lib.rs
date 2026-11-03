//! What goes over the socket between a client and the daemon: requests, responses and the
//! options and results they carry.

mod command;
pub mod ipc;
mod response;

pub use command::{FindOptions, LinkSummary, SearchOptions, SyncResult, TraceOptions};

pub use response::{
    FindResponse, SearchResponse, StatsResponse, SymbolInfo, SymbolSource, TraceNode,
    TraceRelation, TraceResponse,
};

pub use ipc::{
    IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult, RequestId, VERSION, socket_path,
};
