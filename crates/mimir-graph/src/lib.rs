//! The code graph, in the format of the file that keeps it on disk: docs/GRAPH-STORE.md.

pub mod relations;

mod encode;
mod facade;
mod file;
mod format;
mod references;
mod trace;

pub use encode::encode;
pub use facade::CodeGraph;
pub use file::{FileEntry, FileError, GraphFile};
pub use references::{ReferencesFile, encode_references};
pub use relations::{EdgeData, EdgeKind};
pub use trace::{TraceNode, TraceOptions, TraceRelationship, TraceResult};
