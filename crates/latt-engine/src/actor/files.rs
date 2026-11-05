//! Adding, inserting and removing files in the graph.

use crate::actor::{GraphActor, TrackedFile};
use crate::error::GraphError;
use crate::save;
use latt_graph::FileEntry;
use latt_resolve::ProjectFile;
use latt_types::{ContentHash, FileId, Language, QualifiedName, Symbol, SymbolId, SymbolKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, warn};

/// When a file last changed, in milliseconds.
fn modified_millis(file: &std::fs::Metadata) -> Option<i64> {
    let since = file
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(since.as_millis()).ok()
}

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
        // nothing, and replacing it would start a new resolution.
        let content_hash = ContentHash::compute(content.as_bytes());
        if let Some(tracked) = self.files.get(&path)
            && tracked.content_hash == content_hash
        {
            return Ok(tracked.file_id);
        }

        // The old version goes in the same step as the new one comes.
        let gone: Vec<FileId> = self
            .files
            .get(&path)
            .map(|tracked| tracked.file_id)
            .into_iter()
            .collect();
        if let Some(old) = self.files.get(&path).cloned() {
            debug!("File already exists, replacing it: {:?}", old.file_id);
            self.forget_path(&path);
        }

        // Store reverse mapping for file path lookup
        let relative_path = path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf();
        let entry_path = relative_path.clone();
        // Nothing known of it on disk is 0: the next scan then reads it to see.
        let (size, modified) = std::fs::metadata(&path).map_or((0, 0), |file| {
            (file.len(), modified_millis(&file).unwrap_or(0))
        });
        let (file_id, symbols) =
            self.insert_file(path, relative_path, content, content_hash, language)?;
        self.graph.change_files(
            &gone,
            vec![FileEntry {
                id: file_id,
                path: entry_path,
                hash: content_hash,
                size,
                modified,
            }],
            symbols,
        );

        self.needs_link = true;

        Ok(file_id)
    }

    /// Parse a file and track it, giving its symbols for the caller to add to the graph, as adding
    /// copies the graph. Edges come from `relink`.
    pub fn insert_file(
        &mut self,
        path: PathBuf,
        relative_path: PathBuf,
        content: String,
        content_hash: ContentHash,
        language: Language,
    ) -> Result<(FileId, Vec<Symbol>), GraphError> {
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
        let file_id = FileId::of_path(&relative_path);

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
        let mut extracted_symbols = adapter.extract_symbols(&tree, file_id);
        adapter.qualify(&relative_path, &mut extracted_symbols);
        let references = adapter.extract_references(&tree, file_id);
        self.references_changed = true;
        self.sources.insert(
            file_id,
            ProjectFile {
                file_id,
                path: relative_path,
                source: content.as_str().into(),
                references: references.into(),
            },
        );

        let mut same_name: HashMap<(&QualifiedName, SymbolKind), u32> = HashMap::new();
        let symbols = extracted_symbols
            .iter()
            .map(|extracted| {
                let order = same_name
                    .entry((&extracted.qualified_name, extracted.kind))
                    .or_default();
                let id =
                    SymbolId::lasting(file_id, &extracted.qualified_name, extracted.kind, *order);
                *order += 1;
                Symbol {
                    id,
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
                }
            })
            .collect();

        Ok((file_id, symbols))
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
        let mut ids = Vec::with_capacity(gone.len());
        for tracked in &gone {
            ids.extend(self.forget_path(tracked).map(|(file_id, _)| file_id));
            debug!("File removed: {}", tracked.display());
        }
        if !ids.is_empty() {
            self.graph.change_files(&ids, Vec::new(), Vec::new());
        }
        self.needs_link |= !gone.is_empty();
    }

    /// Read what a link needs of the files a saved graph holds and this run has not read. A start
    /// with no change never comes here.
    pub fn ensure_sources(&mut self) {
        let missing: Vec<(FileId, PathBuf)> = self
            .file_paths
            .iter()
            .filter(|(id, _)| !self.sources.contains_key(id))
            .map(|(id, path)| (*id, path.clone()))
            .collect();
        if missing.is_empty() {
            return;
        }
        let stored = save::load_references(&self.graph_dir());
        for (file_id, path) in missing {
            let language = Language::from_path(&path);
            let Some(adapter) = self.registry.get_by_language(language) else {
                continue;
            };
            let content = match std::fs::read_to_string(self.root.join(&path)) {
                Ok(content) => content,
                Err(e) => {
                    warn!("Could not read {}: {e}", path.display());
                    continue;
                }
            };
            let hash = ContentHash::compute(content.as_bytes());
            let stored_references = stored
                .as_ref()
                .and_then(|stored| stored.references_of(file_id, hash));
            let references = match stored_references {
                Some(references) => references,
                None => {
                    let Some(tree) = adapter.parse(&content) else {
                        warn!("Could not parse {}", path.display());
                        continue;
                    };
                    self.references_changed = true;
                    adapter.extract_references(&tree, file_id)
                }
            };
            self.sources.insert(
                file_id,
                ProjectFile {
                    file_id,
                    path,
                    source: content.as_str().into(),
                    references: references.into(),
                },
            );
        }
    }

    /// The text of a file: as read, or from the disk if this run has not read it.
    pub fn text_of(&self, file_id: FileId) -> Option<Arc<str>> {
        if let Some(file) = self.sources.get(&file_id) {
            return Some(file.source.clone());
        }
        let path = self.root.join(self.file_paths.get(&file_id)?);
        Some(std::fs::read_to_string(path).ok()?.into())
    }

    /// Stop tracking a file, giving its id and how many symbols it has, for the caller to remove
    /// them from the graph together with others.
    pub fn forget_path(&mut self, path: &PathBuf) -> Option<(FileId, usize)> {
        let tracked = self.files.remove(path)?;
        self.references_changed = true;
        self.file_paths.remove(&tracked.file_id);
        self.sources.remove(&tracked.file_id);
        Some((tracked.file_id, self.graph.count_in_file(tracked.file_id)))
    }
}
