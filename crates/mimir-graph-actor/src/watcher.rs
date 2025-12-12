//! File system watcher for automatic graph updates.

use mimir_error::GraphError;
use mimir_protocol::{CommandSender, GraphCommand};
use mimir_types::Language;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, error, info, warn};

/// Default patterns to ignore when watching.
pub const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
];

/// Configuration for the file watcher.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Debounce duration for file events.
    pub debounce: Duration,
    /// Directory names to ignore.
    pub ignore_dirs: Vec<String>,
    /// Whether to watch recursively.
    pub recursive: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(100),
            ignore_dirs: DEFAULT_IGNORE_PATTERNS.iter().map(|s| s.to_string()).collect(),
            recursive: true,
        }
    }
}

impl WatcherConfig {
    /// Check if a path should be ignored.
    pub fn should_ignore(&self, path: &Path) -> bool {
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                for ignore in &self.ignore_dirs {
                    if name_str == *ignore {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Events from the file watcher.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A file was created or modified.
    Changed(PathBuf),
    /// A file was removed.
    Removed(PathBuf),
    /// An error occurred.
    #[allow(dead_code)] // Used for future error reporting
    Error(String),
}

/// A handle to control the file watcher.
#[derive(Clone)]
pub struct WatcherHandle {
    shutdown_tx: mpsc::Sender<()>,
}

impl WatcherHandle {
    /// Request shutdown of the watcher.
    pub async fn shutdown(&self) -> Result<(), GraphError> {
        self.shutdown_tx.send(()).await.map_err(|_| {
            GraphError::WatcherError("watcher already shut down".to_string())
        })
    }
}

/// File watcher that sends updates to the graph actor.
pub struct FileWatcher {
    root: PathBuf,
    config: WatcherConfig,
    commands: CommandSender,
    /// Maps watched paths to their FileId.
    file_ids: Arc<RwLock<HashMap<PathBuf, mimir_types::FileId>>>,
}

impl FileWatcher {
    /// Create a new file watcher.
    pub fn new(root: PathBuf, config: WatcherConfig, commands: CommandSender) -> Self {
        Self {
            root,
            config,
            commands,
            file_ids: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start watching for file changes.
    ///
    /// Returns a handle that can be used to shut down the watcher.
    pub async fn start(self) -> Result<WatcherHandle, GraphError> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let (event_tx, mut event_rx) = mpsc::channel::<WatchEvent>(256);

        let root = self.root.clone();
        let recursive = self.config.recursive;
        let debounce = self.config.debounce;
        let config = self.config.clone();

        // Spawn the notify watcher in a blocking task
        let event_tx_clone = event_tx.clone();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Handle::try_current() {
                Ok(rt) => rt,
                Err(_) => {
                    error!("No tokio runtime available");
                    return;
                }
            };

            let (tx, rx) = std::sync::mpsc::channel();
            let mut debouncer = match new_debouncer(debounce, tx) {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to create debouncer: {}", e);
                    return;
                }
            };

            let mode = if recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };

            if let Err(e) = debouncer.watcher().watch(&root, mode) {
                error!("Failed to start watching {}: {}", root.display(), e);
                return;
            }

            info!("Watching {} for changes", root.display());

            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(events)) => {
                        for event in events {
                            // Filter ignored paths
                            if config.should_ignore(&event.path) {
                                continue;
                            }

                            let watch_event = match event.kind {
                                DebouncedEventKind::Any => {
                                    if event.path.exists() {
                                        WatchEvent::Changed(event.path)
                                    } else {
                                        WatchEvent::Removed(event.path)
                                    }
                                }
                                DebouncedEventKind::AnyContinuous => continue,
                                _ => continue, // Handle future variants
                            };

                            let tx = event_tx_clone.clone();
                            rt.spawn(async move {
                                let _ = tx.send(watch_event).await;
                            });
                        }
                    }
                    Ok(Err(e)) => {
                        error!("Watch error: {:?}", e);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // Check if we should stop (poll periodically)
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        break;
                    }
                }
            }
        });

        // Spawn the event handler task
        let commands = self.commands.clone();
        let file_ids = self.file_ids.clone();
        let _root = self.root.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        Self::handle_event(&commands, &file_ids, event).await;
                    }
                    _ = shutdown_rx.recv() => {
                        info!("File watcher shutting down");
                        break;
                    }
                }
            }
        });

        Ok(WatcherHandle { shutdown_tx })
    }

    async fn handle_event(
        commands: &CommandSender,
        file_ids: &Arc<RwLock<HashMap<PathBuf, mimir_types::FileId>>>,
        event: WatchEvent,
    ) {
        match event {
            WatchEvent::Changed(path) => {
                // Only process files (not directories)
                if path.is_dir() {
                    return;
                }

                // Only process files with known languages
                let language = match path.extension().and_then(|e| e.to_str()) {
                    Some(ext) => {
                        let lang = Language::from_extension(ext);
                        if lang == Language::Unknown {
                            debug!("Ignoring file with unknown extension: {}", path.display());
                            return;
                        }
                        lang
                    }
                    None => {
                        debug!("Ignoring file without extension: {}", path.display());
                        return;
                    }
                };

                // Read file content
                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Failed to read {}: {}", path.display(), e);
                        return;
                    }
                };

                // Check if we already have this file
                let existing_id = {
                    let ids = file_ids.read().await;
                    ids.get(&path).copied()
                };

                if let Some(file_id) = existing_id {
                    // Remove old version first
                    let (reply_tx, _reply_rx) = oneshot::channel();
                    let _ = commands
                        .send(GraphCommand::RemoveFile {
                            file_id,
                            reply: reply_tx,
                        })
                        .await;
                }

                // Add the file
                let (reply_tx, reply_rx) = oneshot::channel();
                if commands
                    .send(GraphCommand::AddFile {
                        path: path.clone(),
                        content,
                        language,
                        reply: reply_tx,
                    })
                    .await
                    .is_ok()
                {
                    if let Ok(Ok(file_id)) = reply_rx.await {
                        let mut ids = file_ids.write().await;
                        ids.insert(path.clone(), file_id);
                        debug!("Added/updated file: {} -> {:?}", path.display(), file_id);
                    }
                }
            }

            WatchEvent::Removed(path) => {
                let file_id = {
                    let mut ids = file_ids.write().await;
                    ids.remove(&path)
                };

                if let Some(file_id) = file_id {
                    let (reply_tx, _reply_rx) = oneshot::channel();
                    let _ = commands
                        .send(GraphCommand::RemoveFile {
                            file_id,
                            reply: reply_tx,
                        })
                        .await;
                    debug!("Removed file: {} -> {:?}", path.display(), file_id);
                }
            }

            WatchEvent::Error(msg) => {
                error!("Watcher error: {}", msg);
            }
        }
    }

    /// Register a file that was added through other means (e.g., initial sync).
    pub async fn register_file(&self, path: PathBuf, file_id: mimir_types::FileId) {
        let mut ids = self.file_ids.write().await;
        ids.insert(path, file_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watcher_config_default() {
        let config = WatcherConfig::default();
        assert_eq!(config.debounce, Duration::from_millis(100));
        assert!(config.recursive);
    }

    #[test]
    fn test_should_ignore() {
        let config = WatcherConfig::default();

        assert!(config.should_ignore(Path::new("/project/target/debug/main")));
        assert!(config.should_ignore(Path::new("/project/node_modules/package/index.js")));
        assert!(config.should_ignore(Path::new("/project/.git/objects/abc")));

        assert!(!config.should_ignore(Path::new("/project/src/main.rs")));
        assert!(!config.should_ignore(Path::new("/project/lib/utils.rs")));
    }
}
