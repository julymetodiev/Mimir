//! ID types for Lantern Lattice entities.
//!
//! All IDs are simple wrappers around u64 for efficient storage and comparison.

use crate::symbol::{QualifiedName, SymbolKind};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
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

define_id!(
    SymbolId,
    "Unique identifier for a symbol in the code graph."
);
define_id!(FileId, "Unique identifier for a file in the index.");
define_id!(TaskId, "Unique identifier for a task.");
define_id!(StepId, "Unique identifier for a step within a task.");
define_id!(CheckpointId, "Unique identifier for a checkpoint.");
define_id!(SnapshotId, "Unique identifier for a snapshot.");

/// The first eight bytes of a hash, as an id.
fn id_of(hasher: &blake3::Hasher) -> u64 {
    let mut bytes = [0; 8];
    hasher.finalize_xof().fill(&mut bytes);
    u64::from_le_bytes(bytes)
}

impl FileId {
    /// The id of a file, from its path relative to the project, so that it is the same in every
    /// run and after every edit.
    pub fn of_path(path: &Path) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(path.to_string_lossy().as_bytes());
        Self(id_of(&hasher))
    }
}

impl SymbolId {
    /// The id of a symbol: its file, its qualified name and kind, and `order`, its place among the
    /// symbols of that file with the same name and kind. It lasts while these do.
    pub fn lasting(file: FileId, name: &QualifiedName, kind: SymbolKind, order: u32) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&file.0.to_le_bytes());
        for segment in name.segments() {
            hasher.update(&segment.len().to_le_bytes());
            hasher.update(segment.as_bytes());
        }
        // The kind's name, so that a new kind anywhere in the enum changes no id.
        hasher.update(kind.to_string().as_bytes());
        hasher.update(&[0]);
        hasher.update(&order.to_le_bytes());
        Self(id_of(&hasher))
    }
}

/// A counter of ids, for tests that make up symbols.
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

    /// Generate the next ID.
    #[inline]
    pub fn next<T: From<u64>>(&self) -> T {
        T::from(self.next.fetch_add(1, Ordering::Relaxed))
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
    fn lasting_ids_depend_on_what_names_a_symbol() {
        let file = FileId::of_path(Path::new("src/a.rs"));
        let other = FileId::of_path(Path::new("src/b.rs"));
        assert_eq!(file, FileId::of_path(Path::new("src/a.rs")));
        assert_ne!(file, other);

        let name = QualifiedName::new(vec!["a".to_string(), "f".to_string()]);
        let id = SymbolId::lasting(file, &name, SymbolKind::Function, 0);
        assert_eq!(id, SymbolId::lasting(file, &name, SymbolKind::Function, 0));
        assert_ne!(id, SymbolId::lasting(other, &name, SymbolKind::Function, 0));
        assert_ne!(id, SymbolId::lasting(file, &name, SymbolKind::Method, 0));
        assert_ne!(id, SymbolId::lasting(file, &name, SymbolKind::Function, 1));

        // The parts of a name are told apart, not only their letters.
        let joined = QualifiedName::new(vec!["af".to_string()]);
        assert_ne!(
            id,
            SymbolId::lasting(file, &joined, SymbolKind::Function, 0)
        );
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
