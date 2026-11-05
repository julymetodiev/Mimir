//! File system watcher for automatic graph updates.

use crate::error::GraphError;
use mimir_types::Language;
use notify::event::{AccessKind, AccessMode};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::mpsc as std_mpsc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::channel::CommandSender;
use crate::command::GraphCommand;
use crate::scanner;

/// Configuration for the file watcher.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Debounce duration for file events.
    pub debounce: Duration,
    /// Whether to watch recursively.
    pub recursive: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(100),
            recursive: true,
        }
    }
}

/// What a report from the file system means for the graph. A scan decides what belongs there, so
/// the watcher asks the same question.
#[derive(Debug, Clone, PartialEq)]
enum WatchEvent {
    /// A file a scan would take was created or modified.
    Changed(PathBuf),
    /// A path is gone, or holds what a scan would leave out.
    Removed(PathBuf),
    /// A directory changed, which may mean files came with it that are not reported one by one.
    Rescan,
}

/// The meaning of a report about `path`.
fn event_for(root: &Path, path: PathBuf) -> Option<WatchEvent> {
    if path.is_dir() {
        return scanner::visits(root, &path).then_some(WatchEvent::Rescan);
    }
    Some(if scanner::takes(root, &path) {
        WatchEvent::Changed(path)
    } else {
        WatchEvent::Removed(path)
    })
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
}

impl FileWatcher {
    /// Create a new file watcher.
    pub fn new(root: PathBuf, config: WatcherConfig, commands: CommandSender) -> Self {
        Self {
            root,
            config,
            commands,
        }
    }

    /// Start watching for file changes, and return once the watching has begun: a change made
    /// after this is reported, so a scan that starts after this misses nothing.
    pub async fn start(self) -> Result<WatcherHandle, GraphError> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let (event_tx, mut event_rx) = mpsc::channel::<WatchEvent>(256);
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();

        let root = self.root.clone();
        let recursive = self.config.recursive;
        let debounce = self.config.debounce;

        // The events go to a thread of their own, as notify calls back from outside the runtime.
        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let mode = if recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            let started = notify::recommended_watcher(tx).and_then(|mut watcher| {
                watcher.watch(&root, mode)?;
                Ok(watcher)
            });
            let _watcher = match started {
                Ok(watcher) => {
                    if ready_tx.send(Ok(())).is_err() {
                        debug!("Nobody waits for the watcher any more");
                    }
                    watcher
                }
                Err(e) => {
                    if ready_tx.send(Err(e.to_string())).is_err() {
                        debug!("Nobody waits for the watcher any more");
                    }
                    return;
                }
            };
            info!("Watching {} for changes", root.display());

            while let Some(reports) = next_reports(&rx, debounce) {
                for event in reports.meanings(&root) {
                    if event_tx.blocking_send(event).is_err() {
                        debug!("The watcher's event handler has stopped");
                        return;
                    }
                }
            }
        });

        match ready_rx.await {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => {
                return Err(GraphError::Watch {
                    path: self.root,
                    reason,
                });
            }
            Err(_) => {
                return Err(GraphError::Watch {
                    path: self.root,
                    reason: "the watcher's thread stopped".to_string(),
                });
            }
        }

        // Spawn the event handler task. It ends when the actor has stopped.
        let commands = self.commands.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        // A scan starts after all that waits was reported, and covers it.
                        if event == WatchEvent::Rescan {
                            while event_rx.try_recv().is_ok() {}
                        }
                        if let Err(e) = Self::handle_event(&commands, event).await {
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
    async fn handle_event(commands: &CommandSender, event: WatchEvent) -> Result<(), GraphError> {
        match event {
            WatchEvent::Changed(path) => {
                let language = Language::from_path(&path);
                if !language.is_known() {
                    debug!("Ignoring file in no known language: {}", path.display());
                    return Ok(());
                }

                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Failed to read {}: {}", path.display(), e);
                        return Ok(());
                    }
                };

                // Adding a file the graph has replaces it.
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
                    Ok(Ok(_)) => info!("File changed: {}", path.display()),
                    Ok(Err(e)) => warn!("Failed to add {}: {}", path.display(), e),
                    Err(_) => warn!("The graph actor stopped before adding {}", path.display()),
                }
            }

            WatchEvent::Removed(path) => {
                commands
                    .send(GraphCommand::RemovePath { path })
                    .await
                    .map_err(|_| GraphError::EngineStopped)?;
            }

            WatchEvent::Rescan => {
                let (reply_tx, reply_rx) = oneshot::channel();
                commands
                    .send(GraphCommand::Sync { reply: reply_tx })
                    .await
                    .map_err(|_| GraphError::EngineStopped)?;
                match reply_rx.await {
                    Ok(Ok(_)) => debug!("Rescanned after a directory changed"),
                    Ok(Err(e)) => warn!("A rescan failed: {e}"),
                    Err(_) => warn!("The graph actor stopped during a rescan"),
                }
            }
        }
        Ok(())
    }
}

/// Whether a report can mean that something changed. Linux also reports each file and directory that
/// is opened, read and closed, by anyone: a language server reading a project is no change to it.
fn is_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

