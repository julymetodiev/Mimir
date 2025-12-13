//! Graph actor implementation.

use mimir_error::GraphError;
use mimir_files::{ScannedFile, Scanner};
use mimir_graph::CodeGraph;
use mimir_parse_core::AdapterRegistry;
use mimir_protocol::{
    CommandReceiver, EventSender, FindResponse, GraphCommand, GraphEvent, StatsResponse,
    SymbolInfo, SyncResult, TraceNode, TraceRelation, TraceResponse,
};
use mimir_types::{ContentHash, FileId, IdGenerator, Language, Symbol, SymbolId, SymbolKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

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
}

impl GraphActor {
    /// Create a new graph actor.
    pub fn new(root: PathBuf) -> Self {
        Self {
            graph: CodeGraph::new(),
            registry: Arc::new(AdapterRegistry::new()),
            root,
            ids: IdGenerator::new(),
            files: HashMap::new(),
            file_paths: HashMap::new(),
        }
    }

    /// Create with a custom adapter registry.
    pub fn with_registry(root: PathBuf, registry: Arc<AdapterRegistry>) -> Self {
        Self {
            graph: CodeGraph::new(),
            registry,
            root,
            ids: IdGenerator::new(),
            files: HashMap::new(),
            file_paths: HashMap::new(),
        }
    }

    /// Run the actor's command processing loop.
    pub async fn run(
        mut self,
        mut commands: CommandReceiver,
        events: EventSender,
    ) -> Result<(), GraphError> {
        info!("Graph actor starting for root: {}", self.root.display());

        while let Some(cmd) = commands.recv().await {
            match cmd {
                GraphCommand::AddFile {
                    path,
                    content,
                    language,
                    reply,
                } => {
                    let result = self.handle_add_file(path.clone(), content, language, &events);
                    let _ = reply.send(result);
                }

                GraphCommand::RemoveFile { file_id, reply } => {
                    let result = self.handle_remove_file(file_id, &events);
                    let _ = reply.send(result);
                }

                GraphCommand::FindSymbol {
                    name,
                    options,
                    reply,
                } => {
                    let response = self.handle_find_symbol(&name, &options);
                    let _ = reply.send(response);
                }

                GraphCommand::Trace {
                    symbol_id,
                    options,
                    reply,
                } => {
                    let response = self.handle_trace(symbol_id, &options);
                    let _ = reply.send(response);
                }

                GraphCommand::Stats { reply } => {
                    let response = self.handle_stats();
                    let _ = reply.send(response);
                }

                GraphCommand::Sync { reply } => {
                    let result = self.handle_sync(&events).await;
                    let _ = reply.send(result);
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
            debug!("File already exists, removing old version: {:?}", tracked.file_id);
            let _ = self.handle_remove_file(tracked.file_id, events);
        }

        // Get adapter for this language
        let adapter = self.registry.get_by_language(language).ok_or_else(|| {
            GraphError::FileNotFound {
                path: path.clone(),
            }
        })?;

        // Parse the file
        let tree = adapter.parse(&content).ok_or_else(|| GraphError::FileNotFound {
            path: path.clone(),
        })?;

        // Compute content hash
        let content_hash = ContentHash::compute(content.as_bytes());

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
        // Store reverse mapping for file path lookup
        let relative_path = path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf();
        self.file_paths.insert(file_id, relative_path);

        // Extract symbols
        let extracted_symbols = adapter.extract_symbols(&tree, file_id);
        let extracted_refs = adapter.extract_references(&tree, file_id);

        // Convert ExtractedSymbol to Symbol and add to graph
        let mut symbol_map: std::collections::HashMap<String, SymbolId> =
            std::collections::HashMap::new();

        for extracted in &extracted_symbols {
            let symbol_id: SymbolId = self.ids.next();
            symbol_map.insert(extracted.name.clone(), symbol_id);

            let symbol = Symbol {
                id: symbol_id,
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

        // Add references as edges
        for reference in &extracted_refs {
            // Find source symbol (the one making the reference)
            if let Some(&from_id) = reference.container.as_ref().and_then(|c| symbol_map.get(c)) {
                // Find target symbol by name
                if let Some(target) = self.graph.find_symbol(&reference.name).first() {
                    self.graph.add_call(
                        from_id,
                        target.id,
                        reference.span,
                        reference.confidence,
                    );
                }
            }
        }

        let symbols_count = extracted_symbols.len();

        // Emit event
        let _ = events.send(GraphEvent::FileAdded {
            file_id,
            path,
            symbols_count,
        });

        Ok(file_id)
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

        let removed = self.graph.remove_file(file_id);
        let was_removed = !removed.is_empty();

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
        let results = self.graph.find_symbol(name);
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
            };
        };

        let root_info = self.symbol_to_info(&root_sym);
        let max_depth = options.max_depth.unwrap_or(1);

        // Get callers
        let callers = if !options.callees_only {
            self.collect_callers(symbol_id, max_depth)
        } else {
            Vec::new()
        };

        // Get callees
        let callees = if !options.callers_only {
            self.collect_callees(symbol_id, max_depth)
        } else {
            Vec::new()
        };

        let total = 1 + callers.len() + callees.len();

        TraceResponse {
            root: root_info,
            callers,
            callees,
            max_depth_reached: max_depth,
            total_symbols: total,
        }
    }

    /// Collect callers recursively up to max depth.
    fn collect_callers(&self, symbol_id: SymbolId, max_depth: u32) -> Vec<TraceNode> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();

        self.collect_callers_recursive(symbol_id, 1, max_depth, &mut visited, &mut result);

        result
    }

    fn collect_callers_recursive(
        &self,
        symbol_id: SymbolId,
        depth: u32,
        max_depth: u32,
        visited: &mut std::collections::HashSet<SymbolId>,
        result: &mut Vec<TraceNode>,
    ) {
        if depth > max_depth || visited.contains(&symbol_id) {
            return;
        }
        visited.insert(symbol_id);

        for caller in self.graph.get_callers(symbol_id) {
            if !visited.contains(&caller.id) {
                let info = self.symbol_to_info(&caller);
                result.push(TraceNode {
                    symbol: info,
                    depth,
                    relationship: TraceRelation::Caller,
                });

                self.collect_callers_recursive(caller.id, depth + 1, max_depth, visited, result);
            }
        }
    }

    /// Collect callees recursively up to max depth.
    fn collect_callees(&self, symbol_id: SymbolId, max_depth: u32) -> Vec<TraceNode> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();

        self.collect_callees_recursive(symbol_id, 1, max_depth, &mut visited, &mut result);

        result
    }

