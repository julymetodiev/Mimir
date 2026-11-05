//! Content hash type for content-addressed storage.
//!
//! Uses Blake3 for fast, cryptographically secure hashing.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A 32-byte Blake3 hash of content.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Compute the hash of the given data.
    pub fn compute(data: &[u8]) -> Self {
        Self(*blake3::hash(data).as_bytes())
    }

    /// A hash from its bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The hash's bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to a hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Read what [`Self::to_hex`] wrote.
    pub fn from_hex(text: &str) -> Option<Self> {
        Some(Self(hex::decode(text).ok()?.try_into().ok()?))
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
