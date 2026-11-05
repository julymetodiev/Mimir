//! Response types for graph commands.

use latt_types::{FileId, Language, SymbolId, SymbolKind};
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

/// A symbol's source, with any lines of context around it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSource {
    /// The symbol.
    pub symbol: SymbolInfo,
    /// The number of the first of `lines`, 1-indexed.
    pub first_line: u32,
    /// The lines.
    pub lines: Vec<String>,
    /// The symbol has lines beyond these.
    pub truncated: bool,
    /// How many symbols have the name that was asked for; this is the one it most likely means.
    pub matches: usize,
}

/// A line of text that matched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    /// The file, relative to the project.
    pub path: PathBuf,
    /// The line's number, from 1.
    pub line: u32,
    /// The line.
    pub text: String,
    /// Lines just before it.
    pub before: Vec<String>,
    /// Lines just after it.
    pub after: Vec<String>,
    /// The innermost symbol holding the line, in a language Lantern Lattice parses.
    pub symbol: Option<String>,
}

/// Lines of text that matched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepResponse {
    /// The first matches, by path and line.
    pub matches: Vec<GrepMatch>,
    /// All that match, of which `matches` may be the first few.
    pub total: usize,
    /// Files passed over for their size, so a match there is not among these.
    pub too_large: Vec<PathBuf>,
}

/// Files of a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesResponse {
    /// The first of them by path, relative to the project.
    pub files: Vec<PathBuf>,
    /// All that match, of which `files` may be the first few.
    pub total: usize,
}

/// Lines of a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileText {
    /// The file, relative to the project.
    pub path: PathBuf,
    /// The number of the first of `lines`, from 1.
    pub first_line: u32,
    /// How many lines the file has, of which `lines` may be a part.
    pub total_lines: u32,
    /// The lines.
    pub lines: Vec<String>,
}

/// What a change touches, and what depends on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactResponse {
    /// The commit the files were compared with.
    pub base: String,
    /// The symbols added or changed since it.
    pub changed: Vec<ChangedSymbol>,
    /// The symbols gone since it. A symbol moved with its text unchanged is not among them.
    pub removed: Vec<RemovedSymbol>,
    /// Files changed where no symbol did, as in imports, or in a language Lantern Lattice does not parse.
    pub other_files: Vec<PathBuf>,
    /// Analysis is still running, so dependents may be missing.
    pub provisional: bool,
}

/// A symbol added or changed, and what depends on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedSymbol {
    /// The symbol.
    pub symbol: SymbolInfo,
    /// It did not exist before.
    pub added: bool,
    /// What depends on it, each symbol once across a whole answer.
    pub dependents: Vec<TraceNode>,
}

/// A symbol gone. Nothing in the graph points at it any more, so what is known is where its name
/// is still written and means nothing else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovedSymbol {
    /// Its name.
    pub name: String,
    /// Its fullest name.
    pub qualified_name: String,
    /// Its kind.
    pub kind: SymbolKind,
    /// The file it was in, relative to the project.
    pub file_path: PathBuf,
    /// Its first line in the version compared with.
    pub line: u32,
    /// The first places that still name it and resolve to nothing else.
    pub named_at: Vec<NamedAt>,
    /// All such places, of which `named_at` may be the first few.
    pub named_total: usize,
}

/// A place that names a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedAt {
    /// The file, relative to the project.
    pub path: PathBuf,
    /// The line, from 1.
    pub line: u32,
    /// The innermost symbol holding the line.
    pub symbol: Option<String>,
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
    /// Calls a trait method this symbol implements, so it may reach this symbol at runtime.
    CallerViaTrait,
    /// Implements the trait method above it, so a call may land here at runtime.
    Implementation,
    /// Holds this function as a value, so it may call it at runtime.
    CallerByValue,
    /// Held as a value by the symbol above it, which may call it at runtime.
    CalleeByValue,
    /// Reads the constant being traced.
    Reader,
    /// Uses the type or field being traced.
    User,
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
    /// Semantic resolution is still running: files it has not reached come from syntax rules,
    /// which leave out what they cannot be sure of.
    #[serde(default)]
    pub provisional: bool,
    /// How many symbols have the name that was traced; this is the one it most likely means.
    #[serde(default)]
    pub matches: usize,
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
            provisional: false,
            matches: 1,
        }
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
    /// What a person should know about the answers, as in a sync.
    #[serde(default)]
    pub notes: Vec<String>,
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
            notes: Vec::new(),
        }
    }
}

/// Response for search queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Matching symbols.
    pub symbols: Vec<SymbolInfo>,
    /// All that match, of which `symbols` may be the first few.
    pub total_count: usize,
    /// Whether results were truncated.
    pub truncated: bool,
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
    }

    #[test]
    fn test_stats_response_empty() {
        let stats = StatsResponse::empty();
        assert_eq!(stats.files_count, 0);
        assert_eq!(stats.symbols_count, 0);
    }
}
