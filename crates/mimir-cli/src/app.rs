//! Application state and configuration.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

/// The name of the Mimir data directory.
pub const MIMIR_DIR: &str = ".mimir";

/// Sync configuration.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Automatically sync before queries (trace, find, etc.)
    pub sync_on_query: bool,
    /// Automatically sync after edits.
    pub sync_on_edit: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            sync_on_query: true,
            sync_on_edit: true,
        }
    }
}

/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Root directory of the project.
    pub root: PathBuf,

    /// Path to the .mimir directory.
    pub agent_dir: PathBuf,

    /// Path to the graph directory.
    pub graph_dir: PathBuf,

    /// Sync configuration.
    pub sync: SyncConfig,
}

impl Config {
    /// Create a config for the given root directory.
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let agent_dir = root.join(MIMIR_DIR);
        let graph_dir = agent_dir.join("graph");

        Self {
            root,
            agent_dir,
            graph_dir,
            sync: SyncConfig::default(),
        }
    }

    /// Check if Mimir is initialized in this directory.
    pub fn is_initialized(&self) -> bool {
        self.agent_dir.exists()
    }

    /// Create the directory structure.
    pub async fn create_directories(&self) -> Result<()> {
        fs::create_dir_all(&self.agent_dir)
            .await
            .context("Failed to create .mimir directory")?;
        fs::create_dir_all(&self.graph_dir)
            .await
            .context("Failed to create graph directory")?;
        Ok(())
    }
}

/// Application state.
pub struct App {
    /// Configuration.
    pub config: Config,
}

impl App {
    /// Create a new app for the current directory.
    pub fn new() -> Result<Self> {
        let root = std::env::current_dir().context("Failed to get current directory")?;
        Self::with_root(root)
    }

    /// Create a new app for the given root directory.
    pub fn with_root(root: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            config: Config::new(root),
        })
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new().expect("Failed to get current directory")
    }
}

/// Find the project root by looking for .mimir or .git directories.
pub fn find_project_root() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    let mut path = current.as_path();

    loop {
        // Check for .mimir directory
        if path.join(MIMIR_DIR).exists() {
            return Some(path.to_path_buf());
        }

        // Check for .git directory (fallback)
        if path.join(".git").exists() {
            return Some(path.to_path_buf());
        }

        // Move to parent
        path = path.parent()?;
    }
}
