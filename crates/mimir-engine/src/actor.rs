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
mod view;

use crate::channel::CommandReceiver;
use crate::command::GraphCommand;
use crate::error::GraphError;
use crate::project_files;
use mimir_graph::CodeGraph;
use mimir_parse_core::AdapterRegistry;
use mimir_protocol::{ActivityKind, GraphEvent, SymbolInfo};
use mimir_resolve::{ProjectFile, SemanticIndex, SemanticSource};
use mimir_types::{ContentHash, FileId, IdGenerator, Symbol};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};
use tracing::{debug, info};

use crate::actor::link::{PendingLink, WaitingSync};

/// How many files a listing returns unless asked otherwise.
const DEFAULT_FILES_LIMIT: u32 = 500;

/// How long a running analyzer, which holds gigabytes, is kept for the next edit.
const ANALYZER_IDLE: Duration = Duration::from_secs(600);

/// How long a query waits for semantic resolution. A running analyzer is usually done by then;
/// a new index or a first load is not, and the query goes on with syntax rules.
const QUERY_WAIT: Duration = Duration::from_millis(500);

/// How many events a watcher may fall behind by before it is told that it did.
const EVENTS_KEPT: usize = 256;

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
    /// What the engine announces, to whoever watches.
    events: broadcast::Sender<GraphEvent>,
    /// Rises each time the edges are resolved anew.
    generation: u64,
    /// A watcher was told of the files changed since edges were last resolved.
    announced_change: bool,
    /// Syncs answered when `pending` ends.
    waiting_syncs: Vec<WaitingSync>,
    query_wait: Duration,
}

/// Send a reply. An asker that went away no longer wants it, which is no failure.
fn answer<T>(reply: oneshot::Sender<T>, value: T) {
    if reply.send(value).is_err() {
        debug!("The asker left before the answer");
    }
}

/// What the actor does next.
enum Next {
    Command(Option<GraphCommand>),
    /// The semantic resolution a sync waits for has ended.
    LinkEnded(Result<Vec<SemanticIndex>, tokio::task::JoinError>),
}

/// The next thing to do. Waiting for it is also when idle analyzers stop.
async fn next(
    commands: &mut CommandReceiver,
    semantics: &dyn SemanticSource,
    waited_for: Option<&mut PendingLink>,
) -> Next {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    let ended = async {
        match waited_for {
            Some(pending) => (&mut pending.indexes).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(ended);
    loop {
        tokio::select! {
            command = commands.recv() => return Next::Command(command),
            ended = &mut ended => return Next::LinkEnded(ended),
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
            events: broadcast::channel(EVENTS_KEPT).0,
            generation: 0,
            announced_change: false,
            waiting_syncs: Vec::new(),
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

        loop {
            let waited_for = self
                .pending
                .as_mut()
                .filter(|_| !self.waiting_syncs.is_empty() || self.events.receiver_count() > 0);
            let cmd = match next(&mut commands, self.semantics.as_ref(), waited_for).await {
                Next::Command(Some(cmd)) => cmd,
                Next::Command(None) => break,
                Next::LinkEnded(indexes) => {
                    self.finish_link(indexes);
                    continue;
                }
            };
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

                GraphCommand::RemovePath { path } => {
                    self.handle_remove_path(&path);
                }

                GraphCommand::Snapshot { reply } => {
                    self.ensure_linked().await;
                    answer(reply, self.handle_snapshot());
                }

                GraphCommand::Watch { reply } => {
                    answer(reply, self.events.subscribe());
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
                    if let Ok(trace) = &response {
                        let around = trace.callers.iter().chain(&trace.callees);
                        let touched = std::iter::once(trace.root.id)
                            .chain(around.map(|node| node.symbol.id))
                            .collect();
                        self.announce_activity(touched, ActivityKind::Read);
                    }
                    answer(reply, response);
                }

                GraphCommand::Read {
                    name,
                    file,
                    before,
                    after,
                    reply,
                } => {
                    let response = self.handle_read(&name, file.as_deref(), before, after);
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
                    self.handle_sync_complete(full, reply).await;
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

                GraphCommand::Grep { options, reply } => {
                    let response = self.handle_grep(options).await;
                    answer(reply, response);
                }

                GraphCommand::Files { glob, limit, reply } => {
                    let limit = limit.unwrap_or(DEFAULT_FILES_LIMIT) as usize;
                    answer(
                        reply,
                        project_files::list(&self.root, glob.as_deref(), limit),
                    );
                }

                GraphCommand::ReadFile {
                    path,
                    first,
                    last,
                    reply,
                } => {
                    answer(reply, project_files::read(&self.root, &path, first, last));
                }

                GraphCommand::WriteFile {
                    path,
                    content,
                    overwrite,
                    reply,
                } => {
                    let result = self.handle_write_file(&path, &content, overwrite).await;
                    answer(reply, result);
                }

                GraphCommand::Shutdown => {
                    info!("Graph actor shutting down");
                    break;
                }
            }
            self.announce_changed_files();
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
