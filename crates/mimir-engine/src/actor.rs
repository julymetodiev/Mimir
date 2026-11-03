//! Graph actor implementation.

use mimir_error::GraphError;
use mimir_graph::{CodeGraph, TraceOptions as GraphTraceOptions, TraceRelationship};
use mimir_parse_core::AdapterRegistry;
use mimir_protocol::{
    FindResponse, LinkSummary, SearchOptions, SearchResponse, StatsResponse, SymbolInfo,
    SymbolSource, SyncResult, TraceNode, TraceRelation, TraceResponse,
};
use mimir_resolve::{ProjectFile, Refresh, ScipIndex, SemanticSource, link};
use mimir_types::{ContentHash, FileId, IdGenerator, Language, Symbol, SymbolId, SymbolKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::channel::{CommandReceiver, EventSender};
use crate::command::GraphCommand;
use crate::event::GraphEvent;
use crate::scanner::{ScannedFile, Scanner};
use crate::{save, source};

/// How long a running analyzer, which holds gigabytes, is kept for the next edit.
const ANALYZER_IDLE: Duration = Duration::from_secs(600);

/// How long a query waits for semantic resolution. A running analyzer is usually done by then;
/// a new index or a first load is not, and the query goes on with syntax rules.
const QUERY_WAIT: Duration = Duration::from_millis(500);

/// Semantic resolution running in the background, for a set of files.
struct PendingLink {
    indexes: tokio::task::JoinHandle<Vec<ScipIndex>>,
    files: Vec<ProjectFile>,
    /// The graph already holds what syntax rules give for these files.
    linked_by_syntax: bool,
}

/// Tracked file info.
#[derive(Clone)]
struct TrackedFile {
    file_id: FileId,
    content_hash: ContentHash,
    #[allow(dead_code)] // May be used for language-specific operations
    language: Language,
}

/// The graph actor owns the graph and processes commands.
pub struct GraphActor {
    /// The code graph.
    graph: CodeGraph,
    /// Language adapter registry.
    registry: Arc<AdapterRegistry>,
    /// Project root path.
    root: PathBuf,
    /// ID generator for symbols and files.
    ids: IdGenerator,
    /// Tracked files by path.
    files: HashMap<PathBuf, TrackedFile>,
    /// Reverse mapping: file ID to path.
    file_paths: HashMap<FileId, PathBuf>,
    /// What was extracted from each file, from which all edges are resolved.
    sources: HashMap<FileId, ProjectFile>,
    /// A file changed since edges were last resolved.
    needs_link: bool,
    /// What knows more than syntax about the files.
    semantics: Arc<dyn SemanticSource>,
    pending: Option<PendingLink>,
    query_wait: Duration,
}

/// Send a reply. An asker that went away no longer wants it, which is no failure.
fn answer<T>(reply: oneshot::Sender<T>, value: T) {
    if reply.send(value).is_err() {
        debug!("The asker left before the answer");
    }
}

/// The next command. Waiting for it is also when idle analyzers stop.
async fn next_command(
    commands: &mut CommandReceiver,
    semantics: &dyn SemanticSource,
) -> Option<GraphCommand> {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            command = commands.recv() => return command,
            _ = tick.tick() => semantics.stop_if_idle(ANALYZER_IDLE),
        }
    }
}

impl GraphActor {
    /// Create a new graph actor.
    pub fn new(root: PathBuf, semantics: Arc<dyn SemanticSource>) -> Self {
        Self {
            graph: CodeGraph::new(),
            registry: Arc::new(AdapterRegistry::new()),
            root,
            ids: IdGenerator::new(),
            files: HashMap::new(),
            file_paths: HashMap::new(),
            sources: HashMap::new(),
            needs_link: false,
            semantics,
            pending: None,
            query_wait: QUERY_WAIT,
        }
    }

    /// Create with a custom adapter registry.
    pub fn with_registry(
        root: PathBuf,
        registry: Arc<AdapterRegistry>,
        semantics: Arc<dyn SemanticSource>,
    ) -> Self {
        Self {
            graph: CodeGraph::new(),
            registry,
            root,
            ids: IdGenerator::new(),
            files: HashMap::new(),
            file_paths: HashMap::new(),
            sources: HashMap::new(),
            needs_link: false,
            semantics,
            pending: None,
            query_wait: QUERY_WAIT,
        }
    }

