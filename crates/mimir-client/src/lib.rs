//! The one way to the daemon, for the CLI and MCP alike. Opening a client finds the project and
//! starts the daemon when it is not running; a daemon that went away is started again.

use mimir_protocol::{
    Edit, EditReport, FileText, FilesResponse, FindOptions, FindResponse, GrepOptions,
    GrepResponse, IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult, SearchOptions,
    SearchResponse, StatsResponse, SymbolSource, SyncResult, TraceOptions, TraceResponse,
};
use mimir_types::SymbolId;
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// The name of Mimir's directory in a project.
const MIMIR_DIR: &str = ".mimir";
/// How long a daemon that was just started gets to listen on its socket.
const START_WAIT: Duration = Duration::from_secs(30);

/// What can go wrong talking to the daemon.
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("{0} is not in a project: no .mimir or .git here or above. Run `mimir init`.")]
    NoProject(PathBuf),
    #[error("{action}: {source}")]
    Io {
        action: &'static str,
        source: std::io::Error,
    },
    #[error("the daemon did not start within {waited:?}; see {log}")]
    StartFailed { waited: Duration, log: PathBuf },
    #[error("the daemon closed the connection")]
    Disconnected,
    #[error("cannot read the daemon's message: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("the daemon answered with another kind of response")]
    UnexpectedResponse,
    #[error("{0}")]
    Daemon(String),
}

fn io_error(action: &'static str) -> impl FnOnce(std::io::Error) -> ClientError {
    move |source| ClientError::Io { action, source }
}

/// The nearest directory at or above `from` with `.mimir` or `.git`.
pub fn find_root(from: &Path) -> Result<PathBuf, ClientError> {
    let start = from
        .canonicalize()
        .map_err(io_error("read the directory"))?;
    start
        .ancestors()
        .find(|dir| dir.join(MIMIR_DIR).exists() || dir.join(".git").exists())
        .map(Path::to_path_buf)
        .ok_or(ClientError::NoProject(start))
}

/// Create Mimir's directory in `root`, ignored by git. Doing it again changes nothing.
pub fn init(root: &Path) -> Result<PathBuf, ClientError> {
    let dir = root.join(MIMIR_DIR);
    fs::create_dir_all(dir.join("graph")).map_err(io_error("create .mimir"))?;
    fs::write(dir.join(".gitignore"), "*\n").map_err(io_error("write .mimir/.gitignore"))?;
    Ok(dir)
}

/// A connection to a project's daemon.
pub struct Client {
    root: PathBuf,
    socket: PathBuf,
    stream: UnixStream,
    next_id: u64,
}

impl Client {
    /// Connect to the daemon of the project at or above `from`, starting it if needed.
    pub async fn open(from: &Path) -> Result<Self, ClientError> {
        let root = find_root(from)?;
        init(&root)?;
        let socket = socket_of(&root)?;
        let stream = connect_or_start(&root, &socket).await?;
        let mut client = Self {
            root,
            socket,
            stream,
            next_id: 1,
        };
        client.replace_other_version().await?;
        Ok(client)
    }

    /// Connect only if the daemon of the project at or above `from` is running.
    pub async fn running(from: &Path) -> Result<Option<Self>, ClientError> {
        let root = find_root(from)?;
        if !root.join(MIMIR_DIR).exists() {
            return Ok(None);
        }
        let socket = socket_of(&root)?;
        Ok(UnixStream::connect(&socket).await.ok().map(|stream| Self {
            root,
            socket,
            stream,
            next_id: 1,
        }))
    }

    /// The project's root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A daemon left from another build speaks another protocol, so it gives way.
    async fn replace_other_version(&mut self) -> Result<(), ClientError> {
        let answer = self.exchange(&IpcCommand::Ping).await;
        let ours = mimir_protocol::build_identity();
        if matches!(&answer, Ok(IpcData::Pong { version }) if *version == ours) {
            return Ok(());
        }
        // Either answer ends the old daemon; an error here means it is already gone.
        if self.exchange(&IpcCommand::Shutdown).await.is_ok() {
            wait_until_gone(&self.socket).await;
        }
        self.stream = connect_or_start(&self.root, &self.socket).await?;
        Ok(())
    }

    /// Send a command, once more through a new daemon if the connection is gone.
    async fn call(&mut self, command: IpcCommand) -> Result<IpcData, ClientError> {
        match self.exchange(&command).await {
            Err(ClientError::Disconnected | ClientError::Io { .. }) => {
                self.stream = connect_or_start(&self.root, &self.socket).await?;
                self.exchange(&command).await
            }
            other => other,
        }
    }

    async fn exchange(&mut self, command: &IpcCommand) -> Result<IpcData, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = IpcRequest::new(id, command.clone());
        let encoded = serde_json::to_string(&request)? + "\n";
        self.stream
            .write_all(encoded.as_bytes())
            .await
            .map_err(io_error("write to the daemon"))?;

