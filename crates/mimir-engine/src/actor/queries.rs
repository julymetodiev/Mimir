//! Read-only queries: find, trace, read, source, stats and search.

use crate::actor::GraphActor;
use crate::error::GraphError;
use crate::source;
use mimir_graph::{TraceOptions as GraphTraceOptions, TraceRelationship};
use mimir_protocol::{
    FindResponse, SearchOptions, SearchResponse, StatsResponse, SymbolInfo, SymbolSource,
    TraceNode, TraceRelation, TraceResponse,
};
use mimir_types::{FileId, Language, Symbol, SymbolId, SymbolKind};
use std::path::PathBuf;

impl GraphActor {
    /// Handle finding symbols by name.
    pub fn handle_find_symbol(
        &self,
        name: &str,
        options: &mimir_protocol::FindOptions,
    ) -> FindResponse {
        // First try qualified name if it looks like one (contains "::")
        let results = if name.contains("::") {
            // Try exact qualified name match first
            if let Some(sym) = self.graph.find_by_qualified_name(name) {
                vec![sym]
            } else {
                // Fall back to simple name search
                self.graph.find_symbol(name)
            }
        } else {
            self.graph.find_symbol(name)
        };
        let mut symbols: Vec<SymbolInfo> = results.iter().map(|s| self.symbol_to_info(s)).collect();

        // Apply language filter
        if let Some(lang) = options.language {
            symbols.retain(|s| s.language == lang);
        }

        // Apply limit
        let truncated = if let Some(limit) = options.limit {
            if symbols.len() > limit {
                symbols.truncate(limit);
                true
            } else {
                false
            }
        } else {
            false
        };

        FindResponse {
            total: Some(symbols.len()),
            symbols,
            truncated,
        }
    }

    /// Handle tracing a symbol.
    pub fn handle_trace(
        &self,
        symbol_id: SymbolId,
        options: &mimir_protocol::TraceOptions,
    ) -> TraceResponse {
        // Get the symbol
        let Some(root_sym) = self.graph.get_symbol(symbol_id) else {
            return TraceResponse {
                root: SymbolInfo {
                    id: symbol_id,
                    name: "unknown".to_string(),
                    qualified_name: None,
                    kind: SymbolKind::Function,
                    file_id: FileId::new(0),
                    file_path: PathBuf::new(),
                    start_line: 0,
                    end_line: 0,
                    language: Language::Unknown,
                },
                callers: Vec::new(),
                callees: Vec::new(),
                max_depth_reached: 0,
                total_symbols: 0,
                provisional: self.pending.is_some(),
                matches: 0,
            };
        };

        let trace_options = GraphTraceOptions {
            max_depth: Some(options.max_depth.unwrap_or(1) as usize),
            include_callers: !options.callees_only,
            include_callees: !options.callers_only,
            cross_language: options.cross_language,
        };
        let Some(trace) = self.graph.trace(symbol_id, trace_options) else {
            return TraceResponse::new(self.symbol_to_info(&root_sym));
        };

        let callers = self.to_trace_nodes(trace.callers);
        let callees = self.to_trace_nodes(trace.callees);
        let total = 1 + callers.len() + callees.len();

        TraceResponse {
            root: self.symbol_to_info(&root_sym),
            callers,
            callees,
            max_depth_reached: trace.max_depth_reached as u32,
            total_symbols: total,
            provisional: self.pending.is_some(),
            matches: 1,
        }
    }

    fn to_trace_nodes(&self, nodes: Vec<mimir_graph::TraceNode>) -> Vec<TraceNode> {
        nodes
            .into_iter()
            .map(|node| TraceNode {
                symbol: self.symbol_to_info(&node.symbol),
                depth: node.depth as u32,
                relationship: match node.relationship {
                    TraceRelationship::Root => TraceRelation::Root,
                    TraceRelationship::Caller => TraceRelation::Caller,
                    TraceRelationship::Callee => TraceRelation::Callee,
                    TraceRelationship::CallerViaTrait => TraceRelation::CallerViaTrait,
                    TraceRelationship::Implementation => TraceRelation::Implementation,
                    TraceRelationship::CallerByValue => TraceRelation::CallerByValue,
                    TraceRelationship::CalleeByValue => TraceRelation::CalleeByValue,
                },
            })
            .collect()
    }

