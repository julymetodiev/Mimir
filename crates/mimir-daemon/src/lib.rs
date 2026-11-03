//! The daemon: one per project, a socket server over the engine. Each request maps to one engine
//! call. It stops when no request has come for a while; the next client starts it again.

use mimir_engine::GraphHandle;
use mimir_protocol::{IpcCommand, IpcData, IpcRequest, IpcResponse, IpcResult};
use mimir_resolve::SemanticSource;
use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

/// How long a daemon that was told to go gets to let go of the project.
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// Why the daemon could not run.
#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("{action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot encode a response: {0}")]
    Encode(#[from] serde_json::Error),
}

fn io_error(action: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> DaemonError {
    let path = path.to_path_buf();
    move |source| DaemonError::Io {
        action,
        path,
        source,
    }
}

/// Time of the last request, shared by the connections and the idle timer.
struct LastUsed {
    started: Instant,
    millis: AtomicU64,
}

impl LastUsed {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            millis: AtomicU64::new(0),
        }
    }

    fn touch(&self) {
        let now = self.started.elapsed().as_millis() as u64;
        self.millis.store(now, Ordering::Relaxed);
    }

    fn idle(&self) -> Duration {
        let now = self.started.elapsed().as_millis() as u64;
        Duration::from_millis(now.saturating_sub(self.millis.load(Ordering::Relaxed)))
    }
}

/// Serve `root` until told to stop or idle for `idle`. If another daemon serves the project, return:
/// a project has one.
pub async fn run(
    root: PathBuf,
    idle: Duration,
    semantics: Arc<dyn SemanticSource>,
) -> Result<(), DaemonError> {
    let agent_dir = root.join(".mimir");
    fs::create_dir_all(agent_dir.join("graph")).map_err(io_error("create", &agent_dir))?;

    let lock_path = agent_dir.join("daemon.lock");
    let lock = File::create(&lock_path).map_err(io_error("create", &lock_path))?;
    let socket = mimir_protocol::socket_path(&root).map_err(io_error("prepare socket", &root))?;
    let started = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(TryLockError::Error(source)) => return Err(io_error("lock", &lock_path)(source)),
            // A daemon that answers serves this project. One that does not is on its way out,
            // having just been told to give way to this one.
            Err(TryLockError::WouldBlock) => {
                if UnixStream::connect(&socket).await.is_ok() || started.elapsed() > LOCK_WAIT {
                    info!("Another daemon serves {}", root.display());
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

    let identity: Arc<str> = mimir_protocol::build_identity().into();
    if socket.exists() {
        fs::remove_file(&socket).map_err(io_error("remove", &socket))?;
    }

    // The first scan is queued before anyone connects, so every query waits for it in the actor.
    let engine = mimir_engine::start(root.clone(), semantics).await;
    let listener = UnixListener::bind(&socket).map_err(io_error("bind", &socket))?;
    info!("Serving {} on {}", root.display(), socket.display());

    let last_used = Arc::new(LastUsed::new());
    let (shutdown, mut stop) = broadcast::channel(1);
    let mut tick = tokio::time::interval(
        idle.min(Duration::from_secs(60))
            .max(Duration::from_millis(50)),
    );
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let (handle, shutdown, last_used, identity) = (
                        engine.handle().clone(),
                        shutdown.clone(),
                        last_used.clone(),
                        identity.clone(),
                    );
                    tokio::spawn(async move {
                        if let Err(e) = serve(stream, handle, shutdown, last_used, identity).await {
                            error!("Connection failed: {e}");
                        }
                    });
                }
                Err(e) => error!("Accept failed: {e}"),
            },
            _ = tick.tick() => {
                if last_used.idle() >= idle {
                    info!("Idle for {idle:?}, stopping");
                    break;
                }
            }
            _ = stop.recv() => break,
        }
    }

    if let Err(e) = fs::remove_file(&socket) {
        warn!("Cannot remove {}: {e}", socket.display());
    }
    engine.stop().await;
    info!("Stopped");
    Ok(())
}

/// Answer one client until it disconnects.
async fn serve(
    stream: UnixStream,
    handle: GraphHandle,
    shutdown: broadcast::Sender<()>,
    last_used: Arc<LastUsed>,
    identity: Arc<str>,
) -> Result<(), DaemonError> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).await;
        let read = read.map_err(io_error("read", Path::new("socket")))?;
        if read == 0 {
            return Ok(());
        }
        last_used.touch();

        let (response, stopping) = match serde_json::from_str::<IpcRequest>(&line) {
            Ok(request) => {
                let stopping = matches!(request.command, IpcCommand::Shutdown);
                (answer(request, &handle, &identity).await, stopping)
            }
            Err(e) => (IpcResponse::err(0, format!("Invalid request: {e}")), false),
        };
        last_used.touch();

        let encoded = serde_json::to_string(&response)? + "\n";
        writer
            .write_all(encoded.as_bytes())
            .await
            .map_err(io_error("write", Path::new("socket")))?;
        if stopping {
            // Nobody listening means the daemon is already going down.
            if shutdown.send(()).is_err() {
                info!("Already stopping");
            }
            return Ok(());
        }
    }
}

/// Map a request to its engine call and the result back.
async fn answer(request: IpcRequest, handle: &GraphHandle, identity: &str) -> IpcResponse {
    let result: Result<IpcData, _> = match request.command {
        IpcCommand::Ping => Ok(IpcData::Pong {
            version: identity.to_string(),
        }),
        IpcCommand::FindSymbol { name, options } => handle
            .find_symbol_with_options(&name, options)
            .await
            .map(IpcData::FindResult),
        IpcCommand::TraceByName { name, options } => handle
            .trace_by_name(name, options)
            .await
            .map(IpcData::TraceResult),
        IpcCommand::Read {
            name,
            file,
            before,
            after,
        } => handle
            .read(name, file, before, after)
            .await
            .map(IpcData::Source),
        IpcCommand::Source { symbol_id, lines } => {
            handle.source(symbol_id, lines).await.map(IpcData::Source)
        }
        IpcCommand::Edit {
            edits,
            preview,
            backup,
        } => handle
            .edit(edits, preview, backup)
            .await
            .map(IpcData::Edited),
        IpcCommand::Stats => handle.stats().await.map(IpcData::Stats),
        IpcCommand::Sync { full } => handle.sync_complete(full).await.map(IpcData::SyncResult),
        IpcCommand::Search { options } => handle.search(options).await.map(IpcData::SearchResult),
        IpcCommand::Shutdown => Ok(IpcData::ShutdownAck),
    };
    IpcResponse::new(
        request.id,
        match result {
            Ok(data) => IpcResult::Ok(data),
            Err(e) => IpcResult::Err(e.to_string()),
        },
    )
}
