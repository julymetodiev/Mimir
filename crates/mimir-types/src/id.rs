//! ID types for Mimir entities.
//!
//! All IDs are simple wrappers around u64 for efficient storage and comparison.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Macro to define ID types with consistent implementation.
macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Create a new ID with the given value.
            #[inline]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Get the underlying value.
            #[inline]
            pub const fn value(&self) -> u64 {
                self.0
            }

            /// Alias for value() - get as u64.
            #[inline]
            pub const fn as_u64(&self) -> u64 {
                self.0
            }

            /// Create from u64.
            #[inline]
            pub const fn from_u64(value: u64) -> Self {
                Self(value)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<u64> for $name {
            fn from(value: u64) -> Self {
                Self(value)
            }
        }

        impl From<$name> for u64 {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

define_id!(SymbolId, "Unique identifier for a symbol in the code graph.");
define_id!(FileId, "Unique identifier for a file in the index.");
define_id!(TaskId, "Unique identifier for a task.");
define_id!(StepId, "Unique identifier for a step within a task.");
define_id!(CheckpointId, "Unique identifier for a checkpoint.");
define_id!(SnapshotId, "Unique identifier for a snapshot.");

/// Thread-safe ID generator.
pub struct IdGenerator {
    next: AtomicU64,
}

impl IdGenerator {
    /// Create a new ID generator starting from 1.
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Create a new ID generator starting from a specific value.
    pub const fn starting_from(start: u64) -> Self {
        Self {
            next: AtomicU64::new(start),
        }
    }

    /// Generate the next ID.
    #[inline]
    pub fn next<T: From<u64>>(&self) -> T {
        T::from(self.next.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the current value without incrementing.
    #[inline]
    pub fn current(&self) -> u64 {
        self.next.load(Ordering::Relaxed)
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_creation() {
        let id = SymbolId::new(42);
        assert_eq!(id.value(), 42);
    }

    #[test]
    fn test_id_display() {
        let id = FileId::new(123);
        assert_eq!(format!("{}", id), "123");
        assert_eq!(format!("{:?}", id), "FileId(123)");
    }

    #[test]
    fn test_id_serialization() {
        let id = TaskId::new(999);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "999");
        let restored: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, restored);
    }

    #[test]
    fn test_id_generator() {
        let generator = IdGenerator::new();
        let id1: SymbolId = generator.next();
        let id2: SymbolId = generator.next();
        let id3: FileId = generator.next();

        assert_eq!(id1.value(), 1);
        assert_eq!(id2.value(), 2);
        assert_eq!(id3.value(), 3);
    }

    #[test]
    fn test_id_equality() {
        let id1 = SymbolId::new(42);
        let id2 = SymbolId::new(42);
        let id3 = SymbolId::new(43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
