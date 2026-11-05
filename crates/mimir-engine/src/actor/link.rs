//! Resolving edges between files, and bringing the tracked files up to date with the disk.

use crate::actor::{GraphActor, answer};
use crate::error::GraphError;
use crate::save;
use crate::scanner::{Known, ScannedFile, Scanner};
use mimir_graph::FileEntry;
use mimir_protocol::{LinkSummary, SyncResult};
use mimir_resolve::{LinkReport, ProjectFile, Refresh, SemanticIndex, link};
use mimir_types::Symbol;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

/// Semantic resolution running in the background, for a set of files.
pub struct PendingLink {
    pub indexes: tokio::task::JoinHandle<Vec<SemanticIndex>>,
    files: Vec<ProjectFile>,
    /// The graph already holds what syntax rules give for these files.
    linked_by_syntax: bool,
}

/// A sync whose answer waits for semantic resolution.
pub struct WaitingSync {
    reply: oneshot::Sender<Result<SyncResult, GraphError>>,
    result: SyncResult,
    started: Instant,
    /// Write the graph as JSON, for measuring it from outside.
    json: bool,
}

impl GraphActor {
    /// Resolve every edge if a file changed since the last time. A query that reads edges
    /// waits for this, so several edits in a row cost one index and no answer is stale.
    /// Bring edges up to date, waiting only briefly: if semantic resolution takes longer, it
    /// goes on in the background and the graph has what syntax rules give until it is done.
    /// Later queries pick up its result.
    pub async fn ensure_linked(&mut self) {
        if self.needs_link {
            self.start_link(Refresh::Edits);
        }
        let cache_dir = self.graph_dir();
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        let wait = if pending.linked_by_syntax {
            Duration::ZERO
        } else {
            self.query_wait
        };
        match tokio::time::timeout(wait, &mut pending.indexes).await {
            Ok(indexes) => self.finish_link(indexes),
            Err(_) if pending.linked_by_syntax => {}
            Err(_) => {
                let (root, scan) = (self.root.clone(), pending.files.clone());
                let semantics = self.semantics.clone();
                let saved = match tokio::task::spawn_blocking(move || {
                    semantics.load(&root, &cache_dir, &scan, Refresh::Saved)
                })
                .await
                {
                    Ok(saved) => saved,
                    Err(e) => {
                        warn!("Saved semantic indexes failed to load: {}", e);
                        Vec::new()
                    }
                };
                let report = link(&self.graph, &pending.files, &saved);
                debug!("Linked for now: {:?}", report);
                pending.linked_by_syntax = true;
                self.announce_new_edges();
            }
        }
    }

    /// Start resolving every edge of the files as they are now. An unfinished resolution is left
    /// to end alone; this one waits for it.
    fn start_link(&mut self, refresh: Refresh) {
        self.needs_link = false;
        self.announced_change = false;
        self.ensure_sources();
        let files: Vec<ProjectFile> = self.sources.values().cloned().collect();
        let (root, cache_dir, scan) = (self.root.clone(), self.graph_dir(), files.clone());
        let semantics = self.semantics.clone();
        let indexes =
            tokio::task::spawn_blocking(move || semantics.load(&root, &cache_dir, &scan, refresh));
        self.pending = Some(PendingLink {
            indexes,
            files,
            linked_by_syntax: false,
        });
    }

    /// Take in what semantic resolution found, and answer the syncs that wait for it.
    pub fn finish_link(&mut self, indexes: Result<Vec<SemanticIndex>, tokio::task::JoinError>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        // Files changed meanwhile: these edges would be those of files no longer in the graph, and a
        // waiting sync is owed the files as they are.
        if self.needs_link && !self.waiting_syncs.is_empty() {
            self.start_link(Refresh::Edits);
            return;
        }
        let failure = indexes.as_ref().err().map(ToString::to_string);
        let indexes = indexes.unwrap_or_else(|e| {
            warn!("Semantic indexing failed: {}", e);
            Vec::new()
        });
        let report = link(&self.graph, &pending.files, &indexes);
        debug!("Linked: {:?}", report);
        // Edges of files as they were, or from no index, are not what the next start should find.
        if !self.needs_link && failure.is_none() {
            let references = self.references_changed.then(|| {
                self.files
                    .values()
                    .filter_map(|tracked| {
                        let file = self.sources.get(&tracked.file_id)?;
                        Some((
                            tracked.file_id,
                            tracked.content_hash,
                            file.references.clone(),
                        ))
                    })
                    .collect()
            });
            self.references_changed = false;
            self.writer.save(save::Stored {
                graph: self.graph.snapshot(),
                references,
            });
        }
        self.announce_new_edges();

        for waiting in std::mem::take(&mut self.waiting_syncs) {
            let answered = match &failure {
                Some(reason) => Err(GraphError::Index(reason.clone())),
                None => self.finished_sync(waiting.result, waiting.started, waiting.json, &report),
            };
            answer(waiting.reply, answered);
        }
    }