        let mut line = String::new();
        let read = BufReader::new(&mut self.stream)
            .read_line(&mut line)
            .await
            .map_err(io_error("read from the daemon"))?;
        if read == 0 {
            return Err(ClientError::Disconnected);
        }
        let response: IpcResponse = serde_json::from_str(&line)?;
        match response.result {
            IpcResult::Ok(data) => Ok(data),
            IpcResult::Err(message) => Err(ClientError::Daemon(message)),
        }
    }

    /// Find symbols by name.
    pub async fn find_symbol(
        &mut self,
        name: &str,
        options: FindOptions,
    ) -> Result<FindResponse, ClientError> {
        let command = IpcCommand::FindSymbol {
            name: name.to_string(),
            options,
        };
        match self.call(command).await? {
            IpcData::FindResult(found) => Ok(found),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Trace a symbol by name.
    pub async fn trace(
        &mut self,
        name: &str,
        options: TraceOptions,
    ) -> Result<TraceResponse, ClientError> {
        let command = IpcCommand::TraceByName {
            name: name.to_string(),
            options,
        };
        match self.call(command).await? {
            IpcData::TraceResult(trace) => Ok(trace),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// The source of the symbol a name most likely means, with lines of context around it.
    pub async fn read(
        &mut self,
        name: &str,
        file: Option<PathBuf>,
        before: u32,
        after: u32,
    ) -> Result<SymbolSource, ClientError> {
        let command = IpcCommand::Read {
            name: name.to_string(),
            file,
            before,
            after,
        };
        match self.call(command).await? {
            IpcData::Source(source) => Ok(source),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// The first `lines` lines of a symbol found earlier, or all of them.
    pub async fn source(
        &mut self,
        symbol_id: SymbolId,
        lines: Option<u32>,
    ) -> Result<SymbolSource, ClientError> {
        match self.call(IpcCommand::Source { symbol_id, lines }).await? {
            IpcData::Source(source) => Ok(source),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Search symbols by pattern.
    pub async fn search(&mut self, options: SearchOptions) -> Result<SearchResponse, ClientError> {
        match self.call(IpcCommand::Search { options }).await? {
            IpcData::SearchResult(found) => Ok(found),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Search the text of the project's files.
    pub async fn grep(&mut self, options: GrepOptions) -> Result<GrepResponse, ClientError> {
        match self.call(IpcCommand::Grep { options }).await? {
            IpcData::GrepResult(found) => Ok(found),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// The project's files, only those matching a glob if given.
    pub async fn files(
        &mut self,
        glob: Option<String>,
        limit: Option<u32>,
    ) -> Result<FilesResponse, ClientError> {
        match self.call(IpcCommand::Files { glob, limit }).await? {
            IpcData::FilesResult(found) => Ok(found),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Lines of a file of the project.
    pub async fn read_file(
        &mut self,
        path: PathBuf,
        first: Option<u32>,
        last: Option<u32>,
    ) -> Result<FileText, ClientError> {
        match self
            .call(IpcCommand::ReadFile { path, first, last })
            .await?
        {
            IpcData::FileText(text) => Ok(text),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Write a whole file of the project.
    pub async fn write_file(
        &mut self,
        path: PathBuf,
        content: String,
        overwrite: bool,
    ) -> Result<EditReport, ClientError> {
        let command = IpcCommand::WriteFile {
            path,
            content,
            overwrite,
        };
        match self.call(command).await? {
            IpcData::Edited(report) => Ok(report),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Apply edits together, or with `preview` only show them.
    pub async fn edit(
        &mut self,
        edits: Vec<Edit>,
        preview: bool,
        backup: bool,
    ) -> Result<EditReport, ClientError> {
        let command = IpcCommand::Edit {
            edits,
            preview,
            backup,
        };
        match self.call(command).await? {
            IpcData::Edited(report) => Ok(report),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Graph statistics.
    pub async fn stats(&mut self) -> Result<StatsResponse, ClientError> {
        match self.call(IpcCommand::Stats).await? {
            IpcData::Stats(stats) => Ok(stats),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Bring the graph up to date and wait until nothing about it is provisional.
    pub async fn sync(&mut self, full: bool) -> Result<SyncResult, ClientError> {
        match self.call(IpcCommand::Sync { full }).await? {
            IpcData::SyncResult(result) => Ok(result),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Stop the daemon and wait until it is gone.
    pub async fn shutdown(mut self) -> Result<(), ClientError> {
        self.exchange(&IpcCommand::Shutdown).await?;
        wait_until_gone(&self.socket).await;
        Ok(())
    }
}

fn socket_of(root: &Path) -> Result<PathBuf, ClientError> {
    mimir_protocol::socket_path(root).map_err(io_error("prepare the daemon's socket"))
}

async fn wait_until_gone(socket: &Path) {
    let started = Instant::now();
    while socket.exists() && started.elapsed() < START_WAIT {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn connect_or_start(root: &Path, socket: &Path) -> Result<UnixStream, ClientError> {
    if let Ok(stream) = UnixStream::connect(socket).await {
        return Ok(stream);
    }
    start_daemon(root)?;
    let started = Instant::now();
    loop {
        if let Ok(stream) = UnixStream::connect(socket).await {
            return Ok(stream);
        }
        if started.elapsed() > START_WAIT {
            return Err(ClientError::StartFailed {
                waited: START_WAIT,
                log: root.join(MIMIR_DIR).join("daemon.log"),
            });
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Start this same binary as the project's daemon, detached from this process.
fn start_daemon(root: &Path) -> Result<(), ClientError> {
    let exe = std::env::current_exe().map_err(io_error("find this program"))?;
    let log = File::create(root.join(MIMIR_DIR).join("daemon.log"))
        .map_err(io_error("create the daemon's log"))?;
    let errors = log.try_clone().map_err(io_error("open the daemon's log"))?;
    Command::new(exe)
        .args(["daemon", "run"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .process_group(0)
        .spawn()
        .map_err(io_error("start the daemon"))?;
    Ok(())
}
