//! Graph client abstraction for tools to query the code graph.
//!
//! This module defines the `GraphClient` trait that allows tools to interact
//! with the Mimir code graph without depending on the daemon implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Information about a symbol in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// Symbol name.
    pub name: String,
    /// Qualified name (e.g., `module::Type::method`).
    pub qualified_name: String,
    /// Symbol kind (function, struct, etc.).
    pub kind: String,
    /// File path relative to project root.
    pub file: String,
    /// Start line (1-based).
    pub start_line: u32,
    /// End line (1-based).
    pub end_line: u32,
    /// Source code (optional, included when requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Result of a symbol search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Symbols matching the search.
    pub symbols: Vec<SymbolInfo>,
    /// Total matches found.
    pub total: usize,
}

/// Result of a symbol trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceResult {
    /// The traced symbol.
    pub symbol: SymbolInfo,
    /// Symbols that call this symbol.
    pub callers: Vec<SymbolInfo>,
    /// Symbols that this symbol calls.
    pub callees: Vec<SymbolInfo>,
}

/// Error type for graph operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphError {
    pub message: String,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GraphError {}

impl GraphError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Trait for interacting with the code graph.
///
/// This trait is implemented by the daemon client to provide graph operations
/// to tools without tight coupling to the daemon implementation.
#[async_trait]
pub trait GraphClient: Send + Sync {
    /// Search for symbols matching a pattern.
    ///
    /// # Arguments
    /// * `pattern` - Regex pattern to match symbol names
    /// * `kind` - Optional filter by symbol kind
    /// * `limit` - Maximum number of results
    async fn search(
        &self,
        pattern: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<SearchResult, GraphError>;

    /// Trace a symbol to find callers and callees.
    ///
    /// # Arguments
    /// * `symbol` - Symbol name to trace
    /// * `depth` - Maximum depth for transitive traces
    async fn trace(&self, symbol: &str, depth: u32) -> Result<TraceResult, GraphError>;

    /// Find a symbol by name.
    ///
    /// # Arguments
    /// * `name` - Symbol name to find
    async fn find_symbol(&self, name: &str) -> Result<Option<SymbolInfo>, GraphError>;

    /// Read the source code of a symbol.
    ///
    /// # Arguments
    /// * `symbol` - Symbol name to read
    async fn read_symbol(&self, symbol: &str) -> Result<Option<(SymbolInfo, String)>, GraphError>;
}
