//! LRU cache implementation using moka.

use crate::traits::{Cache, CacheStats};
use moka::sync::Cache as MokaCache;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// An LRU cache backed by moka.
pub struct LruCache<K, V>
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    inner: MokaCache<K, V>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl<K, V> LruCache<K, V>
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Create a new LRU cache with the specified maximum capacity.
    pub fn new(max_capacity: u64) -> Self {
        Self {
            inner: MokaCache::new(max_capacity),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Create a new LRU cache with TTL (time-to-live).
    pub fn with_ttl(max_capacity: u64, ttl: Duration) -> Self {
        Self {
            inner: MokaCache::builder()
                .max_capacity(max_capacity)
                .time_to_live(ttl)
                .build(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Create a new LRU cache with both TTL and TTI (time-to-idle).
    pub fn with_ttl_and_tti(max_capacity: u64, ttl: Duration, tti: Duration) -> Self {
        Self {
            inner: MokaCache::builder()
                .max_capacity(max_capacity)
                .time_to_live(ttl)
                .time_to_idle(tti)
                .build(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Get the maximum capacity of the cache.
    pub fn max_capacity(&self) -> u64 {
        self.inner.policy().max_capacity().unwrap_or(0)
    }
}

impl<K, V> Cache<K, V> for LruCache<K, V>
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: &K) -> Option<V> {
        match self.inner.get(key) {
            Some(value) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(value)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    fn insert(&self, key: K, value: V) {
        self.inner.insert(key, value);
    }

    fn remove(&self, key: &K) -> Option<V> {
        self.inner.remove(key)
    }

    fn clear(&self) {
        self.inner.invalidate_all();
    }

    fn len(&self) -> usize {
        self.inner.entry_count() as usize
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: 0, // moka doesn't expose eviction count
            entries: self.inner.entry_count(),
            size_bytes: self.inner.weighted_size(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let cache: LruCache<String, i32> = LruCache::new(100);

        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);

        assert_eq!(cache.get(&"a".to_string()), Some(1));
        assert_eq!(cache.get(&"b".to_string()), Some(2));
        assert_eq!(cache.get(&"c".to_string()), None);
    }

    #[test]
    fn test_remove() {
        let cache: LruCache<String, i32> = LruCache::new(100);

        cache.insert("key".to_string(), 42);
        assert_eq!(cache.remove(&"key".to_string()), Some(42));
        assert_eq!(cache.get(&"key".to_string()), None);
    }

    #[test]
    fn test_clear() {
        let cache: LruCache<String, i32> = LruCache::new(100);

        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        // Run pending tasks to ensure inserts are processed
        cache.inner.run_pending_tasks();
        assert_eq!(cache.len(), 2);

        cache.clear();
        // Note: moka's invalidate_all is async, so we sync first
        cache.inner.run_pending_tasks();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_stats() {
        let cache: LruCache<String, i32> = LruCache::new(100);

        cache.insert("key".to_string(), 42);
        cache.get(&"key".to_string()); // hit
        cache.get(&"missing".to_string()); // miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate(), 0.5);
    }

    #[test]
    fn test_capacity_limit() {
        let cache: LruCache<i32, i32> = LruCache::new(3);

        for i in 0..10 {
            cache.insert(i, i * 10);
        }

        // Run pending tasks to ensure eviction
        cache.inner.run_pending_tasks();

        // Should have at most 3 entries
        assert!(cache.len() <= 3);
    }
}
