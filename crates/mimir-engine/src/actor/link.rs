//! Resolving edges between files, and bringing the tracked files up to date with the disk.

use crate::actor::GraphActor;
use crate::error::GraphError;
use crate::save;
use crate::scanner::{ScannedFile, Scanner};
use mimir_protocol::{LinkSummary, SyncResult};
use mimir_resolve::{ProjectFile, Refresh, ScipIndex, link};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Semantic resolution running in the background, for a set of files.
pub struct PendingLink {
    indexes: tokio::task::JoinHandle<Vec<ScipIndex>>,
    files: Vec<ProjectFile>,
    /// The graph already holds what syntax rules give for these files.
    linked_by_syntax: bool,
}

impl GraphActor {
    /// Resolve every edge if a file changed since the last time. A query that reads edges
    /// waits for this, so several edits in a row cost one index and no answer is stale.
    /// Bring edges up to date, waiting only briefly: if semantic resolution takes longer, it
    /// goes on in the background and the graph has what syntax rules give until it is done.
    /// Later queries pick up its result.
    pub async fn ensure_linked(&mut self) {
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
            }
        }
    }

    /// Scan, resolve every edge without settling for a provisional answer, and save the graph.
    pub async fn handle_sync_complete(&mut self, full: bool) -> Result<SyncResult, GraphError> {
        let mut result = self.scan_files().await?;
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
                .map_err(GraphError::Index)?;
        let report = link(&self.graph, &files, &indexes);

        let graph_file = save::save(&self.graph, &self.file_paths, &self.graph_dir())
            .map_err(GraphError::Save)?;

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

    /// Scan for changes and resolve edges, settling for a provisional answer if that takes long.
    pub async fn handle_sync(&mut self) -> Result<SyncResult, GraphError> {
        let result = self.scan_files().await?;
        self.ensure_linked().await;
        Ok(result)
    }

    /// Bring the tracked files and their symbols up to date with the disk.
    pub async fn scan_files(&mut self) -> Result<SyncResult, GraphError> {
        let start = Instant::now();

        // Scan the directory
        let scanner = Scanner::new(&self.root);
        let scanned_files = scanner.scan().await.map_err(GraphError::Scan)?;

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
                    if let Some(removed_count) =
                        self.remove_file_by_path(&scanned.absolute_path.clone())
                    {
                        symbols_removed += removed_count;
                    }

                    // Add updated file
                    match self.sync_add_file(&scanned) {
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
                match self.sync_add_file(&scanned) {
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
                && let Some(removed_count) = self.remove_file_by_path(&path)
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

        Ok(result)
    }

    /// Add a file during sync operation.
    pub fn sync_add_file(&mut self, scanned: &ScannedFile) -> Result<usize, GraphError> {
        // Read file content
        let content =
            std::fs::read_to_string(&scanned.absolute_path).map_err(|source| GraphError::Read {
                path: scanned.absolute_path.clone(),
                source,
            })?;

        let (_, symbols_count) = self.insert_file(
            scanned.absolute_path.clone(),
            scanned.path.clone(),
            content,
            scanned.content_hash,
            scanned.language,
        )?;

        Ok(symbols_count)
    }
}
