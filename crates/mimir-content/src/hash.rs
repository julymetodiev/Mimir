//! Blake3 hashing utilities.

use mimir_types::ContentHash;

/// Compute the Blake3 hash of data.
#[inline]
pub fn compute_hash(data: &[u8]) -> ContentHash {
    ContentHash::compute(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_hash() {
        let data = b"hello world";
        let hash = compute_hash(data);

        // Same data should produce same hash
        let hash2 = compute_hash(data);
        assert_eq!(hash, hash2);

        // Different data should produce different hash
        let hash3 = compute_hash(b"different");
        assert_ne!(hash, hash3);
    }
}
