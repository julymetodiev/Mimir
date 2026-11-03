//! Command and event protocol for the Mimir actor system.
//!
//! This crate defines the communication protocol between components:
//!
//! - **Commands**: Messages sent to actors to request actions
//! - **Events**: Notifications emitted when state changes
//! - **Responses**: Data returned from queries
//! - **Channels**: Typed channel wrappers for actor communication
//!
//! # Architecture
//!
//! The actor system uses a channel-based architecture:
//!
//! ```text
//! ┌─────────────┐     commands      ┌─────────────┐
//! │   Client    │ ─────────────────▶│   Actor     │
//! │  (Handle)   │                   │  (Worker)   │
//! │             │ ◀───────────────  │             │
//! └─────────────┘    responses      └─────────────┘
//!                        │
//!                        │ events (broadcast)
//!                        ▼
//!               ┌─────────────────┐
//!               │   Subscribers   │
//!               └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use mimir_protocol::{command_channel, GraphCommand};
//! use tokio::sync::oneshot;
//!
//! // Create a command channel
//! let (tx, mut rx) = command_channel(256);
//!
//! // Send a stats request
//! let (reply_tx, reply_rx) = oneshot::channel();
//! tx.send(GraphCommand::Stats { reply: reply_tx }).await?;
//!
//! // Receive in the actor
//! while let Some(cmd) = rx.recv().await {
//!     match cmd {
//!         GraphCommand::Stats { reply } => {
//!             let stats = get_stats();
//!             let _ = reply.send(stats);
//!         }
//!         GraphCommand::Shutdown => break,
//!         _ => {}
//!     }
//! }
//! ```

mod channel;
mod command;
mod event;
pub mod ipc;
mod response;

pub use channel::{
    CommandReceiver, CommandSender, DEFAULT_COMMAND_BUFFER, DEFAULT_EVENT_CAPACITY, EventReceiver,
    EventSender, command_channel, event_channel,
};

pub use command::{FindOptions, GraphCommand, SearchOptions, SyncResult, TraceOptions};

pub use event::{FileEvent, GraphEvent};

pub use response::{
    FindResponse, SearchResponse, StatsResponse, SymbolInfo, TraceNode, TraceRelation,
    TraceResponse,
};

pub use ipc::{
    DaemonStatus, IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult, RequestId, SOCKET_NAME,
    socket_path,
};
