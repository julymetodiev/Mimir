//! Content store backends.

mod filesystem;
mod memory;

pub use filesystem::FilesystemContentStore;
pub use memory::MemoryContentStore;
