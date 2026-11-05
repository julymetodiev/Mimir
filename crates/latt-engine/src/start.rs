//! Starting the engine for a project: the actor, its languages and the file watcher.

use latt_parse_core::{AdapterRegistry, LanguageAdapter};
use latt_resolve::SemanticSource;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{error, warn};

use crate::actor::{GraphActor, Working};
use crate::channel::command_channel;
use crate::handle::GraphHandle;
use crate::scanner;
use crate::watcher::{FileWatcher, WatcherConfig, WatcherHandle};

/// A write takes moments, so a temporary file this old lost its writer in a crash.
const LEFTOVER_AGE: Duration = Duration::from_secs(3600);

/// A running engine.
pub struct Engine {
    handle: GraphHandle,
    /// Kept so the watcher runs as long as the engine.
    _watcher: Option<WatcherHandle>,
    /// The actor's task, which ends once what it left to save is on disk.
    actor: JoinHandle<()>,
    semantics: Arc<dyn SemanticSource>,
    working: Working,
}

impl Engine {
    /// The handle to send it commands with.
    pub fn handle(&self) -> &GraphHandle {
        &self.handle
    }

    /// Semantic resolution is running, which nobody may be waiting for.
    pub fn is_working(&self) -> bool {
        self.working.any()
    }

    /// The actor has ended without being told to, which only a bug in it does: nothing can be answered.
    pub fn has_failed(&self) -> bool {
        self.actor.is_finished()
    }

    /// Stop the actor and wait for what it has left to save, then end every tool and server and wait
    /// for that: a next daemon must not find them writing the same files. The actor goes first, so
    /// that it keeps nothing of what a stopped tool left.
    pub async fn stop(self) {
        if let Err(e) = self.handle.shutdown().await {
            warn!("The engine was already stopped: {e}");
        }
        if let Err(e) = self.actor.await {
            warn!("The graph actor failed: {e}");
        }
        self.semantics.stop();
        while self.working.any() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// Start the engine for `root` and queue the first scan, which any later command waits for.
pub async fn start(
    root: PathBuf,
    languages: Vec<Arc<dyn LanguageAdapter>>,
    semantics: Arc<dyn SemanticSource>,
) -> Engine {
    let leftovers_of = root.clone();
    tokio::task::spawn_blocking(move || {
        scanner::remove_leftover_writes(&leftovers_of, LEFTOVER_AGE);
    });

    let registry = AdapterRegistry::new();
    for language in languages {
        registry.register(language);
    }

    let (commands, receiver) = command_channel(256);
    let actor = GraphActor::with_registry(root.clone(), Arc::new(registry), semantics.clone());
    let working = actor.working();
    let actor = tokio::spawn(async move {
        if let Err(e) = actor.run(receiver).await {
            error!("The graph actor failed: {e}");
        }
    });

    let watcher = FileWatcher::new(root, WatcherConfig::default(), commands.clone());
    let watcher = match watcher.start().await {
        Ok(watcher) => Some(watcher),
        Err(e) => {
            warn!("No file watching: {e}. Run `latt sync` after a change.");
            None
        }
    };

    let handle = GraphHandle::new(commands);
    handle.queue_first_scan().await;
    Engine {
        handle,
        _watcher: watcher,
        actor,
        semantics,
        working,
    }
}
