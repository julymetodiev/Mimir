//! File indexing for the Mimir semantic code intelligence engine.
//!
//! Provides file scanning, indexing, and change detection.

mod entry;
mod handle;
mod ignore;
mod index;
mod language;
mod scanner;

pub use entry::FileEntry;
pub use handle::FilesHandle;
pub use index::FileIndex;
pub use scanner::{ScannedFile, Scanner};
