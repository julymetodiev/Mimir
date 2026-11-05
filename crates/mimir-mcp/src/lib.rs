//! The MCP server: each tool parses its arguments, makes one client call and returns the answer as
//! compact JSON (see `view`). The client finds the project from the directory the server runs in, and starts the daemon.

mod arguments;
mod view;

use arguments::{
    Direction, EditLinesArguments, EditSymbolsArguments, FilesArguments, GrepArguments,
    ImpactArguments, InsertAfterArguments, ReadArguments, ReadFileArguments, ReplaceTextArguments,
    SearchArguments, SyncArguments, TraceArguments, WriteFileArguments,
};
use mimir_client::Client;
use mimir_protocol::{Edit, EditChange, EditTarget, GrepOptions, SearchOptions, TraceOptions};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::transport::stdio;
use rmcp::{ServiceExt, tool, tool_handler, tool_router};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

/// Why the server could not run.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("cannot start the MCP server: {0}")]
    Start(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("the MCP server failed: {0}")]
    Serve(#[source] tokio::task::JoinError),
}

/// What the server tells a model about itself.
const INSTRUCTIONS: &str = "Mimir knows the call graph of this project, from its language's compiler. Use \
impact after changing code, to see what depends on the change. Use \
trace before changing a function, to see who calls it and what it calls: it finds callers grep \
misses and does not confuse same-named methods. An entry marked possible (via trait, \
implementation, as value) may run at runtime or not. An entry marked not_a_call reads or uses the \
symbol and calls nothing. A result with provisional true is still \
being resolved: ask again shortly. Use grep instead of searching text with other tools: each match says which symbol it is in. \
Use files to find files and read_file to read them. Use read and search to find code by symbol, \
edit_symbols to replace whole symbols, insert_after to add code, replace_text for small changes and write_file to create a file.";

/// Serve MCP on stdin and stdout until the client disconnects.
pub async fn serve(from: PathBuf) -> Result<(), McpError> {
    let server = MimirServer::new(from);
    let running = server
        .serve(stdio())
        .await
        .map_err(|e| McpError::Start(Box::new(e)))?;
    running.waiting().await.map_err(McpError::Serve)?;
    Ok(())
}

/// The server. One client, opened at the first call so that starting up costs nothing.
#[derive(Clone)]
struct MimirServer {
    from: PathBuf,
    client: Arc<Mutex<Option<Client>>>,
}

impl MimirServer {
    fn new(from: PathBuf) -> Self {
        Self {
            from,
            client: Arc::default(),
        }
    }

    /// Run a call on the client, opening it if this is the first, and give back JSON.
    async fn with_client<T, F>(&self, call: F) -> Result<String, String>
    where
        T: Serialize,
        F: AsyncFnOnce(&mut Client) -> Result<T, mimir_client::ClientError>,
    {
        let mut slot = self.client.lock().await;
        if slot.is_none() {
            let opened = Client::open(&self.from).await.map_err(|e| e.to_string())?;
            *slot = Some(opened);
        }
        let client = slot.as_mut().ok_or("no client")?;
        let answer = call(client).await.map_err(|e| e.to_string())?;
        serde_json::to_string(&answer).map_err(|e| e.to_string())
    }
}

