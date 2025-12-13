//! Daemon module for background graph management.
//!
//! The daemon runs a GraphActor with FileWatcher to automatically
//! keep the code graph in sync with filesystem changes.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐         ┌─────────────────────────────────────┐
//! │   CLI       │         │            Daemon Process           │
//! │  commands   │◄───────►│  GraphActor + FileWatcher           │
//! │             │   IPC   │                                     │
//! └─────────────┘  (Unix  │  - FileWatcher detects changes      │
//!                  Socket)│  - Actor syncs automatically        │
//!                         │  - CLI queries use Handle           │
//!                         └─────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```bash
//! # Start daemon in background
//! mimir daemon start
//!
//! # Check status
//! mimir daemon status
//!
//! # Stop daemon
//! mimir daemon stop
//!
//! # CLI commands automatically use daemon if running
//! mimir trace -s my_function
//! ```

mod client;
mod graph_client;
mod server;

pub use client::{check_daemon_running, DaemonClient};
pub use graph_client::DaemonGraphClient;
pub use server::DaemonServer;
