//! Applying edits to files, and taking the edited files into the graph at once.

use crate::actor::GraphActor;
use crate::channel::EventSender;
use crate::error::GraphError;
use crate::{edit, source};
use mimir_edit::{EditError, EditOp, Editor};
use mimir_protocol::{Edit, EditReport, EditTarget, FileEdit};
use mimir_types::Language;

impl GraphActor {
    /// Apply edits together. A symbol's lines come from the graph, so if a file changed on disk and
    /// the watcher has not said so yet, the edit fails its hash check, and is tried once more after
    /// a scan.
    pub async fn handle_edit(
        &mut self,
        edits: &[Edit],
        preview: bool,
        backup: bool,
        events: &EventSender,
    ) -> Result<EditReport, GraphError> {
        let editor = if backup {
            Editor::new().with_backups()
        } else {
            Editor::new()
        };
        match self.apply_edits(&editor, edits, preview).await {
            Err(GraphError::Edit(EditError::HashMismatch { .. })) => {
                self.scan_files(events).await?;
                let report = self.apply_edits(&editor, edits, preview).await?;
                self.reload_edited(&report, preview, events)?;
                Ok(report)
            }
            Ok(report) => {
                self.reload_edited(&report, preview, events)?;
                Ok(report)
            }
            Err(e) => Err(e),
        }
    }

    async fn apply_edits(
        &self,
        editor: &Editor,
        edits: &[Edit],
        preview: bool,
    ) -> Result<EditReport, GraphError> {
        let ops = edits
            .iter()
            .map(|edit| self.edit_op(edit))
            .collect::<Result<Vec<_>, _>>()?;
        let mut files = Vec::new();
        if preview {
            for op in &ops {
                let diff = editor.preview(op).await?;
                files.push(FileEdit {
                    path: self.relative(op.file_path()),
                    bytes_changed: diff.new_content.len() as i64 - diff.old_content.len() as i64,
                    lines_changed: diff.new_content.lines().count() as i64
                        - diff.old_content.lines().count() as i64,
                    diff: Some(diff.unified),
                });
            }
        } else {
            for result in editor.apply_batch(&ops).await? {
                files.push(FileEdit {
                    path: self.relative(&result.file_path),
                    bytes_changed: result.bytes_changed,
                    lines_changed: result.lines_changed,
                    diff: None,
                });
            }
        }
        Ok(EditReport { files })
    }

    fn edit_op(&self, edit: &Edit) -> Result<EditOp, GraphError> {
        match &edit.target {
            EditTarget::File(path) => {
                let absolute = self.root.join(path);
                let inside = absolute
                    .canonicalize()
                    .map_err(|_| GraphError::FileNotFound {
                        path: absolute.clone(),
                    })?;
                if !inside.starts_with(&self.root) {
                    return Err(GraphError::OutsideProject(absolute));
                }
                edit::on_file(&inside, &edit.change)
            }
            EditTarget::Symbol(name) => {
                let found = self.graph.find_symbol(name);
                let symbol = source::most_likely(&found, name).ok_or_else(|| {
                    GraphError::SymbolNotFound {
                        name: name.to_string(),
                    }
                })?;
                let path = self
                    .root
                    .join(self.file_paths.get(&symbol.file_id).ok_or_else(|| {
                        GraphError::SymbolNotFound {
                            name: name.to_string(),
                        }
                    })?);
                let hash = self
                    .files
                    .get(&path)
                    .map(|tracked| tracked.content_hash)
                    .ok_or_else(|| GraphError::FileNotFound { path: path.clone() })?;
                edit::on_symbol(&path, symbol, hash, &edit.change)
            }
        }
    }

    /// Take edited files into the graph at once, without waiting for the watcher to report them.
    fn reload_edited(
        &mut self,
        report: &EditReport,
        preview: bool,
        events: &EventSender,
    ) -> Result<(), GraphError> {
        if preview {
            return Ok(());
        }
        for file in &report.files {
            let path = self.root.join(&file.path);
            let content = std::fs::read_to_string(&path).map_err(|source| GraphError::Read {
                path: path.clone(),
                source,
            })?;
            let language = Language::from_path(&path);
            if self.registry.get_by_language(language).is_some() {
                self.handle_add_file(path, content, language, events)?;
            }
        }
        Ok(())
    }
}
