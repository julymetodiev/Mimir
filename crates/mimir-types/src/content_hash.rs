//! Content hash type for content-addressed storage.
//!
//! Uses Blake3 for fast, cryptographically secure hashing.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A 32-byte Blake3 hash of content.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Create a ContentHash from raw bytes.
    #[inline]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Compute the hash of the given data.
    pub fn compute(data: &[u8]) -> Self {
        Self(*blake3::hash(data).as_bytes())
    }

    /// Get the raw bytes of the hash.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to a hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from a hex string.
    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(Self(arr))
    }

    /// Get the path prefix for content-addressed storage.
    /// Returns (directory, filename) tuple.
    /// Example: hash "abcdef..." -> ("ab", "cdef...")
    pub fn to_path_parts(&self) -> (String, String) {
        let hex = self.to_hex();
        let dir = hex[..2].to_string();
        let file = hex[2..].to_string();
        (dir, file)
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", &self.to_hex()[..12])
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_hash() {
        let data = b"hello world";
        let hash1 = ContentHash::compute(data);
        let hash2 = ContentHash::compute(data);
        assert_eq!(hash1, hash2);

        let hash3 = ContentHash::compute(b"different data");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_hex_roundtrip() {
        let hash = ContentHash::compute(b"test data");
        let hex = hash.to_hex();
        let restored = ContentHash::from_hex(&hex).unwrap();
        assert_eq!(hash, restored);
    }

    #[test]
    fn test_path_parts() {
        let hash = ContentHash::compute(b"test");
        let (dir, file) = hash.to_path_parts();
        assert_eq!(dir.len(), 2);
        assert_eq!(file.len(), 62);
    }

    #[test]
    fn test_serialization() {
        let hash = ContentHash::compute(b"serialize me");
        let json = serde_json::to_string(&hash).unwrap();
        let restored: ContentHash = serde_json::from_str(&json).unwrap();
        assert_eq!(hash, restored);
    }

    #[test]
    fn test_debug_format() {
        let hash = ContentHash::compute(b"debug test");
        let debug = format!("{:?}", hash);
        assert!(debug.starts_with("ContentHash("));
        assert!(debug.len() < 30); // Should be truncated
    }
}
