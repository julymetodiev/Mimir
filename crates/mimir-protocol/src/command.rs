//! Options and results of graph operations.

use mimir_types::Language;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Options for finding symbols.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindOptions {
    /// Filter by file path pattern.
    pub file_pattern: Option<String>,
    /// Filter by language.
    pub language: Option<Language>,
    /// Maximum results to return.
    pub limit: Option<usize>,
    /// Include symbols from dependencies.
    pub include_deps: bool,
}

/// Options for tracing symbol relationships.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceOptions {
    /// Maximum depth for trace traversal.
    pub max_depth: Option<u32>,
    /// Only trace callers (who calls this).
    pub callers_only: bool,
    /// Only trace callees (what this calls).
    pub callees_only: bool,
    /// Include cross-language references.
    pub cross_language: bool,
    /// Only a symbol in this file, by absolute path.
    #[serde(default)]
    pub file: Option<PathBuf>,
}

/// Options for searching symbols by pattern.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Regex pattern to match symbol names.
    pub pattern: String,
    /// Filter by symbol kind.
    pub kind_filter: Option<String>,
    /// Maximum results to return.
    pub limit: Option<u32>,
}

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Number of files added.
    pub files_added: usize,
    /// Number of files updated.
    pub files_updated: usize,
    /// Number of files removed.
    pub files_removed: usize,
    /// Number of symbols added.
    pub symbols_added: usize,
    /// Number of symbols removed.
    pub symbols_removed: usize,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// How edges were resolved, when the sync waited for that.
    #[serde(default)]
    pub link: Option<LinkSummary>,
    /// Where the graph was saved, when it was.
    #[serde(default)]
    pub saved_to: Option<PathBuf>,
}

/// How edges were resolved.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct LinkSummary {
    /// Call edges from a semantic index.
    pub semantic_calls: usize,
    /// Call edges from syntax rules.
    pub syntax_calls: usize,
    /// Edges from a method of an impl to the method of its trait.
    pub implementations: usize,
    /// Edges from a symbol to a function it names without calling.
    pub function_values: usize,
    /// Import edges.
    pub imports: usize,
    /// Files covered by a semantic index.
    pub semantic_files: usize,
    /// Of those, files edited since the index, resolved by a running analyzer.
    pub live_files: usize,
    /// Files resolved by syntax rules only.
    pub syntax_files: usize,
}

impl SyncResult {
    /// Create a new empty sync result.
    pub fn empty() -> Self {
        Self {
            files_added: 0,
            files_updated: 0,
            files_removed: 0,
            symbols_added: 0,
            symbols_removed: 0,
            duration_ms: 0,
            link: None,
            saved_to: None,
        }
    }

    /// Check if any changes occurred.
    pub fn has_changes(&self) -> bool {
        self.files_added > 0
            || self.files_updated > 0
            || self.files_removed > 0
            || self.symbols_added > 0
            || self.symbols_removed > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_options_default() {
        let opts = FindOptions::default();
        assert!(opts.file_pattern.is_none());
        assert!(opts.language.is_none());
        assert!(opts.limit.is_none());
        assert!(!opts.include_deps);
    }

    #[test]
    fn test_trace_options_default() {
        let opts = TraceOptions::default();
        assert!(opts.max_depth.is_none());
        assert!(!opts.callers_only);
        assert!(!opts.callees_only);
        assert!(!opts.cross_language);
    }

    #[test]
    fn test_sync_result_empty() {
        let result = SyncResult::empty();
        assert!(!result.has_changes());
    }

    #[test]
    fn test_sync_result_has_changes() {
        let mut result = SyncResult::empty();
        result.files_added = 1;
        assert!(result.has_changes());
    }
}
