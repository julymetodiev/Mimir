//! Response types for graph commands.

use mimir_types::{FileId, Language, SymbolId, SymbolKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A symbol in a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// Unique symbol ID.
    pub id: SymbolId,
    /// Symbol name.
    pub name: String,
    /// Fully qualified name.
    pub qualified_name: Option<String>,
    /// Symbol kind.
    pub kind: SymbolKind,
    /// File containing this symbol.
    pub file_id: FileId,
    /// File path.
    pub file_path: PathBuf,
    /// Start line (1-indexed).
    pub start_line: u32,
    /// End line (1-indexed).
    pub end_line: u32,
    /// Language.
    pub language: Language,
}

/// Response for find symbol queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindResponse {
    /// Matching symbols.
    pub symbols: Vec<SymbolInfo>,
    /// Whether results were truncated.
    pub truncated: bool,
    /// Total matches (if known).
    pub total: Option<usize>,
}

impl FindResponse {
    /// Create an empty response.
    pub fn empty() -> Self {
        Self {
            symbols: Vec::new(),
            truncated: false,
            total: Some(0),
        }
    }

    /// Check if any results were found.
    pub fn has_results(&self) -> bool {
        !self.symbols.is_empty()
    }
}

/// A node in a trace result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNode {
    /// The symbol at this node.
    pub symbol: SymbolInfo,
    /// Depth from the root symbol.
    pub depth: u32,
    /// Relationship type.
    pub relationship: TraceRelation,
}

/// Relationship in a trace.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraceRelation {
    /// This is the root symbol being traced.
    Root,
    /// This symbol calls the parent.
    Caller,
    /// This symbol is called by the parent.
    Callee,
    /// Cross-language reference.
    CrossLanguage,
}

/// Response for trace queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceResponse {
    /// The root symbol being traced.
    pub root: SymbolInfo,
    /// Caller chain.
    pub callers: Vec<TraceNode>,
    /// Callee chain.
    pub callees: Vec<TraceNode>,
    /// Maximum depth reached.
    pub max_depth_reached: u32,
    /// Total symbols in trace.
    pub total_symbols: usize,
}

impl TraceResponse {
    /// Create a new trace response with just the root.
    pub fn new(root: SymbolInfo) -> Self {
        Self {
            root,
            callers: Vec::new(),
            callees: Vec::new(),
            max_depth_reached: 0,
            total_symbols: 1,
        }
    }

    /// Check if the trace found any relationships.
    pub fn has_relationships(&self) -> bool {
        !self.callers.is_empty() || !self.callees.is_empty()
    }
}

/// Response for stats queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    /// Total number of files indexed.
    pub files_count: usize,
    /// Total number of symbols.
    pub symbols_count: usize,
    /// Total number of references.
    pub references_count: usize,
    /// Files by language.
    pub files_by_language: Vec<(Language, usize)>,
    /// Symbols by kind.
    pub symbols_by_kind: Vec<(SymbolKind, usize)>,
}

impl StatsResponse {
    /// Create an empty stats response.
    pub fn empty() -> Self {
        Self {
            files_count: 0,
            symbols_count: 0,
            references_count: 0,
            files_by_language: Vec::new(),
            symbols_by_kind: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_symbol() -> SymbolInfo {
        SymbolInfo {
            id: SymbolId::new(1),
            name: "test_func".to_string(),
            qualified_name: Some("crate::module::test_func".to_string()),
            kind: SymbolKind::Function,
            file_id: FileId::new(1),
            file_path: PathBuf::from("src/lib.rs"),
            start_line: 10,
            end_line: 20,
            language: Language::Rust,
        }
    }

    #[test]
    fn test_find_response_empty() {
        let response = FindResponse::empty();
        assert!(!response.has_results());
        assert_eq!(response.total, Some(0));
    }

    #[test]
    fn test_find_response_with_results() {
        let response = FindResponse {
            symbols: vec![make_test_symbol()],
            truncated: false,
            total: Some(1),
        };
        assert!(response.has_results());
    }

    #[test]
    fn test_trace_response_new() {
        let root = make_test_symbol();
        let response = TraceResponse::new(root);
        assert_eq!(response.total_symbols, 1);
        assert!(!response.has_relationships());
    }

    #[test]
    fn test_trace_response_with_relationships() {
        let root = make_test_symbol();
        let mut response = TraceResponse::new(root.clone());
        response.callers.push(TraceNode {
            symbol: make_test_symbol(),
            depth: 1,
            relationship: TraceRelation::Caller,
        });
        assert!(response.has_relationships());
    }

    #[test]
    fn test_stats_response_empty() {
        let stats = StatsResponse::empty();
        assert_eq!(stats.files_count, 0);
        assert_eq!(stats.symbols_count, 0);
    }
}
