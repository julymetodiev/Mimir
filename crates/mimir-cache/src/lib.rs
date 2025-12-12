//! Caching layer for the Mimir semantic code intelligence engine.
//!
//! Provides LRU caching using the moka crate.

mod lru;
mod traits;

pub use lru::LruCache;
pub use traits::{Cache, CacheStats};
