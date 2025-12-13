//! CLI commands.

pub mod agent;
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

    /// Apply batch symbol edits from JSON.
    /// JSON format: [{"symbol": "name", "content": "new code"}, ...]
    EditSymbols {
        /// JSON string containing symbol edit operations.
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

    /// Run an AI agent to perform coding tasks.
    Agent {
        /// The goal or task for the agent to accomplish.
        goal: String,

        /// Model to use (default: GLM-4.6).
        #[arg(short, long)]
        model: Option<String>,

        /// Maximum iterations (tool call rounds).
        #[arg(short = 'i', long, default_value = "50")]
        max_iterations: u32,

        /// Verbose output (show all tool calls and responses).
        #[arg(short, long)]
        verbose: bool,

        /// Enable checkpoints for resumability.
        #[arg(short, long)]
        checkpoint: bool,
    },

    /// List agent tasks.
    Tasks {
        /// Filter by status (pending, running, paused, completed, failed, cancelled).
        #[arg(short, long)]
        status: Option<String>,
    },

    /// Resume a paused agent task.
    Resume {
        /// Task ID to resume.
        task_id: String,

        /// Verbose output.
        #[arg(short, long)]
        verbose: bool,
    },
}
