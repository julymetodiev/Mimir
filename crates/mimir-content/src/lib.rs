//! Content-addressed storage for the Mimir semantic code intelligence engine.
//!
//! Provides content-addressed storage with Blake3 hashing and zstd compression.

pub mod backend;
mod compression;
mod hash;
mod traits;

pub use backend::{FilesystemContentStore, MemoryContentStore};
pub use compression::{compress, decompress};
pub use hash::compute_hash;
pub use traits::ContentStore;
