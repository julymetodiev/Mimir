//! Zstd compression utilities.

use mimir_error::StorageError;

/// Default compression level (3 is a good balance of speed and ratio).
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

/// Compress data using zstd.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, StorageError> {
    compress_with_level(data, DEFAULT_COMPRESSION_LEVEL)
}

/// Compress data using zstd with a specific compression level.
pub fn compress_with_level(data: &[u8], level: i32) -> Result<Vec<u8>, StorageError> {
    zstd::encode_all(data, level).map_err(|e| StorageError::Compression(e.to_string()))
}

/// Decompress zstd-compressed data.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, StorageError> {
    zstd::decode_all(data).map_err(|e| StorageError::Decompression(e.to_string()))
}

/// Check if data appears to be zstd-compressed.
pub fn is_compressed(data: &[u8]) -> bool {
    // Zstd magic number: 0x28 0xB5 0x2F 0xFD
    data.len() >= 4 && data[0] == 0x28 && data[1] == 0xB5 && data[2] == 0x2F && data[3] == 0xFD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress() {
        let original = b"Hello, World! This is some test data that should compress well when repeated. ".repeat(100);

        let compressed = compress(&original).unwrap();
        let decompressed = decompress(&compressed).unwrap();

        assert_eq!(original, decompressed.as_slice());
        assert!(compressed.len() < original.len());
    }

    #[test]
    fn test_is_compressed() {
        let data = b"uncompressed data";
        assert!(!is_compressed(data));

        let compressed = compress(data).unwrap();
        assert!(is_compressed(&compressed));
    }

    #[test]
    fn test_empty_data() {
        let empty: &[u8] = &[];
        let compressed = compress(empty).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_compression_levels() {
        let data = b"test data ".repeat(1000);

        let level1 = compress_with_level(&data, 1).unwrap();
        let level9 = compress_with_level(&data, 9).unwrap();

        // Higher level should generally produce smaller output
        // (though not guaranteed for all data)
        assert!(level9.len() <= level1.len() + 100);

        // Both should decompress to original
        assert_eq!(decompress(&level1).unwrap(), data);
        assert_eq!(decompress(&level9).unwrap(), data);
    }
}
