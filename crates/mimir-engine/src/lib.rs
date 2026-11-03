//! Actor-based graph management for Mimir.
//!
//! This crate provides an actor-based architecture for managing the code graph.
//! The actor model ensures thread-safe access to the graph while allowing
//! concurrent queries.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │   GraphHandle   │──────┐
//! │    (client)     │      │
//! └─────────────────┘      │
//!                          │ commands
//! ┌─────────────────┐      │         ┌─────────────────┐
//! │   GraphHandle   │──────┼────────▶│   GraphActor    │
//! │    (client)     │      │         │    (owner)      │
//! └─────────────────┘      │         └─────────────────┘
//!                          │                  │
//! ┌─────────────────┐      │                  │ events
//! │   GraphHandle   │──────┘                  │
//! │    (client)     │                         ▼
//! └─────────────────┘               ┌─────────────────┐
//!                                   │   Subscribers   │
//!                                   └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use mimir_engine::{GraphActor, GraphHandle};
//! use mimir_engine::{command_channel, event_channel};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Create channels
//!     let (cmd_tx, cmd_rx) = command_channel(256);
//!     let (event_tx, event_rx) = event_channel(64);
//!
//!     // Start actor
//!     let actor = GraphActor::new(PathBuf::from("."));
//!     tokio::spawn(async move {
//!         actor.run(cmd_rx, event_tx).await
//!     });
//!
//!     // Create handle
//!     let handle = GraphHandle::new(cmd_tx, event_rx);
//!
//!     // Use handle
//!     let file_id = handle.add_file(
//!         PathBuf::from("src/main.rs"),
//!         "fn main() {}".to_string(),
//!         Language::Rust,
//!     ).await?;
//!
//!     let response = handle.find_symbol("main").await?;
//!     println!("Found: {:?}", response.symbols);
//!
//!     handle.shutdown().await?;
//! }
//! ```

mod actor;
mod channel;
mod command;
mod event;
mod handle;
mod save;
mod scanner;
mod source;
mod start;
mod watcher;

pub use actor::GraphActor;
pub use channel::{
    CommandReceiver, CommandSender, DEFAULT_COMMAND_BUFFER, DEFAULT_EVENT_CAPACITY, EventReceiver,
    EventSender, command_channel, event_channel,
};
pub use command::GraphCommand;
pub use event::{FileEvent, GraphEvent};
pub use handle::GraphHandle;
pub use scanner::{ScannedFile, Scanner};
pub use start::{Engine, start};
pub use watcher::{FileWatcher, WatcherConfig, WatcherHandle};
