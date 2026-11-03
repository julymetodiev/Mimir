//! Graph actor implementation.
//!
//! The actor owns the graph and handles its commands one at a time. The methods are split by
//! responsibility into child modules: `files` (add and remove), `link` (edges and the scan),
//! `queries` (read-only answers) and `edits` (applying edits).

mod edits;
mod files;
mod link;
mod queries;
#[cfg(test)]
mod tests;

use crate::channel::CommandReceiver;
use crate::command::GraphCommand;
use crate::error::GraphError;
use mimir_graph::CodeGraph;
use mimir_parse_core::AdapterRegistry;
use mimir_protocol::SymbolInfo;
use mimir_resolve::{ProjectFile, SemanticSource};
use mimir_types::{ContentHash, FileId, IdGenerator, Symbol};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tracing::{debug, info};

use crate::actor::link::PendingLink;

/// How long a running analyzer, which holds gigabytes, is kept for the next edit.
const ANALYZER_IDLE: Duration = Duration::from_secs(600);

/// How long a query waits for semantic resolution. A running analyzer is usually done by then;
/// a new index or a first load is not, and the query goes on with syntax rules.
const QUERY_WAIT: Duration = Duration::from_millis(500);

/// Tracked file info.
#[derive(Clone)]
struct TrackedFile {
    file_id: FileId,
    content_hash: ContentHash,
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
    /// Create a graph actor with its adapter registry and semantic source.
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
    pub async fn run(mut self, mut commands: CommandReceiver) -> Result<(), GraphError> {
        info!("Graph actor starting for root: {}", self.root.display());

        while let Some(cmd) = next_command(&mut commands, self.semantics.as_ref()).await {
            match cmd {
                GraphCommand::AddFile {
                    path,
                    content,
                    language,
                    reply,
                } => {
                    let result = self.handle_add_file(path.clone(), content, language);
                    answer(reply, result);
                }

                GraphCommand::RemoveFile { file_id, reply } => {
                    let result = self.handle_remove_file(file_id);
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
                    let result = self.handle_sync_complete(full).await;
                    answer(reply, result);
                }

                GraphCommand::Edit {
                    edits,
                    preview,
                    backup,
                    reply,
                } => {
                    let result = self.handle_edit(&edits, preview, backup).await;
                    answer(reply, result);
                }

                GraphCommand::Stats { reply } => {
                    self.ensure_linked().await;
                    let response = self.handle_stats();
                    answer(reply, response);
                }

                GraphCommand::Sync { reply } => {
                    let result = self.handle_sync().await;
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

    /// Where the index cache and the saved graph live.
    fn graph_dir(&self) -> PathBuf {
        self.root.join(".mimir").join("graph")
    }

    fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root).unwrap_or(path).to_path_buf()
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