    fn collect_callees_recursive(
        &self,
        symbol_id: SymbolId,
        depth: u32,
        max_depth: u32,
        visited: &mut std::collections::HashSet<SymbolId>,
        result: &mut Vec<TraceNode>,
    ) {
        if depth > max_depth || visited.contains(&symbol_id) {
            return;
        }
        visited.insert(symbol_id);

        for callee in self.graph.get_callees(symbol_id) {
            if !visited.contains(&callee.id) {
                let info = self.symbol_to_info(&callee);
                result.push(TraceNode {
                    symbol: info,
                    depth,
                    relationship: TraceRelation::Callee,
                });

                self.collect_callees_recursive(callee.id, depth + 1, max_depth, visited, result);
            }
        }
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

    /// Handle a sync operation.
    async fn handle_sync(&mut self, events: &EventSender) -> Result<SyncResult, GraphError> {
        let start = Instant::now();

        let _ = events.send(GraphEvent::SyncStarted);

        // Scan the directory
        let scanner = Scanner::new(&self.root);
        let scanned_files = scanner.scan().await.map_err(|e| {
            GraphError::WatcherError(format!("Scan failed: {}", e))
        })?;

        info!("Scanned {} files in {:?}", scanned_files.len(), start.elapsed());

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
                    if let Some(removed_count) = self.remove_file_by_path(&scanned.absolute_path.clone(), events) {
                        symbols_removed += removed_count;
                    }

                    // Add updated file
                    match self.sync_add_file(&scanned, events) {
                        Ok(added_count) => {
                            symbols_added += added_count;
                            files_updated += 1;
                        }
                        Err(e) => {
                            warn!("Failed to update {}: {}", scanned.absolute_path.display(), e);
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
            if !seen_paths.contains(&path) {
                if let Some(removed_count) = self.remove_file_by_path(&path, events) {
                    symbols_removed += removed_count;
                    files_removed += 1;
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        let result = SyncResult {
            files_added,
            files_updated,
            files_removed,
            symbols_added,
            symbols_removed,
            duration_ms,
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
    fn sync_add_file(&mut self, scanned: &ScannedFile, events: &EventSender) -> Result<usize, GraphError> {
        // Read file content
        let content = std::fs::read_to_string(&scanned.absolute_path).map_err(|_| {
            GraphError::FileNotFound {
                path: scanned.absolute_path.clone(),
            }
        })?;

        // Get adapter for this language
        let adapter = self.registry.get_by_language(scanned.language).ok_or_else(|| {
            GraphError::FileNotFound {
                path: scanned.absolute_path.clone(),
            }
        })?;

        // Parse the file
        let tree = adapter.parse(&content).ok_or_else(|| {
            GraphError::FileNotFound {
                path: scanned.absolute_path.clone(),
            }
        })?;

        // Assign file ID
        let file_id: FileId = self.ids.next();

        // Track the file
        self.files.insert(
            scanned.absolute_path.clone(),
            TrackedFile {
                file_id,
                content_hash: scanned.content_hash.clone(),
                language: scanned.language,
            },
        );
        // Store reverse mapping for file path lookup
        self.file_paths.insert(file_id, scanned.path.clone());

        // Extract symbols
        let extracted_symbols = adapter.extract_symbols(&tree, file_id);
        let extracted_refs = adapter.extract_references(&tree, file_id);

        // Convert ExtractedSymbol to Symbol and add to graph
        let mut symbol_map: HashMap<String, SymbolId> = HashMap::new();

        for extracted in &extracted_symbols {
            let symbol_id: SymbolId = self.ids.next();
            symbol_map.insert(extracted.name.clone(), symbol_id);

            let symbol = Symbol {
                id: symbol_id,
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

        // Add references as edges
        for reference in &extracted_refs {
            if let Some(&from_id) = reference.container.as_ref().and_then(|c| symbol_map.get(c)) {
                if let Some(target) = self.graph.find_symbol(&reference.name).first() {
                    self.graph.add_call(
                        from_id,
                        target.id,
                        reference.span,
                        reference.confidence,
                    );
                }
            }
        }

        let symbols_count = extracted_symbols.len();

        // Emit event
        let _ = events.send(GraphEvent::FileAdded {
            file_id,
            path: scanned.absolute_path.clone(),
            symbols_count,
        });

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
    use mimir_parse_rust::RustAdapter;
    use mimir_protocol::{command_channel, event_channel};
    use tokio::sync::oneshot;

    fn create_test_registry() -> Arc<AdapterRegistry> {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(RustAdapter::new()));
        Arc::new(registry)
    }

    #[tokio::test]
    async fn test_actor_lifecycle() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, _event_rx) = event_channel(16);

        let actor =
            GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

        let handle = tokio::spawn(async move { actor.run(cmd_rx, event_tx).await });

        cmd_tx.send(GraphCommand::Shutdown).await.unwrap();
        handle.await.unwrap().unwrap();
    }

    /// Test: adding a file indexes symbols.
    #[tokio::test]
    async fn test_add_file() {
        let (cmd_tx, cmd_rx) = command_channel(16);
        let (event_tx, mut event_rx) = event_channel(16);

        let actor =
            GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

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

        let actor =
            GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

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

        let actor =
            GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

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

        let actor =
            GraphActor::with_registry(PathBuf::from("/tmp/test"), create_test_registry());

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
