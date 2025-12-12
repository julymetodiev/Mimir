//! Snapshot management for the Mimir semantic code intelligence engine.
//!
//! Provides snapshot creation, listing, and restoration functionality.

mod create;
mod list;
mod restore;
mod snapshot;

pub use create::{create_snapshot, FileInfo};
pub use list::list_snapshots;
pub use restore::restore_snapshot;
pub use snapshot::{Snapshot, SnapshotMetadata};
