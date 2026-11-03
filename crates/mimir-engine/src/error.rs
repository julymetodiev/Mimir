//! Errors of the graph engine.

use mimir_types::Language;
use std::path::PathBuf;
use thiserror::Error;

/// Errors from the graph actor and the handle that talks to it.
#[derive(Error, Debug)]
pub enum GraphError {
    /// The actor has stopped, so a command was not taken or no answer came back.
    #[error("The graph engine has stopped")]
    EngineStopped,

    #[error("Symbol not found: {name}")]
    SymbolNotFound { name: String },

    #[error("{name} could mean any of {places}. Say which file.")]
    AmbiguousSymbol { name: String, places: String },

    #[error("File not found in index: {path}")]
    FileNotFound { path: PathBuf },

    #[error("Failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("No language adapter for {language}")]
    NoAdapter { language: Language },

    #[error("Failed to parse file: {path}")]
    ParseFailed { path: PathBuf },

    #[error("Invalid pattern '{pattern}': {source}")]
    InvalidPattern {
        pattern: String,
        #[source]
        source: regex::Error,
    },

    #[error("Invalid path filter '{glob}': {source}")]
    InvalidGlob {
        glob: String,
        #[source]
        source: ignore::Error,
    },

    #[error("{path} is too large to read: {size} bytes")]
    FileTooLarge { path: PathBuf, size: u64 },

    #[error("{path} is not text")]
    NotText { path: PathBuf },

    #[error("Text search failed: {0}")]
    Grep(#[source] tokio::task::JoinError),

    #[error("Scan failed: {0}")]
    Scan(#[source] std::io::Error),

    #[error("Semantic indexing failed: {0}")]
    Index(#[source] tokio::task::JoinError),

    #[error("Saving the graph failed: {0}")]
    Save(#[source] std::io::Error),

    #[error("{0} is outside the project")]
    OutsideProject(PathBuf),

    #[error("This edit needs a symbol, not a file")]
    NotASymbol,

    #[error(transparent)]
    Edit(#[from] mimir_edit::EditError),
}