    /// Set how long a query waits for semantic resolution.
    pub fn with_query_wait(mut self, wait: Duration) -> Self {
        self.query_wait = wait;
        self
    }

    /// Run the actor's command processing loop.
    pub async fn run(
        mut self,
        mut commands: CommandReceiver,
        events: EventSender,
    ) -> Result<(), GraphError> {
        info!("Graph actor starting for root: {}", self.root.display());

        while let Some(cmd) = next_command(&mut commands, self.semantics.as_ref()).await {
            match cmd {
                GraphCommand::AddFile {
                    path,
                    content,
                    language,
                    reply,
                } => {
                    let result = self.handle_add_file(path.clone(), content, language, &events);
                    answer(reply, result);
                }

                GraphCommand::RemoveFile { file_id, reply } => {
                    let result = self.handle_remove_file(file_id, &events);
                    answer(reply, result);
                }

                GraphCommand::FindSymbol {
                    name,
                    options,
                    reply,
                } => {
                    let response = self.handle_find_symbol(&name, &options);
                    answer(reply, response);
                }

                GraphCommand::Trace {
                    symbol_id,
                    options,
                    reply,
                } => {
                    self.ensure_linked().await;
                    let response = self.handle_trace(symbol_id, &options);
                    answer(reply, response);
                }

                GraphCommand::TraceByName {
                    name,
                    options,
                    reply,
                } => {
                    self.ensure_linked().await;
                    let response = self.handle_trace_by_name(&name, &options);
                    answer(reply, response);
                }

                GraphCommand::Read {
                    name,
                    before,
                    after,
                    reply,
                } => {
                    let response = self.handle_read(&name, before, after);
                    answer(reply, response);
                }

                GraphCommand::Source {
                    symbol_id,
                    lines,
                    reply,
                } => {
                    let response = self.handle_source(symbol_id, lines);
                    answer(reply, response);
                }

                GraphCommand::SyncComplete { full, reply } => {
                    let result = self.handle_sync_complete(full, &events).await;
                    answer(reply, result);
                }

                GraphCommand::Stats { reply } => {
                    self.ensure_linked().await;
                    let response = self.handle_stats();
                    answer(reply, response);
                }

                GraphCommand::Sync { reply } => {
                    let result = self.handle_sync(&events).await;
                    answer(reply, result);
                }

                GraphCommand::Search { options, reply } => {
                    let response = self.handle_search(&options);
                    answer(reply, response);
                }

                GraphCommand::Shutdown => {
                    info!("Graph actor shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle adding a file to the graph.
    fn handle_add_file(
        &mut self,
        path: PathBuf,
        content: String,
        language: Language,
        events: &EventSender,
    ) -> Result<FileId, GraphError> {
        debug!("Adding file: {}", path.display());

        // If file already exists, remove old version first (for updates)
        if let Some(tracked) = self.files.get(&path).cloned() {
            debug!(
                "File already exists, removing old version: {:?}",
                tracked.file_id
            );
            let _ = self.handle_remove_file(tracked.file_id, events);
        }

        // Store reverse mapping for file path lookup
        let relative_path = path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf();
        let content_hash = ContentHash::compute(content.as_bytes());
        let (file_id, _) =
            self.insert_file(path, relative_path, content, content_hash, language, events)?;

        self.needs_link = true;

        Ok(file_id)
    }

    /// Parse a file and add it and its symbols to the graph. Edges come from `relink`.
    fn insert_file(
        &mut self,
        path: PathBuf,
        relative_path: PathBuf,
        content: String,
        content_hash: ContentHash,
        language: Language,
        events: &EventSender,
    ) -> Result<(FileId, usize), GraphError> {
        // Get adapter for this language
        let adapter = self
            .registry
            .get_by_language(language)
            .ok_or_else(|| GraphError::FileNotFound { path: path.clone() })?;

        // Parse the file
        let tree = adapter
            .parse(&content)
            .ok_or_else(|| GraphError::FileNotFound { path: path.clone() })?;

        // Assign file ID
        let file_id: FileId = self.ids.next();

        // Track the file
        self.files.insert(
            path.clone(),
            TrackedFile {
                file_id,
                content_hash,
                language,
            },
        );
        self.file_paths.insert(file_id, relative_path.clone());

        // Extract symbols
        let extracted_symbols = adapter.extract_symbols(&tree, file_id);
        let references = adapter.extract_references(&tree, file_id);
        self.sources.insert(
            file_id,
            ProjectFile {
                file_id,
                path: relative_path,
                source: content.as_str().into(),
                references,
            },
        );

        // Convert ExtractedSymbol to Symbol and add to graph
        for extracted in &extracted_symbols {
            let symbol = Symbol {
                id: self.ids.next(),
                name: extracted.name.clone(),
                qualified_name: extracted.qualified_name.clone(),
                kind: extracted.kind,
                language: extracted.language,
                file_id,
                range: extracted.range,
                span: extracted.span,
                signature: extracted.signature.clone(),
                visibility: extracted.visibility,
                doc: extracted.doc.clone(),
            };

            self.graph.add_symbol(symbol);
        }

        let symbols_count = extracted_symbols.len();

        // Emit event
        let _ = events.send(GraphEvent::FileAdded {
            file_id,
            path,
            symbols_count,
        });

        Ok((file_id, symbols_count))
    }

    /// Resolve every edge if a file changed since the last time. A query that reads edges
    /// waits for this, so several edits in a row cost one index and no answer is stale.
    /// Bring edges up to date, waiting only briefly: if semantic resolution takes longer, it
    /// goes on in the background and the graph has what syntax rules give until it is done.
    /// Later queries pick up its result.
    async fn ensure_linked(&mut self) {
        let cache_dir = self.root.join(".mimir").join("graph");
        if self.needs_link {
            self.needs_link = false;
            let files: Vec<ProjectFile> = self.sources.values().cloned().collect();
            let (root, cache_dir, scan) = (self.root.clone(), cache_dir.clone(), files.clone());
            let semantics = self.semantics.clone();
            // Replacing an unfinished one leaves it to end alone; this one waits for it.
            let indexes = tokio::task::spawn_blocking(move || {
                semantics.load(&root, &cache_dir, &scan, Refresh::Edits)
            });
            self.pending = Some(PendingLink {
                indexes,
                files,
                linked_by_syntax: false,
            });
        }
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        let wait = if pending.linked_by_syntax {
            Duration::ZERO
        } else {
            self.query_wait
        };
        match tokio::time::timeout(wait, &mut pending.indexes).await {
            Ok(indexes) => {
                let indexes = indexes.unwrap_or_else(|e| {
                    warn!("Semantic indexing failed: {}", e);
                    Vec::new()
                });
                let report = link(&self.graph, &pending.files, &indexes);
                debug!("Linked: {:?}", report);
                self.pending = None;
            }
            Err(_) if pending.linked_by_syntax => {}
            Err(_) => {
                let (root, scan) = (self.root.clone(), pending.files.clone());
                let semantics = self.semantics.clone();
                let saved = tokio::task::spawn_blocking(move || {
                    semantics.load(&root, &cache_dir, &scan, Refresh::Saved)
                })
                .await
                .unwrap_or_default();
                let report = link(&self.graph, &pending.files, &saved);
                debug!("Linked for now: {:?}", report);
                pending.linked_by_syntax = true;
            }
        }
    }

    /// Scan, resolve every edge without settling for a provisional answer, and save the graph.
    async fn handle_sync_complete(
        &mut self,
        full: bool,
        events: &EventSender,
    ) -> Result<SyncResult, GraphError> {
        let mut result = self.scan_files(events).await?;
        let started = Instant::now();

        // A link still running gives way: this one covers the same files or newer ones.
        self.pending = None;
        self.needs_link = false;
        let files: Vec<ProjectFile> = self.sources.values().cloned().collect();
        let (root, cache_dir, scan) = (self.root.clone(), self.graph_dir(), files.clone());
        let semantics = self.semantics.clone();
        let refresh = if full { Refresh::Full } else { Refresh::Edits };
        let indexes =
            tokio::task::spawn_blocking(move || semantics.load(&root, &cache_dir, &scan, refresh))
                .await
                .map_err(|e| GraphError::WatcherError(format!("Semantic indexing failed: {e}")))?;
        let report = link(&self.graph, &files, &indexes);

        let graph_file = save::save(&self.graph, &self.file_paths, &self.graph_dir())
            .map_err(|e| GraphError::WatcherError(format!("Saving the graph failed: {e}")))?;

        result.link = Some(LinkSummary {
            semantic_calls: report.semantic_calls,
            syntax_calls: report.syntax_calls,
            implementations: report.implementations,
            function_values: report.function_values,
            imports: report.imports,
            semantic_files: report.semantic_files,
            live_files: report.live_files,
            syntax_files: report.syntax_files,
        });
        result.saved_to = Some(graph_file);
        result.duration_ms += started.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Where the index cache and the saved graph live.
    fn graph_dir(&self) -> PathBuf {
        self.root.join(".mimir").join("graph")
    }

    /// Handle removing a file from the graph.
    fn handle_remove_file(
        &mut self,
        file_id: FileId,
        events: &EventSender,
    ) -> Result<bool, GraphError> {
        debug!("Removing file: {:?}", file_id);

        // Find and remove from tracked files
        let mut path_to_remove = None;
        for (path, tracked) in &self.files {
            if tracked.file_id == file_id {
                path_to_remove = Some(path.clone());
                break;
            }
        }
        if let Some(path) = path_to_remove {
            self.files.remove(&path);
        }
        self.file_paths.remove(&file_id);
        self.sources.remove(&file_id);

        let removed = self.graph.remove_file(file_id);
        let was_removed = !removed.is_empty();
        self.needs_link = true;

        if was_removed {
            let _ = events.send(GraphEvent::FileRemoved {
                file_id,
                path: PathBuf::new(),
            });
        }

        Ok(was_removed)
    }

    /// Remove a file by path.
    fn remove_file_by_path(&mut self, path: &PathBuf, events: &EventSender) -> Option<usize> {
        if let Some(tracked) = self.files.remove(path) {
            self.file_paths.remove(&tracked.file_id);
            self.sources.remove(&tracked.file_id);
            let removed = self.graph.remove_file(tracked.file_id);
            let symbols_removed = removed.len();

            if symbols_removed > 0 {
                let _ = events.send(GraphEvent::FileRemoved {
                    file_id: tracked.file_id,
                    path: path.clone(),
                });
            }

            Some(symbols_removed)
        } else {
            None
        }
    }

    /// Handle finding symbols by name.
    fn handle_find_symbol(
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
    fn handle_trace(
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
    fn handle_trace_by_name(
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
    fn handle_read(&self, name: &str, before: u32, after: u32) -> Result<SymbolSource, GraphError> {
        let found = self.graph.find_symbol(name);
        let best = source::most_likely(&found, name).ok_or_else(|| GraphError::SymbolNotFound {
            name: name.to_string(),
        })?;
        self.source_lines(best, before, after, None)
    }

    /// The first `lines` lines of a symbol, or all of them.
    fn handle_source(
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
    fn handle_stats(&self) -> StatsResponse {
        StatsResponse {
            files_count: self.files.len(),
            symbols_count: self.graph.symbol_count(),
            references_count: self.graph.edge_count(),
            files_by_language: Vec::new(),
            symbols_by_kind: Vec::new(),
        }
    }

    /// Handle searching for symbols by pattern.
    fn handle_search(&self, options: &SearchOptions) -> Result<SearchResponse, GraphError> {
        use regex::Regex;

        let pattern = Regex::new(&options.pattern).map_err(|e| {
            GraphError::WatcherError(format!("Invalid pattern '{}': {e}", options.pattern))
        })?;

        let limit = options.limit.unwrap_or(20) as usize;
        let mut matches = Vec::new();

        // Search all symbols in the graph
        for symbol in self.graph.symbols().all_symbols() {
            if pattern.is_match(&symbol.name)
                || pattern.is_match(&symbol.qualified_name.to_string())
            {
                // Apply kind filter if specified
                if let Some(ref kind_filter) = options.kind_filter {
                    let kind_str = format!("{:?}", symbol.kind).to_lowercase();
                    if !kind_str.contains(&kind_filter.to_lowercase()) {
                        continue;
                    }
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

    /// Scan for changes and resolve edges, settling for a provisional answer if that takes long.
    async fn handle_sync(&mut self, events: &EventSender) -> Result<SyncResult, GraphError> {
        let result = self.scan_files(events).await?;
        self.ensure_linked().await;
        Ok(result)
    }

    /// Bring the tracked files and their symbols up to date with the disk.
    async fn scan_files(&mut self, events: &EventSender) -> Result<SyncResult, GraphError> {
        let start = Instant::now();

        let _ = events.send(GraphEvent::SyncStarted);

        // Scan the directory
        let scanner = Scanner::new(&self.root);
        let scanned_files = scanner
            .scan()
            .await
            .map_err(|e| GraphError::WatcherError(format!("Scan failed: {}", e)))?;

        info!(
            "Scanned {} files in {:?}",
            scanned_files.len(),
            start.elapsed()
        );

        let mut files_added = 0;
        let mut files_updated = 0;
        let mut files_removed = 0;
        let mut symbols_added = 0;
        let mut symbols_removed = 0;

        // Build a set of paths we've seen
        let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

        // Process each scanned file
        for scanned in scanned_files {
            seen_paths.insert(scanned.absolute_path.clone());

            // Check if we already have this file
            if let Some(tracked) = self.files.get(&scanned.absolute_path) {
                // Check if content changed
                if tracked.content_hash != scanned.content_hash {
                    // File was updated - remove old, add new
                    if let Some(removed_count) =
                        self.remove_file_by_path(&scanned.absolute_path.clone(), events)
                    {
                        symbols_removed += removed_count;
                    }

                    // Add updated file
                    match self.sync_add_file(&scanned, events) {
                        Ok(added_count) => {
                            symbols_added += added_count;
                            files_updated += 1;
                        }
                        Err(e) => {
                            warn!(
                                "Failed to update {}: {}",
                                scanned.absolute_path.display(),
                                e
                            );
                        }
                    }
                }
                // File unchanged - skip
            } else {
                // New file - add it
                match self.sync_add_file(&scanned, events) {
                    Ok(added_count) => {
                        symbols_added += added_count;
                        files_added += 1;
                    }
                    Err(e) => {
                        warn!("Failed to add {}: {}", scanned.absolute_path.display(), e);
                    }
                }
            }
        }

        // Find files that were removed
        let tracked_paths: Vec<PathBuf> = self.files.keys().cloned().collect();
        for path in tracked_paths {
            if !seen_paths.contains(&path)
                && let Some(removed_count) = self.remove_file_by_path(&path, events)
            {
                symbols_removed += removed_count;
                files_removed += 1;
            }
        }

        // Calls can point at any file, so edges are resolved once all files are in
        self.needs_link |= files_added + files_updated + files_removed > 0;

        let duration_ms = start.elapsed().as_millis() as u64;

        let result = SyncResult {
            files_added,
            files_updated,
            files_removed,
            symbols_added,
            symbols_removed,
            duration_ms,
            link: None,
            saved_to: None,
        };

        info!(
            "Sync completed: {} added, {} updated, {} removed in {}ms",
            files_added, files_updated, files_removed, duration_ms
        );

        let _ = events.send(GraphEvent::SyncCompleted {
            files_processed: files_added + files_updated,
            duration_ms,
        });

        Ok(result)
    }

    /// Add a file during sync operation.
    fn sync_add_file(
        &mut self,
        scanned: &ScannedFile,
        events: &EventSender,
    ) -> Result<usize, GraphError> {
        // Read file content
        let content = std::fs::read_to_string(&scanned.absolute_path).map_err(|_| {
            GraphError::FileNotFound {
                path: scanned.absolute_path.clone(),
            }
        })?;

        let (_, symbols_count) = self.insert_file(
            scanned.absolute_path.clone(),
            scanned.path.clone(),
            content,
            scanned.content_hash,
            scanned.language,
            events,
        )?;

        Ok(symbols_count)
    }

    /// Convert a Symbol to SymbolInfo.
    fn symbol_to_info(&self, sym: &Symbol) -> SymbolInfo {
        let file_path = self
            .file_paths
            .get(&sym.file_id)
            .cloned()
            .unwrap_or_default();
        SymbolInfo {
            id: sym.id,
            name: sym.name.clone(),
            qualified_name: Some(sym.qualified_name.to_string()),
            kind: sym.kind,
            file_id: sym.file_id,
            file_path,
            start_line: sym.range.start_line,
            end_line: sym.range.end_line,
            language: sym.language,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{command_channel, event_channel};
    use mimir_parse_rust::RustAdapter;

    fn create_test_registry() -> Arc<AdapterRegistry> {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(RustAdapter::new()));
        Arc::new(registry)
    }

    #[tokio::test]
    async fn test_actor_lifecycle() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, _event_rx) = event_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve_ra::RustAnalyzer::default()),
        );

        let handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
        handle.await.unwrap().unwrap();
    }

    /// Test: adding a file indexes symbols.
    #[tokio::test]
    async fn test_add_file() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, mut event_rx) = event_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve_ra::RustAnalyzer::default()),
        );

        let handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        // Add a file
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::AddFile {
                path: PathBuf::from("test.rs"),
                content: "fn hello() {}".to_string(),
                language: Language::Rust,
                reply: reply_tx,
            })
            .await
            .unwrap();

        let file_id = reply_rx.await.unwrap().unwrap();
        assert_eq!(file_id, FileId::new(1));

        // Check event was emitted
        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, GraphEvent::FileAdded { .. }));

        cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_find_symbol() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, _event_rx) = event_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve_ra::RustAnalyzer::default()),
        );

        let handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        // Add a file
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::AddFile {
                path: PathBuf::from("test.rs"),
                content: "fn process() {} fn helper() {}".to_string(),
                language: Language::Rust,
                reply: reply_tx,
            })
            .await
            .unwrap();
        reply_rx.await.unwrap().unwrap();

        // Find symbol
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::FindSymbol {
                name: "process".to_string(),
                options: Default::default(),
                reply: reply_tx,
            })
            .await
            .unwrap();

