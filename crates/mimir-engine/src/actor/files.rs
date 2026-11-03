//! Adding, inserting and removing files in the graph.

use crate::actor::{GraphActor, TrackedFile};
use crate::error::GraphError;
use mimir_resolve::ProjectFile;
use mimir_types::{ContentHash, FileId, Language, Symbol};
use std::path::{Path, PathBuf};
use tracing::debug;

impl GraphActor {
    /// Handle adding a file to the graph.
    pub fn handle_add_file(
        &mut self,
        path: PathBuf,
        content: String,
        language: Language,
    ) -> Result<FileId, GraphError> {
        debug!("Adding file: {}", path.display());

        // A watcher reports a file again after an edit that already took it in: the same text changes
        // nothing, and replacing it would give its symbols new ids and start a new resolution.
        let content_hash = ContentHash::compute(content.as_bytes());
        if let Some(tracked) = self.files.get(&path)
            && tracked.content_hash == content_hash
        {
            return Ok(tracked.file_id);
        }

        // If file already exists, remove old version first (for updates)
        if let Some(tracked) = self.files.get(&path).cloned() {
            debug!(
                "File already exists, removing old version: {:?}",
                tracked.file_id
            );
            self.handle_remove_file(tracked.file_id)?;
        }

        // Store reverse mapping for file path lookup
        let relative_path = path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf();
        let (file_id, _) =
            self.insert_file(path, relative_path, content, content_hash, language)?;

        self.needs_link = true;

        Ok(file_id)
    }

    /// Parse a file and add it and its symbols to the graph. Edges come from `relink`.
    pub fn insert_file(
        &mut self,
        path: PathBuf,
        relative_path: PathBuf,
        content: String,
        content_hash: ContentHash,
        language: Language,
    ) -> Result<(FileId, usize), GraphError> {
        // Get adapter for this language
        let adapter = self
            .registry
            .get_by_language(language)
            .ok_or_else(|| GraphError::NoAdapter { language })?;

        // Parse the file
        let tree = adapter
            .parse(&content)
            .ok_or_else(|| GraphError::ParseFailed { path: path.clone() })?;

        // Assign file ID
        let file_id: FileId = self.ids.next();

        // Track the file
        self.files.insert(
            path.clone(),
            TrackedFile {
                file_id,
                content_hash,
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

        Ok((file_id, symbols_count))
    }

    /// Handle removing a file from the graph.
    pub fn handle_remove_file(&mut self, file_id: FileId) -> Result<bool, GraphError> {
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

        Ok(was_removed)
    }

    /// Forget a file, or everything under a directory. A path the graph does not hold changes nothing.
    pub fn handle_remove_path(&mut self, path: &Path) {
        let gone: Vec<PathBuf> = self
            .files
            .keys()
            .filter(|tracked| tracked.starts_with(path))
            .cloned()
            .collect();
        for tracked in &gone {
            self.remove_file_by_path(tracked);
            debug!("File removed: {}", tracked.display());
        }
        self.needs_link |= !gone.is_empty();
    }

    /// Remove a file by path.
    pub fn remove_file_by_path(&mut self, path: &PathBuf) -> Option<usize> {
        if let Some(tracked) = self.files.remove(path) {
            self.file_paths.remove(&tracked.file_id);
            self.sources.remove(&tracked.file_id);
            let removed = self.graph.remove_file(tracked.file_id);
            let symbols_removed = removed.len();

            Some(symbols_removed)
        } else {
            None
        }
    }
}
