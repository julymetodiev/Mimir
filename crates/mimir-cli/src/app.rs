//! Application state and configuration.

use anyhow::{Context, Result};
use mimir_content::FilesystemContentStore;
use mimir_error::StorageError;
use mimir_files::{FileIndex, FilesHandle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;

/// The name of the Mimir data directory.
pub const MIMIR_DIR: &str = ".agent";

/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Root directory of the project.
    pub root: PathBuf,

    /// Path to the .agent directory.
    pub agent_dir: PathBuf,

    /// Path to the objects directory.
    pub objects_dir: PathBuf,

    /// Path to the index directory.
    pub index_dir: PathBuf,

    /// Path to the snapshots directory.
    pub snapshots_dir: PathBuf,

    /// Path to the graph directory.
    pub graph_dir: PathBuf,
}

impl Config {
    /// Create a config for the given root directory.
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let agent_dir = root.join(MIMIR_DIR);
        let objects_dir = agent_dir.join("objects");
        let index_dir = agent_dir.join("index");
        let snapshots_dir = agent_dir.join("snapshots");
        let graph_dir = agent_dir.join("graph");

        Self {
            root,
            agent_dir,
            objects_dir,
            index_dir,
            snapshots_dir,
            graph_dir,
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
            .context("Failed to create .agent directory")?;
        fs::create_dir_all(&self.objects_dir)
            .await
            .context("Failed to create objects directory")?;
        fs::create_dir_all(&self.index_dir)
            .await
            .context("Failed to create index directory")?;
        fs::create_dir_all(&self.snapshots_dir)
            .await
            .context("Failed to create snapshots directory")?;
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

    /// Files handle (if initialized).
    files_handle: Option<FilesHandle<FilesystemContentStore>>,
}

impl App {
    /// Create a new app for the current directory.
    pub fn new() -> Result<Self> {
        let root = std::env::current_dir().context("Failed to get current directory")?;
        Self::with_root(root)
    }

    /// Create a new app for the given root directory.
    pub fn with_root(root: impl AsRef<Path>) -> Result<Self> {
        let config = Config::new(root);
        Ok(Self {
            config,
            files_handle: None,
        })
    }

    /// Initialize the storage backend.
    pub async fn init_storage(&mut self) -> Result<&FilesHandle<FilesystemContentStore>, StorageError> {
        if self.files_handle.is_none() {
            let store = FilesystemContentStore::new(&self.config.objects_dir).await?;
            let index = Arc::new(FileIndex::new());
            let handle = FilesHandle::new(index, Arc::new(store));
            self.files_handle = Some(handle);
        }
        // Safe: we just ensured files_handle is Some
        match &self.files_handle {
            Some(handle) => Ok(handle),
            None => unreachable!("files_handle was just set"),
        }
    }

    /// Get the files handle if initialized.
    #[allow(dead_code)]
    pub fn files(&self) -> Option<&FilesHandle<FilesystemContentStore>> {
        self.files_handle.as_ref()
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new().expect("Failed to get current directory")
    }
}

/// Find the project root by looking for .agent or .git directories.
pub fn find_project_root() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    let mut path = current.as_path();

    loop {
        // Check for .agent directory
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
