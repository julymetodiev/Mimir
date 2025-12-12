//! Events emitted by the graph actor.

use mimir_types::{FileId, SymbolId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Events emitted by the graph actor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphEvent {
    /// A file was added to the graph.
    FileAdded {
        file_id: FileId,
        path: PathBuf,
        symbols_count: usize,
    },

    /// A file was updated in the graph.
    FileUpdated {
        file_id: FileId,
        path: PathBuf,
        symbols_added: usize,
        symbols_removed: usize,
    },

    /// A file was removed from the graph.
    FileRemoved {
        file_id: FileId,
        path: PathBuf,
    },

    /// A symbol was added to the graph.
    SymbolAdded {
        symbol_id: SymbolId,
        name: String,
        file_id: FileId,
    },

    /// A symbol was removed from the graph.
    SymbolRemoved {
        symbol_id: SymbolId,
        name: String,
    },

    /// A sync operation started.
    SyncStarted,

    /// A sync operation completed.
    SyncCompleted {
        files_processed: usize,
        duration_ms: u64,
    },

    /// An error occurred.
    Error {
        message: String,
    },
}

impl GraphEvent {
    /// Check if this is an error event.
    pub fn is_error(&self) -> bool {
        matches!(self, GraphEvent::Error { .. })
    }

    /// Get the file ID if this event relates to a file.
    pub fn file_id(&self) -> Option<FileId> {
        match self {
            GraphEvent::FileAdded { file_id, .. }
            | GraphEvent::FileUpdated { file_id, .. }
            | GraphEvent::FileRemoved { file_id, .. }
            | GraphEvent::SymbolAdded { file_id, .. } => Some(*file_id),
            _ => None,
        }
    }
}

/// File system events from the watcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileEvent {
    /// A file was created.
    Created(PathBuf),

    /// A file was modified.
    Modified(PathBuf),

    /// A file was deleted.
    Deleted(PathBuf),

    /// A file was renamed.
    Renamed {
        from: PathBuf,
        to: PathBuf,
    },
}

impl FileEvent {
    /// Get the primary path involved in this event.
    pub fn path(&self) -> &PathBuf {
        match self {
            FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => p,
            FileEvent::Renamed { to, .. } => to,
        }
    }

    /// Check if this event indicates the file should be re-indexed.
    pub fn needs_reindex(&self) -> bool {
        matches!(
            self,
            FileEvent::Created(_) | FileEvent::Modified(_) | FileEvent::Renamed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_event_is_error() {
        let error = GraphEvent::Error {
            message: "test".to_string(),
        };
        assert!(error.is_error());

        let added = GraphEvent::FileAdded {
            file_id: FileId::new(1),
            path: PathBuf::from("test.rs"),
            symbols_count: 5,
        };
        assert!(!added.is_error());
    }

    #[test]
    fn test_graph_event_file_id() {
        let event = GraphEvent::FileAdded {
            file_id: FileId::new(42),
            path: PathBuf::from("test.rs"),
            symbols_count: 5,
        };
        assert_eq!(event.file_id(), Some(FileId::new(42)));

        let sync = GraphEvent::SyncStarted;
        assert_eq!(sync.file_id(), None);
    }

    #[test]
    fn test_file_event_path() {
        let created = FileEvent::Created(PathBuf::from("test.rs"));
        assert_eq!(created.path(), &PathBuf::from("test.rs"));

        let renamed = FileEvent::Renamed {
            from: PathBuf::from("old.rs"),
            to: PathBuf::from("new.rs"),
        };
        assert_eq!(renamed.path(), &PathBuf::from("new.rs"));
    }

    #[test]
    fn test_file_event_needs_reindex() {
        assert!(FileEvent::Created(PathBuf::from("test.rs")).needs_reindex());
        assert!(FileEvent::Modified(PathBuf::from("test.rs")).needs_reindex());
        assert!(!FileEvent::Deleted(PathBuf::from("test.rs")).needs_reindex());
    }
}
