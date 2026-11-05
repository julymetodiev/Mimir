//! A running language server: starting it, its messages, and the file URIs it is given. What one
//! language's server wants, such as its configuration, belongs to that language's adapter.

mod server;
mod wire;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

pub use server::{Server, stop};

/// Says that what runs should end now, for whoever waits on a tool or a server.
#[derive(Clone, Default)]
pub struct Stopping(Arc<AtomicBool>);

impl Stopping {
    pub fn begin(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn has_begun(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// How to run one language server.
pub struct ServerSpec {
    pub program: String,
    pub args: Vec<String>,
    /// Sent when it starts, and whenever it asks for its configuration.
    pub configuration: Value,
    pub capabilities: Value,
    /// A notification it reports its state with, given to [`Server::next_status`].
    pub status_method: Option<String>,
    /// Ends whatever waits for its answers.
    pub stopping: Stopping,
}

/// A definition's name: an absolute path, and a 0-based range on one line.
pub struct Location {
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub end_column: u32,
}

/// What can go wrong with a language server.
#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("cannot run {program}: {source}")]
    Start {
        program: String,
        source: std::io::Error,
    },
    #[error("cannot use the root {path}: {source}")]
    Root {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{program} gave no pipe to talk through")]
    NoPipe { program: String },
    #[error("the language server stopped: {0}")]
    Stopped(#[source] std::io::Error),
    #[error("the daemon is stopping")]
    Stopping,
    #[error("the lock on the language server's input was poisoned")]
    Poisoned,
    #[error("{method} timed out")]
    TimedOut { method: String },
    #[error("{method} failed: {message}")]
    Failed { method: String, message: String },
    #[error("{method} kept being cancelled")]
    Cancelled { method: String },
}
