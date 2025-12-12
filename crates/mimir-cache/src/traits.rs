//! Cache trait definition.

use std::hash::Hash;

/// Statistics for a cache.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of evictions.
    pub evictions: u64,
    /// Current number of entries.
    pub entries: u64,
    /// Approximate size in bytes.
    pub size_bytes: u64,
}

impl CacheStats {
    /// Calculate the hit rate (0.0 to 1.0).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// A generic cache trait.
pub trait Cache<K, V>: Send + Sync
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Get a value from the cache.
    fn get(&self, key: &K) -> Option<V>;

    /// Insert a value into the cache.
    fn insert(&self, key: K, value: V);

    /// Remove a value from the cache.
    fn remove(&self, key: &K) -> Option<V>;

    /// Check if the cache contains a key.
    fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Clear all entries from the cache.
    fn clear(&self);

    /// Get the number of entries in the cache.
    fn len(&self) -> usize;

    /// Check if the cache is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get cache statistics.
    fn stats(&self) -> CacheStats;
}
