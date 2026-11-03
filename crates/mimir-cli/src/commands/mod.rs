//! CLI commands.

pub mod daemon;
pub mod edit;
pub mod init;
pub mod read;
pub mod search;
pub mod status;
pub mod sync;
pub mod trace;

use clap::Subcommand;
use std::path::PathBuf;

/// Daemon subcommands.
#[derive(Subcommand)]
pub enum DaemonCommands {
    /// Start the daemon. Any command does this when needed.
    Start,
    /// Serve this project in this process. A client starts this.
    #[command(hide = true)]
    Run,
    /// Stop the daemon.
    Stop,
    /// Show daemon status.
    Status,
}

/// Available commands.
#[derive(Subcommand)]
pub enum Commands {
    /// Manage the Mimir daemon, which any command starts when needed.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },

    /// Serve MCP on stdin and stdout, for coding agents.
    Mcp,

    /// Initialize Mimir in the current directory.
    Init,

    /// Show project status.
    Status,

    /// Sync the code graph from project files.
    Sync {
        /// Write a new semantic index instead of asking a running analyzer about edited files.
        #[arg(long)]
        full: bool,
    },

    /// Trace call chains for a symbol.
    Trace {
        /// Only a symbol in this file.
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
        /// File to edit (optional if --symbol is provided).
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Symbol name to edit (resolves file and line automatically).
        #[arg(long)]
        symbol: Option<String>,

        /// Edit mode: replace, insert, delete, lines, symbol.
        /// Use 'symbol' mode to replace entire symbol with new content.
        #[arg(short, long, default_value = "replace")]
        mode: String,

        /// Start position (byte offset for replace/insert/delete, line number for lines).
        /// When --symbol is used with lines mode, this is relative to the symbol's start.
        /// Not needed for 'symbol' mode.
        #[arg(short, long)]
        start: Option<u32>,

        /// End position (byte offset for replace/delete, line number for lines).
        /// When --symbol is used with lines mode, this is relative to the symbol's start.
        /// Not needed for 'symbol' mode.
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

    /// Apply batch symbol edits from JSON.
    /// JSON format: [{"symbol": "name", "content": "new code"}, ...]
    /// Use --json=- to read from stdin.
    EditSymbols {
        /// JSON string or "-" to read from stdin.
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

    /// Read source code by symbol name.
    Read {
        /// Symbol name to read.
        #[arg(short, long)]
        symbol: String,

        /// Lines of context before the symbol.
        #[arg(short = 'B', long, default_value = "0")]
        before: usize,

        /// Lines of context after the symbol.
        #[arg(short = 'A', long, default_value = "0")]
        after: usize,

        /// Show line numbers.
        #[arg(short, long, default_value = "true")]
        numbers: bool,

        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
}
