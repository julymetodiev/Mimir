//! The MCP server: each tool parses its arguments, makes one client call and returns the answer as
//! compact JSON (see [`view`]). The client finds the project from the directory the server runs in, and starts the daemon.

mod view;

use mimir_client::Client;
use mimir_protocol::{Edit, EditChange, EditTarget, SearchOptions, TraceOptions};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::transport::stdio;
use rmcp::{ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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
const INSTRUCTIONS: &str = "Mimir knows the call graph of this project, from rust-analyzer. Use \
trace before changing a function, to see who calls it and what it calls: it finds callers grep \
misses and does not confuse same-named methods. An entry marked possible (via trait, \
implementation, as value) may run at runtime or not. A result with provisional true is still \
being resolved: ask again shortly. Use read and search to find code by symbol, and edit_symbols \
to replace whole symbols.";

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

/// The direction of a trace.
#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Direction {
    /// Who calls it and what it calls.
    #[default]
    Both,
    /// Only who calls it.
    Callers,
    /// Only what it calls.
    Callees,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TraceArguments {
    /// The symbol's name.
    symbol: String,
    /// Only a symbol in this file, by absolute path, when several share the name.
    file: Option<PathBuf>,
    /// How many levels to follow. The default is 1.
    depth: Option<u32>,
    /// Which way to follow. The default is both.
    #[serde(default)]
    direction: Direction,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchArguments {
    /// A regular expression over symbol names and qualified names.
    pattern: String,
    /// Only this kind of symbol: function (methods too), method, struct, enum and so on.
    kind: Option<String>,
    /// The most results to return. The default is 20.
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadArguments {
    /// The symbol's name.
    symbol: String,
    /// Lines of context before it.
    #[serde(default)]
    before: u32,
    /// Lines of context after it.
    #[serde(default)]
    after: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SymbolEdit {
    /// The symbol's name.
    symbol: String,
    /// Its whole new source. Doc comments and attributes above it are not part of the symbol.
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditSymbolsArguments {
    /// The symbols to replace, applied together.
    edits: Vec<SymbolEdit>,
    /// Only show what would change.
    #[serde(default)]
    preview: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditLinesArguments {
    /// The file, by absolute path or relative to the project.
    file: PathBuf,
    /// The first line to replace, from 1.
    start: u32,
    /// The last line to replace, inclusive.
    end: u32,
    /// The new text.
    content: String,
    /// Only show what would change.
    #[serde(default)]
    preview: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SyncArguments {
    /// Write a new semantic index instead of asking a running analyzer about edited files.
    #[serde(default)]
    full: bool,
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

    #[tool(description = "The source of a symbol by name, with its file and lines.")]
    async fn read(
        &self,
        Parameters(arguments): Parameters<ReadArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| {
            let source = client
                .read(&arguments.symbol, arguments.before, arguments.after)
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
                target: EditTarget::Symbol(edit.symbol),
                change: EditChange::WholeSymbol {
                    content: edit.content,
                },
            })
            .collect();
        self.with_client(async |client| client.edit(edits, arguments.preview, false).await)
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
        description = "Bring the graph up to date and wait until nothing is provisional. Rarely needed: changes are followed by themselves. A first index can take minutes."
    )]
    async fn sync(
        &self,
        Parameters(arguments): Parameters<SyncArguments>,
    ) -> Result<String, String> {
        self.with_client(async |client| client.sync(arguments.full).await)
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