/// What the file system reported in one short while.
#[derive(Debug, Default, PartialEq)]
struct Reports {
    paths: BTreeSet<PathBuf>,
    /// Reports were lost, as a queue overflowed, so only a scan can tell what changed.
    lost: bool,
}

impl Reports {
    fn add(&mut self, event: Event) {
        self.lost |= event.need_rescan();
        if is_change(&event.kind) {
            self.paths.extend(event.paths);
        }
    }

    /// What they mean, with at most one rescan: it covers the whole project.
    fn meanings(self, root: &Path) -> Vec<WatchEvent> {
        let mut meant = Vec::new();
        if self.lost {
            meant.push(WatchEvent::Rescan);
        }
        for path in self.paths {
            if let Some(event) = event_for(root, path)
                && !(event == WatchEvent::Rescan && meant.contains(&WatchEvent::Rescan))
            {
                meant.push(event);
            }
        }
        meant
    }
}

/// Wait for a report that can mean a change, and gather what follows it for this long. None when the
/// watcher has stopped.
fn next_reports(
    reported: &std_mpsc::Receiver<notify::Result<Event>>,
    gather: Duration,
) -> Option<Reports> {
    let mut reports = Reports::default();
    let add = |reports: &mut Reports, report: notify::Result<Event>| match report {
        Ok(event) => reports.add(event),
        Err(e) => error!("Watch error: {e:?}"),
    };
    while reports == Reports::default() {
        add(&mut reports, reported.recv().ok()?);
    }
    let until = Instant::now() + gather;
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        match reported.recv_timeout(left) {
            Ok(report) => add(&mut reports, report),
            Err(_) => break,
        }
    }
    Some(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::command_channel;
    use notify::event::{DataChange, Flag, ModifyKind};
    use tempfile::TempDir;

    #[test]
    fn test_watcher_config_default() {
        let config = WatcherConfig::default();
        assert_eq!(config.debounce, Duration::from_millis(100));
        assert!(config.recursive);
    }

    #[test]
    fn test_a_report_means_what_a_scan_would_do() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), "skipped/\n").unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::create_dir_all(root.join("skipped")).unwrap();
        for file in ["build/kept.rs", "skipped/left.rs"] {
            std::fs::write(root.join(file), "fn a() {}\n").unwrap();
        }

        let meaning = |path: &str| event_for(root, root.join(path));
        // A directory named as a build's is kept if git does not ignore it, as in a scan.
        assert_eq!(
            meaning("build/kept.rs"),
            Some(WatchEvent::Changed(root.join("build/kept.rs")))
        );
        assert_eq!(
            meaning("skipped/left.rs"),
            Some(WatchEvent::Removed(root.join("skipped/left.rs")))
        );
        assert_eq!(
            meaning("gone.rs"),
            Some(WatchEvent::Removed(root.join("gone.rs")))
        );
        assert_eq!(meaning("build"), Some(WatchEvent::Rescan));
        assert_eq!(meaning("skipped"), None);
    }

    #[test]
    fn opening_and_reading_is_no_change() {
        for kind in [
            AccessKind::Open(AccessMode::Any),
            AccessKind::Read,
            AccessKind::Close(AccessMode::Read),
            AccessKind::Any,
        ] {
            assert!(!is_change(&EventKind::Access(kind)), "{kind:?}");
        }
        assert!(is_change(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(is_change(&EventKind::Modify(ModifyKind::Data(
            DataChange::Any
        ))));
        assert!(is_change(&EventKind::Any));
    }

    #[test]
    fn reports_lost_mean_a_rescan_once() {
        let dir = TempDir::new().unwrap();
        let mut reports = Reports::default();
        reports.add(Event::new(EventKind::Other).set_flag(Flag::Rescan));
        reports.add(Event::new(EventKind::Any).add_path(dir.path().to_path_buf()));
        reports.add(
            Event::new(EventKind::Access(AccessKind::Read)).add_path(dir.path().join("read.rs")),
        );
        assert_eq!(reports.paths.len(), 1);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert_eq!(reports.meanings(dir.path()), [WatchEvent::Rescan]);
    }

    /// On Linux the system reports each read: this holds only if they are left out.
    #[tokio::test]
    async fn a_file_read_changes_nothing_and_a_file_written_does() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        let file = root.join("a.rs");
        std::fs::write(&file, "fn a() {}\n").unwrap();
        let (commands, mut received) = command_channel(16);
        let _watcher = FileWatcher::new(root.clone(), WatcherConfig::default(), commands)
            .start()
            .await
            .unwrap();
        // What the writing above reported has gone by.
        tokio::time::sleep(Duration::from_millis(400)).await;
        while received.try_recv().is_ok() {}

        std::fs::read_to_string(&file).unwrap();
        std::fs::read_dir(&root).unwrap().for_each(drop);
        let quiet = tokio::time::timeout(Duration::from_millis(500), received.recv()).await;
        assert!(quiet.is_err(), "reading was taken for a change");

        std::fs::write(&file, "fn b() {}\n").unwrap();
        let change = tokio::time::timeout(Duration::from_secs(5), received.recv()).await;
        assert!(
            matches!(change, Ok(Some(GraphCommand::AddFile { .. }))),
            "a write was not seen"
        );
    }
}
