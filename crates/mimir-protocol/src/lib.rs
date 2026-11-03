//! What goes over the socket between a client and the daemon: requests, responses and the
//! options and results they carry.

mod command;
pub mod ipc;
mod response;

pub use command::{
    Edit, EditChange, EditReport, EditTarget, FileEdit, FindOptions, GrepOptions, LinkSummary,
    SearchOptions, SyncResult, TraceOptions,
};

pub use response::{
    FileText, FilesResponse, FindResponse, GrepMatch, GrepResponse, SearchResponse, StatsResponse,
    SymbolInfo, SymbolSource, TraceNode, TraceRelation, TraceResponse,
};

pub use ipc::{
    IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult, RequestId, build_identity, socket_path,
};
