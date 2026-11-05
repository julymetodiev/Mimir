//! Read-only queries: find, trace, read, source, stats and search.

use crate::actor::GraphActor;
use crate::error::GraphError;
use crate::{grep, source};
use latt_graph::{TraceOptions as GraphTraceOptions, TraceRelationship};
use latt_protocol::{
    ActivityKind, FindResponse, GrepOptions, GrepResponse, SearchOptions, SearchResponse,
    StatsResponse, SymbolInfo, SymbolSource, TraceNode, TraceRelation, TraceResponse,
};
use latt_resolve::type_name;
use latt_types::{FileId, Language, Symbol, SymbolId, SymbolKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How many matches of a text search come back unless asked otherwise.
const DEFAULT_GREP_LIMIT: u32 = 50;

/// Whether a qualified name ends as `name` is written, an `impl` counting as its type or its trait.
pub fn ends_with(segments: &[String], name: &str) -> bool {
    let wanted: Vec<&str> = name.split("::").collect();
    wanted.len() <= segments.len()
        && segments
            .iter()
            .rev()
            .zip(wanted.iter().rev())
            .all(|(segment, wanted)| {
                segment == wanted
                    || segment.strip_prefix("impl ").is_some_and(|header| {
                        header.split(" for ").any(|part| type_name(part) == *wanted)
                    })
            })
}

impl GraphActor {
    /// Handle finding symbols by name.
    pub fn handle_find_symbol(
        &self,
        name: &str,
        options: &latt_protocol::FindOptions,
    ) -> FindResponse {
        // First try qualified name if it looks like one (contains "::")
        let results = if name.contains("::") {
            // Try exact qualified name match first
            let exact = self.graph.find_by_qualified_name(name);
            if exact.is_empty() {
                // Fall back to simple name search
                self.graph.find_symbol(name)
            } else {
                exact
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
        options: &latt_protocol::TraceOptions,
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
        };
        let Some(trace) = self.graph.trace(symbol_id, trace_options, &self.file_paths) else {
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

    pub fn to_trace_nodes(&self, nodes: Vec<latt_graph::TraceNode>) -> Vec<TraceNode> {
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
                    TraceRelationship::Reader => TraceRelation::Reader,
                    TraceRelationship::User => TraceRelation::User,
                },
            })
            .collect()
    }

    /// Trace the symbol this name most likely means, in `options.file` if given.
    pub fn handle_trace_by_name(
        &self,
        name: &str,
        options: &latt_protocol::TraceOptions,
    ) -> Result<TraceResponse, GraphError> {
        let found = self.named(name, options.file.as_deref())?;
        let best = source::most_likely(&found, name, &self.file_paths, |symbol| {
            self.graph.callers(symbol.id).len()
        })
        .ok_or_else(|| GraphError::SymbolNotFound {
            name: name.to_string(),
        })?;
        let mut trace = self.handle_trace(best.id, options);
        trace.matches = found.len();
        Ok(trace)
    }

    /// The symbols with this name, only those in `file` if given.
    pub fn named(&self, name: &str, file: Option<&Path>) -> Result<Vec<Arc<Symbol>>, GraphError> {
        // A qualified name, as in `GraphCommand::Read`, narrows it down. Other files may hold the
        // same one, as a class of the same name, so `file` must still have them all to choose from.
        let mut found = self.graph.find_symbol(name);
        if let Some((_, simple)) = name.rsplit_once("::") {
            found.extend(
                self.graph
                    .find_symbol(simple)
                    .into_iter()
                    .filter(|symbol| symbol.qualified_name.to_string() == name),
            );
            if found.is_empty() {
                found.extend(self.graph.find_by_qualified_name(name));
            }
            // Written as far as it tells them apart: `Known::add` for `impl Known<'a>::add`.
            if found.is_empty() {
                found.extend(
                    self.graph
                        .find_symbol(simple)
                        .into_iter()
                        .filter(|symbol| ends_with(symbol.qualified_name.segments(), name)),
                );
            }
        }
        if let Some(file) = file {
            let absolute = self.root.join(file);
            let relative = absolute.strip_prefix(&self.root).unwrap_or(&absolute);
            if !self.file_paths.values().any(|path| path == relative) {
                return Err(GraphError::FileNotFound {
                    path: file.to_path_buf(),
                });
            }
            found.retain(|symbol| {
                self.file_paths
                    .get(&symbol.file_id)
                    .is_some_and(|path| path == relative)
            });
        }
        Ok(found)
    }

    /// The source of the symbol a name most likely means, with lines of context around it.
    pub fn handle_read(
        &self,
        name: &str,
        file: Option<&Path>,
        before: u32,
        after: u32,
    ) -> Result<SymbolSource, GraphError> {
        let found = self.named(name, file)?;
        let best = source::most_likely(&found, name, &self.file_paths, |symbol| {
            self.graph.callers(symbol.id).len()
        })
        .ok_or_else(|| GraphError::SymbolNotFound {
            name: name.to_string(),
        })?;
        let mut source = self.source_lines(best, before, after, None)?;
        source.matches = found.len();
        self.announce_activity(vec![best.id], ActivityKind::Read);
        Ok(source)
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
        let text = self
            .text_of(symbol.file_id)
            .ok_or_else(|| GraphError::FileNotFound {
                path: self
                    .file_paths
                    .get(&symbol.file_id)
                    .cloned()
                    .unwrap_or_default(),
            })?;
        let (first_line, lines, truncated) = source::cut(
            &text,
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
            matches: 1,
        })
    }

    /// Search the text of the project's files and say which symbol each match is in. The files are
    /// read off the actor, but it waits: a question after this one sees the same project.
    pub async fn handle_grep(&self, options: GrepOptions) -> Result<GrepResponse, GraphError> {
        let limit = options.limit.unwrap_or(DEFAULT_GREP_LIMIT) as usize;
        let root = self.root.clone();
        let (mut matches, too_large) =
            tokio::task::spawn_blocking(move || grep::search(&root, &options))
                .await
                .map_err(GraphError::Grep)??;
        let total = matches.len();
        matches.truncate(limit);

        let ids: HashMap<&Path, FileId> = self
            .file_paths
            .iter()
            .map(|(id, path)| (path.as_path(), *id))
            .collect();
        for found in &mut matches {
            if let Some(id) = ids.get(found.path.as_path()) {
                found.symbol = self.symbol_at(*id, found.line);
            }
        }
        Ok(GrepResponse {
            matches,
            total,
            too_large,
        })
    }

    /// The fullest name of the innermost symbol holding a line.
    pub fn symbol_at(&self, file: FileId, line: u32) -> Option<String> {
        self.graph
            .symbols_in_file(file)
            .iter()
            .filter(|symbol| symbol.range.start_line <= line && line <= symbol.range.end_line)
            .min_by_key(|symbol| symbol.range.end_line - symbol.range.start_line)
            .map(|symbol| symbol.qualified_name.to_string())
    }

    /// Handle getting statistics.
    pub fn handle_stats(&self) -> StatsResponse {
        StatsResponse {
            files_count: self.files.len(),
            symbols_count: self.graph.symbol_count(),
            references_count: self.graph.edge_count(),
            files_by_language: Vec::new(),
            symbols_by_kind: Vec::new(),
            notes: self.semantics.notes(&self.root),
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
        let mut matches: Vec<SymbolInfo> = self
            .graph
            .all_symbols()
            .iter()
            .filter(|symbol| {
                pattern.is_match(&symbol.name)
                    || pattern.is_match(&symbol.qualified_name.to_string())
            })
            .filter(|symbol| {
                options
                    .kind_filter
                    .as_ref()
                    .is_none_or(|filter| kind_matches(symbol.kind, filter))
            })
            .map(|symbol| self.symbol_to_info(symbol))
            .collect();
        // The graph holds symbols in no order, and the first few must be the same ones each time.
        matches.sort_by(|a, b| (&a.file_path, a.start_line).cmp(&(&b.file_path, b.start_line)));

        let total_count = matches.len();
        matches.truncate(limit);
        Ok(SearchResponse {
            truncated: matches.len() < total_count,
            symbols: matches,
            total_count,
        })
    }
}

/// Whether a kind is what a filter asks for: part of its short name, or its full name. A method is
/// a function too: someone asking for functions named `sync` means those as well.
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
    let is_a_function = matches!(kind, SymbolKind::Method | SymbolKind::Constructor);
    short.contains(&filter) || full == filter || (is_a_function && filter == "function")
}
