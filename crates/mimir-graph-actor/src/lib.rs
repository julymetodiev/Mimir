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
//! use mimir_graph_actor::{GraphActor, GraphHandle};
//! use mimir_protocol::{command_channel, event_channel};
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
mod handle;
mod watcher;

pub use actor::GraphActor;
pub use handle::GraphHandle;
pub use watcher::{FileWatcher, WatcherConfig, WatcherHandle};