        let response = reply_rx.await.unwrap();
        assert!(response.has_results());
        assert_eq!(response.symbols[0].name, "process");

        cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_stats() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, _event_rx) = event_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve_ra::RustAnalyzer::default()),
        );

        let handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        // Add a file
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::AddFile {
                path: PathBuf::from("test.rs"),
                content: "fn a() {} fn b() {}".to_string(),
                language: Language::Rust,
                reply: reply_tx,
            })
            .await
            .unwrap();
        reply_rx.await.unwrap().unwrap();

        // Get stats
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::Stats { reply: reply_tx })
            .await
            .unwrap();

        let stats = reply_rx.await.unwrap();
        assert_eq!(stats.symbols_count, 2);

        cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_remove_file() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, _event_rx) = event_channel(16);

        let actor = GraphActor::with_registry(
            PathBuf::from("/tmp/test"),
            create_test_registry(),
            Arc::new(mimir_resolve_ra::RustAnalyzer::default()),
        );

        let handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        // Add a file
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::AddFile {
                path: PathBuf::from("test.rs"),
                content: "fn test() {}".to_string(),
                language: Language::Rust,
                reply: reply_tx,
            })
            .await
            .unwrap();
        let file_id = reply_rx.await.unwrap().unwrap();

        // Check stats before
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::Stats { reply: reply_tx })
            .await
            .unwrap();
        let stats = reply_rx.await.unwrap();
        assert_eq!(stats.symbols_count, 1);

        // Remove file
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::RemoveFile {
                file_id,
                reply: reply_tx,
            })
            .await
            .unwrap();
        let removed = reply_rx.await.unwrap().unwrap();
        assert!(removed);

        // Check stats after
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .send(GraphCommand::Stats { reply: reply_tx })
            .await
            .unwrap();
        let stats = reply_rx.await.unwrap();
        assert_eq!(stats.symbols_count, 0);

        cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
        handle.await.unwrap().unwrap();
    }
}
