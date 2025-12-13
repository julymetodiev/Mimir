//! CLI commands.

pub mod daemon;
pub mod edit;
pub mod init;
pub mod status;
pub mod sync;
pub mod trace;
pub mod search;

use clap::Subcommand;
use std::path::PathBuf;

/// Daemon subcommands.
#[derive(Subcommand)]
pub enum DaemonCommands {
    /// Start the daemon.
    Start {
        /// Run in foreground (don't daemonize).
        #[arg(short, long)]
        foreground: bool,
    },
    /// Stop the daemon.
    Stop,
    /// Show daemon status.
    Status,
}

/// Available commands.
#[derive(Subcommand)]
pub enum Commands {
    /// Manage the Mimir daemon (background graph service).
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },

    /// Initialize Mimir in the current directory.
    Init {
        /// Force re-initialization even if already initialized.
        #[arg(short, long)]
        force: bool,
    },

    /// Show project status.
    Status,

    /// Sync the code graph from project files.
    Sync,

    /// Trace call chains for a symbol.
    Trace {
        /// Source file to analyze (optional if graph is synced).
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Symbol name to trace.
        #[arg(short, long)]
        symbol: String,

        /// Maximum trace depth.
        #[arg(short, long, default_value = "3")]
        depth: usize,

        /// Output as JSON.
        #[arg(long)]
        json: bool,

        /// Show source code snippet.
        #[arg(short, long)]
        /// Show source code snippet.
        #[arg(short, long)]
        code: bool,

        /// Show only callers (who calls this symbol).
        #[arg(long)]
        callers: bool,

        /// Show only callees (what this symbol calls).
        #[arg(long)]
        callees: bool,
    },

    /// Edit a file with conflict detection.
    Edit {
        /// File to edit.
        #[arg(short, long)]
        file: PathBuf,

        /// Edit mode: replace, insert, delete, lines.
        #[arg(short, long, default_value = "replace")]
        mode: String,

        /// Start position (byte offset for replace/insert/delete, line number for lines).
        #[arg(short, long)]
        start: u32,

        /// End position (byte offset for replace/delete, line number for lines).
        #[arg(short, long)]
        end: Option<u32>,

        /// New content to insert or replace with.
        #[arg(short, long)]
        content: Option<String>,

        /// Preview changes without applying.
        #[arg(short, long)]
        preview: bool,

        /// Create backup file before editing.
        #[arg(short, long)]
        backup: bool,
    },

    /// Apply batch edits from JSON.
    EditBatch {
        /// JSON string containing edit operations.
        #[arg(short, long)]
        json: String,

        /// Preview changes without applying.
        #[arg(short, long)]
        preview: bool,

        /// Create backup files before editing.
        #[arg(short, long)]
        backup: bool,
    },

    /// Search for symbols by pattern.
    Search {
        /// Pattern to search for (supports regex).
        #[arg(short, long)]
        pattern: String,

        /// Filter by symbol kind (function, struct, enum, etc.).
        #[arg(short, long)]
        kind: Option<String>,

        /// Maximum results to return.
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Output as JSON.
        #[arg(long)]
        json: bool,

        /// Show source code snippet.
        #[arg(short, long)]
        code: bool,
    },
}