    fn finished_sync(
        &self,
        mut result: SyncResult,
        started: Instant,
        json: bool,
        report: &LinkReport,
    ) -> Result<SyncResult, GraphError> {
        if json {
            let saved = save::save(&self.graph, &self.file_paths, &self.graph_dir())
                .map_err(GraphError::Save)?;
            result.saved_to = Some(saved);
        }
        result.link = Some(LinkSummary {
            semantic_calls: report.semantic_calls,
            syntax_calls: report.syntax_calls,
            implementations: report.implementations,
            function_values: report.function_values,
            constant_uses: report.constant_uses,
            type_uses: report.type_uses,
            field_uses: report.field_uses,
            imports: report.imports,
            semantic_files: report.semantic_files,
            live_files: report.live_files,
            syntax_files: report.syntax_files,
        });
        result.notes = self.semantics.notes(&self.root);
        result.duration_ms += started.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Scan and start resolving every edge. The answer comes when that is done, without settling
    /// for a provisional one; meanwhile the actor goes on answering from the graph it has.
    pub async fn handle_sync_complete(
        &mut self,
        full: bool,
        json: bool,
        reply: oneshot::Sender<Result<SyncResult, GraphError>>,
    ) {
        let result = match self.scan_files(full).await {
            Ok(result) => result,
            Err(e) => return answer(reply, Err(e)),
        };

        // A link still running gives way: this one covers the same files or newer ones.
        self.start_link(if full { Refresh::Full } else { Refresh::Edits });
        self.waiting_syncs.push(WaitingSync {
            reply,
            result,
            started: Instant::now(),
            json,
        });
    }

    /// Scan for changes and resolve edges, settling for a provisional answer if that takes long.
    pub async fn handle_sync(&mut self) -> Result<SyncResult, GraphError> {
        let result = self.scan_files(false).await?;
        self.ensure_linked().await;
        Ok(result)
    }

    /// Bring the tracked files and their symbols up to date with the disk.
    pub async fn scan_files(&mut self, full: bool) -> Result<SyncResult, GraphError> {
        let start = Instant::now();
        let known: Known = if full {
            Known::new()
        } else {
            self.graph
                .files()
                .into_iter()
                .map(|file| {
                    (
                        self.root.join(&file.path),
                        (file.size, file.modified, file.hash),
                    )
                })
                .collect()
        };

        // Scan the directory
        let scanner = Scanner::new(&self.root);
        let scanned_files = scanner.scan(known).await.map_err(GraphError::Scan)?;

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
        let mut symbols = Vec::new();
        let mut entries = Vec::new();
        let mut gone = Vec::new();

        // Process each scanned file
        for scanned in scanned_files {
            // A file in a language Mimir does not read yet is not an error.
            if self.registry.get_by_language(scanned.language).is_none() {
                continue;
            }
            seen_paths.insert(scanned.absolute_path.clone());

            // Check if we already have this file
            if let Some(tracked) = self.files.get(&scanned.absolute_path) {
                // Check if content changed
                if tracked.content_hash != scanned.content_hash {
                    // File was updated - remove old, add new
                    if let Some((file_id, removed_count)) =
                        self.forget_path(&scanned.absolute_path.clone())
                    {
                        gone.push(file_id);
                        symbols_removed += removed_count;
                    }

                    // Add updated file
                    match self.sync_add_file(&scanned, &mut symbols, &mut entries) {
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
                match self.sync_add_file(&scanned, &mut symbols, &mut entries) {
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
                && let Some((file_id, removed_count)) = self.forget_path(&path)
            {
                gone.push(file_id);
                symbols_removed += removed_count;
                files_removed += 1;
            }
        }

        if !gone.is_empty() || !entries.is_empty() {
            self.graph.change_files(&gone, entries, symbols);
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
            notes: Vec::new(),
        };

        info!(
            "Sync completed: {} added, {} updated, {} removed in {}ms",
            files_added, files_updated, files_removed, duration_ms
        );

        Ok(result)
    }

    /// Add a file during sync operation: its symbols join those to add when the scan ends.
    pub fn sync_add_file(
        &mut self,
        scanned: &ScannedFile,
        symbols: &mut Vec<Symbol>,
        entries: &mut Vec<FileEntry>,
    ) -> Result<usize, GraphError> {
        // Read file content
        let content =
            std::fs::read_to_string(&scanned.absolute_path).map_err(|source| GraphError::Read {
                path: scanned.absolute_path.clone(),
                source,
            })?;

        let (id, new) = self.insert_file(
            scanned.absolute_path.clone(),
            scanned.path.clone(),
            content,
            scanned.content_hash,
            scanned.language,
        )?;
        entries.push(FileEntry {
            id,
            path: scanned.path.clone(),
            hash: scanned.content_hash,
            size: scanned.size,
            modified: scanned.modified_at.timestamp_millis(),
        });
        let count = new.len();
        symbols.extend(new);

        Ok(count)
    }
}