    /// Trace the first symbol with this name, in `options.file` if given.
    pub fn handle_trace_by_name(
        &self,
        name: &str,
        options: &mimir_protocol::TraceOptions,
    ) -> Result<TraceResponse, GraphError> {
        let mut found = self.graph.find_symbol(name);
        if let Some(file) = &options.file {
            let relative = file.strip_prefix(&self.root).unwrap_or(file);
            if !self.file_paths.values().any(|path| path == relative) {
                return Err(GraphError::FileNotFound { path: file.clone() });
            }
            found.retain(|symbol| {
                self.file_paths
                    .get(&symbol.file_id)
                    .is_some_and(|path| path == relative)
            });
        }
        let first = found.first().ok_or_else(|| GraphError::SymbolNotFound {
            name: name.to_string(),
        })?;
        let mut trace = self.handle_trace(first.id, options);
        trace.matches = found.len();
        Ok(trace)
    }

    /// The source of the symbol a name most likely means, with lines of context around it.
    pub fn handle_read(
        &self,
        name: &str,
        before: u32,
        after: u32,
    ) -> Result<SymbolSource, GraphError> {
        let found = self.graph.find_symbol(name);
        let best = source::most_likely(&found, name).ok_or_else(|| GraphError::SymbolNotFound {
            name: name.to_string(),
        })?;
        self.source_lines(best, before, after, None)
    }

    /// The first `lines` lines of a symbol, or all of them.
    pub fn handle_source(
        &self,
        symbol_id: SymbolId,
        lines: Option<u32>,
    ) -> Result<SymbolSource, GraphError> {
        let symbol =
            self.graph
                .get_symbol(symbol_id)
                .ok_or_else(|| GraphError::SymbolNotFound {
                    name: symbol_id.to_string(),
                })?;
        self.source_lines(&symbol, 0, 0, lines)
    }

    fn source_lines(
        &self,
        symbol: &Symbol,
        before: u32,
        after: u32,
        limit: Option<u32>,
    ) -> Result<SymbolSource, GraphError> {
        let file = self
            .sources
            .get(&symbol.file_id)
            .ok_or_else(|| GraphError::FileNotFound {
                path: self
                    .file_paths
                    .get(&symbol.file_id)
                    .cloned()
                    .unwrap_or_default(),
            })?;
        let (first_line, lines, truncated) = source::cut(
            &file.source,
            symbol.range.start_line,
            symbol.range.end_line,
            before,
            after,
            limit,
        );
        Ok(SymbolSource {
            symbol: self.symbol_to_info(symbol),
            first_line,
            lines,
            truncated,
        })
    }

    /// Handle getting statistics.
    pub fn handle_stats(&self) -> StatsResponse {
        StatsResponse {
            files_count: self.files.len(),
            symbols_count: self.graph.symbol_count(),
            references_count: self.graph.edge_count(),
            files_by_language: Vec::new(),
            symbols_by_kind: Vec::new(),
        }
    }

    /// Handle searching for symbols by pattern.
    pub fn handle_search(&self, options: &SearchOptions) -> Result<SearchResponse, GraphError> {
        use regex::Regex;

        let pattern =
            Regex::new(&options.pattern).map_err(|source| GraphError::InvalidPattern {
                pattern: options.pattern.clone(),
                source,
            })?;

        let limit = options.limit.unwrap_or(20) as usize;
        let mut matches = Vec::new();

        // Search all symbols in the graph
        for symbol in self.graph.symbols().all_symbols() {
            if pattern.is_match(&symbol.name)
                || pattern.is_match(&symbol.qualified_name.to_string())
            {
                // Apply kind filter if specified
                if let Some(ref kind_filter) = options.kind_filter
                    && !kind_matches(symbol.kind, kind_filter)
                {
                    continue;
                }

                matches.push(self.symbol_to_info(&symbol));

                if matches.len() >= limit {
                    break;
                }
            }
        }

        let truncated = matches.len() >= limit;
        let total_count = matches.len();

        Ok(SearchResponse {
            symbols: matches,
            total_count,
            truncated,
        })
    }
}

/// Whether a kind is what a filter asks for: part of its short name, or its full name.
pub fn kind_matches(kind: SymbolKind, filter: &str) -> bool {
    let filter = filter.to_lowercase();
    let full = match kind {
        SymbolKind::Constant => "constant",
        SymbolKind::Variable => "variable",
        SymbolKind::Parameter => "parameter",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::TypeParameter => "type_parameter",
        SymbolKind::EnumVariant => "enum_variant",
        _ => "",
    };
    let short = kind.to_string();
    short.contains(&filter) || full == filter
}
