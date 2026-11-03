//! File system watcher for automatic graph updates.

use crate::error::GraphError;
use mimir_types::Language;
use notify::RecursiveMode;
use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::channel::CommandSender;
use crate::command::GraphCommand;

/// Default patterns to ignore when watching.
const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
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
            ignore_dirs: DEFAULT_IGNORE_PATTERNS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            recursive: true,
        }
    }
}

impl WatcherConfig {
    /// Check if a path should be ignored.
    fn should_ignore(&self, path: &Path) -> bool {
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
enum WatchEvent {
    /// A file was created or modified.
    Changed(PathBuf),
    /// A file was removed.
    Removed(PathBuf),
}

/// Keeps the file watcher running. Dropping the last handle ends the watcher's event handler.
#[derive(Clone)]
pub struct WatcherHandle {
    _shutdown_tx: mpsc::Sender<()>,
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

        // Get runtime handle BEFORE spawning the thread
        let rt = tokio::runtime::Handle::current();

        // Spawn the notify watcher in a blocking task
        let event_tx_clone = event_tx.clone();
        std::thread::spawn(move || {
            // Use the captured runtime handle
            let rt = rt;

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
                                if tx.send(watch_event).await.is_err() {
                                    debug!("The watcher's event handler has stopped");
                                }
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

        // Spawn the event handler task. It ends when the actor has stopped.
        let commands = self.commands.clone();
        let file_ids = self.file_ids.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        if let Err(e) = Self::handle_event(&commands, &file_ids, event).await {
                            warn!("The file watcher stops: {e}");
                            break;
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("File watcher shutting down");
                        break;
                    }
                }
            }
        });

        Ok(WatcherHandle {
            _shutdown_tx: shutdown_tx,
        })
    }

    /// Apply one watch event to the graph. It fails only when the actor has stopped.
    async fn handle_event(
        commands: &CommandSender,
        file_ids: &Arc<RwLock<HashMap<PathBuf, mimir_types::FileId>>>,
        event: WatchEvent,
    ) -> Result<(), GraphError> {
        match event {
            WatchEvent::Changed(path) => {
                // Only process files (not directories)
                if path.is_dir() {
                    return Ok(());
                }

                // Only process files with known languages
                let language = match path.extension().and_then(|e| e.to_str()) {
                    Some(ext) => {
                        let lang = Language::from_extension(ext);
                        if lang == Language::Unknown {
                            debug!("Ignoring file with unknown extension: {}", path.display());
                            return Ok(());
                        }
                        lang
                    }
                    None => {
                        debug!("Ignoring file without extension: {}", path.display());
                        return Ok(());
                    }
                };

                // Read file content
                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Failed to read {}: {}", path.display(), e);
                        return Ok(());
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
                    commands
                        .send(GraphCommand::RemoveFile {
                            file_id,
                            reply: reply_tx,
                        })
                        .await
                        .map_err(|_| GraphError::EngineStopped)?;
                }

                // Add the file
                let (reply_tx, reply_rx) = oneshot::channel();
                commands
                    .send(GraphCommand::AddFile {
                        path: path.clone(),
                        content,
                        language,
                        reply: reply_tx,
                    })
                    .await
                    .map_err(|_| GraphError::EngineStopped)?;
                match reply_rx.await {
                    Ok(Ok(file_id)) => {
                        let mut ids = file_ids.write().await;
                        ids.insert(path.clone(), file_id);
                        info!("File changed: {}", path.display());
                    }
                    Ok(Err(e)) => warn!("Failed to add {}: {}", path.display(), e),
                    Err(_) => warn!("The graph actor stopped before adding {}", path.display()),
                }
            }

            WatchEvent::Removed(path) => {
                let file_id = {
                    let mut ids = file_ids.write().await;
                    ids.remove(&path)
                };

                if let Some(file_id) = file_id {
                    let (reply_tx, _reply_rx) = oneshot::channel();
                    commands
                        .send(GraphCommand::RemoveFile {
                            file_id,
                            reply: reply_tx,
                        })
                        .await
                        .map_err(|_| GraphError::EngineStopped)?;
                    info!("File removed: {}", path.display());
                }
            }
        }
        Ok(())
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
