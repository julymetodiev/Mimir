//! Starting the engine for a project: the actor, its languages and the file watcher.

use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_resolve::SemanticSource;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, warn};

use crate::actor::GraphActor;
use crate::channel::{command_channel, event_channel};
use crate::handle::GraphHandle;
use crate::watcher::{FileWatcher, WatcherConfig, WatcherHandle};

/// A running engine.
pub struct Engine {
    handle: GraphHandle,
    /// Kept so the watcher runs as long as the engine.
    _watcher: Option<WatcherHandle>,
}

impl Engine {
    /// The handle to send it commands with.
    pub fn handle(&self) -> &GraphHandle {
        &self.handle
    }

    /// Stop the actor.
    pub async fn stop(self) {
        if let Err(e) = self.handle.shutdown().await {
            warn!("The engine was already stopped: {e}");
        }
    }
}

/// Start the engine for `root` and queue the first scan, which any later command waits for.
pub async fn start(root: PathBuf, semantics: Arc<dyn SemanticSource>) -> Engine {
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));

    let (commands, receiver) = command_channel(256);
    let (events, _) = event_channel(256);
    let actor = GraphActor::with_registry(root.clone(), Arc::new(registry), semantics);
    let actor_events = events.clone();
    tokio::spawn(async move {
        if let Err(e) = actor.run(receiver, actor_events).await {
            error!("The graph actor failed: {e}");
        }
    });

    let watcher = FileWatcher::new(root, WatcherConfig::default(), commands.clone());
    let watcher = match watcher.start().await {
        Ok(watcher) => Some(watcher),
        Err(e) => {
            warn!("No file watching: {e}. Run `mimir sync` after a change.");
            None
        }
    };

    let handle = GraphHandle::new(commands, events);
    handle.queue_first_scan().await;
    Engine {
        handle,
        _watcher: watcher,
    }
}