#[tool_router]
impl MimirServer {
    #[tool(
        description = "Who calls a symbol and what it calls, across the project. Use before changing a function."
    )]
    async fn trace(
        &self,
        Parameters(arguments): Parameters<TraceArguments>,
    ) -> Result<String, String> {
        let options = TraceOptions {
            max_depth: Some(arguments.depth.unwrap_or(1)),
            callers_only: matches!(arguments.direction, Direction::Callers),
            callees_only: matches!(arguments.direction, Direction::Callees),
            file: arguments.file,
        };
        self.with_client(async |client| {
            let trace = client.trace(&arguments.symbol, options).await?;
            Ok(view::Trace::from(trace))
        })
        .await
    }

    #[tool(
        description = "What the uncommitted changes touch, or all of a branch with base: the symbols added, changed and removed, and what depends on them. Use after changing code, to see what else to check."
    )]
    async fn impact(
        &self,
        Parameters(arguments): Parameters<ImpactArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| {
            let impact = client.impact(arguments.base, arguments.depth).await?;
            Ok(view::Impact::from(impact))
        })
        .await
    }

    #[tool(description = "Find symbols by a regular expression over their names.")]
    async fn search(
        &self,
        Parameters(arguments): Parameters<SearchArguments>,
    ) -> Result<String, String> {
        let options = SearchOptions {
            pattern: arguments.pattern,
            kind_filter: arguments.kind,
            limit: arguments.limit,
        };
        self.with_client(async |client| Ok(view::Search::from(client.search(options).await?)))
            .await
    }

    #[tool(
        description = "Search the text of the project's files in any language, as grep does, skipping what git ignores. Each match says which symbol it is in. Use this instead of grep."
    )]
    async fn grep(
        &self,
        Parameters(arguments): Parameters<GrepArguments>,
    ) -> Result<String, String> {
        let options = GrepOptions {
            pattern: arguments.pattern,
            literal: arguments.literal,
            ignore_case: arguments.ignore_case,
            path: arguments.path,
            context: arguments.context,
            limit: arguments.limit,
        };
        self.with_client(async |client| Ok(view::Grep::from(client.grep(options).await?)))
            .await
    }

    #[tool(
        description = "Find files of the project by a glob, skipping what git ignores. Use this instead of other file searches."
    )]
    async fn files(
        &self,
        Parameters(arguments): Parameters<FilesArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| {
            let found = client.files(arguments.glob, arguments.limit).await?;
            Ok(view::Files::from(found))
        })
        .await
    }

    #[tool(
        description = "Read a file of the project, or a range of its lines, with line numbers for edit_lines. For one symbol, read is shorter."
    )]
    async fn read_file(
        &self,
        Parameters(arguments): Parameters<ReadFileArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| {
            let text = client
                .read_file(arguments.file, arguments.first, arguments.last)
                .await?;
            Ok(view::Text::from(text))
        })
        .await
    }

    #[tool(description = "The source of a symbol by name, with its file and lines.")]
    async fn read(
        &self,
        Parameters(arguments): Parameters<ReadArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| {
            let source = client
                .read(
                    &arguments.symbol,
                    arguments.file,
                    arguments.before,
                    arguments.after,
                )
                .await?;
            Ok(view::Source::from(source))
        })
        .await
    }

    #[tool(
        description = "Replace whole symbols by name, together. Fails rather than change the wrong lines if a file moved on."
    )]
    async fn edit_symbols(
        &self,
        Parameters(arguments): Parameters<EditSymbolsArguments>,
    ) -> Result<String, String> {
        let edits = arguments
            .edits
            .into_iter()
            .map(|edit| Edit {
                target: EditTarget::Symbol {
                    name: edit.symbol,
                    file: edit.file,
                },
                change: EditChange::WholeSymbol {
                    content: edit.content,
                },
            })
            .collect();
        self.with_client(async |client| client.edit(edits, arguments.preview, false).await)
            .await
    }

    #[tool(
        description = "Add new code right after a symbol: a new function after another, a method in an impl, a variant in an enum, a field in a struct."
    )]
    async fn insert_after(
        &self,
        Parameters(arguments): Parameters<InsertAfterArguments>,
    ) -> Result<String, String> {
        let edit = Edit {
            target: EditTarget::Symbol {
                name: arguments.symbol,
                file: arguments.file,
            },
            change: EditChange::AfterSymbol {
                content: arguments.content,
            },
        };
        self.with_client(async |client| client.edit(vec![edit], arguments.preview, false).await)
            .await
    }

    #[tool(description = "Replace a range of lines in a file of the project.")]
    async fn edit_lines(
        &self,
        Parameters(arguments): Parameters<EditLinesArguments>,
    ) -> Result<String, String> {
        let edit = Edit {
            target: EditTarget::File(arguments.file),
            change: EditChange::Lines {
                start: arguments.start,
                end: arguments.end,
                content: arguments.content,
            },
        };
        self.with_client(async |client| client.edit(vec![edit], arguments.preview, false).await)
            .await
    }

    #[tool(
        description = "Replace exact text in files of the project, in any language, without line numbers. Text that is not there or occurs several times is refused, naming the lines. Use this instead of other edit tools for a small change."
    )]
    async fn replace_text(
        &self,
        Parameters(arguments): Parameters<ReplaceTextArguments>,
    ) -> Result<String, String> {
        let edits = arguments
            .edits
            .into_iter()
            .map(|edit| Edit {
                target: EditTarget::File(edit.file),
                change: EditChange::Text {
                    old: edit.old,
                    new: edit.new,
                    all: edit.all,
                },
            })
            .collect();
        self.with_client(async |client| client.edit(edits, arguments.preview, false).await)
            .await
    }

    #[tool(
        description = "Create a new file of the project, in any language, with its directories. An existing file is refused unless overwrite is true. To change part of a file, use replace_text or edit_symbols."
    )]
    async fn write_file(
        &self,
        Parameters(arguments): Parameters<WriteFileArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| {
            client
                .write_file(arguments.file, arguments.content, arguments.overwrite)
                .await
        })
        .await
    }

    #[tool(
        description = "Bring the graph up to date and wait until nothing is provisional. Rarely needed: changes are followed by themselves. A first index can take minutes."
    )]
    async fn sync(
        &self,
        Parameters(arguments): Parameters<SyncArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| client.sync(arguments.full, false).await)
            .await
    }

    #[tool(description = "How many files, symbols and edges the graph holds.")]
    async fn status(&self) -> Result<String, String> {
        self.with_client(async |client| Ok(view::Status::from(client.stats().await?)))
            .await
    }
}

#[tool_handler]
impl rmcp::ServerHandler for MimirServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mimir", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
