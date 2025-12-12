//! CLI commands.

pub mod init;
pub mod status;
pub mod sync;
pub mod trace;

use clap::Subcommand;
use std::path::PathBuf;

/// Available commands.
#[derive(Subcommand)]
pub enum Commands {
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
    },
}
