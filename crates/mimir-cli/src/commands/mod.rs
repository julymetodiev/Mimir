//! CLI commands.

pub mod init;
pub mod status;

use clap::Subcommand;

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
}
