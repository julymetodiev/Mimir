//! Serialization utilities for the Mimir semantic code intelligence engine.
//!
//! Provides bincode-based serialization for efficient binary storage.

use mimir_error::{MimirError, StorageError};
use serde::{Deserialize, Serialize};

/// Serialize a value to bytes using bincode.
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, MimirError> {
    bincode::serde::encode_to_vec(value, bincode::config::standard())
        .map_err(|e| StorageError::Serialization(e.to_string()).into())
}

/// Deserialize bytes to a value using bincode.
pub fn deserialize<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, MimirError> {
    bincode::serde::decode_from_slice(bytes, bincode::config::standard())
        .map(|(value, _)| value)
        .map_err(|e| StorageError::Serialization(e.to_string()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let original = vec![1u32, 2, 3, 4, 5];
        let bytes = serialize(&original).unwrap();
        let restored: Vec<u32> = deserialize(&bytes).unwrap();
        assert_eq!(original, restored);
    }
}
