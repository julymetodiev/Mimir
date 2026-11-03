//! One running language server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mimir_types::ContentHash;
use serde_json::{Value, json};
use tracing::debug;

use crate::wire::{Reader, path_of, uri_of, write_message};
use crate::{Location, LspError, ServerSpec};

const LOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// The first question about a package waits for that package's analysis.
const QUERY_TIMEOUT: Duration = Duration::from_secs(300);
/// The server is still catching up with an edit.
const CONTENT_MODIFIED: i64 = -32801;
const CANCELLED: i64 = -32802;
const ATTEMPTS: usize = 6;
/// How many questions are out at once.
const BATCH: usize = 32;

/// One language server. Dropping it ends the process.
pub struct Server {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    answers: Receiver<Value>,
    status: Receiver<Value>,
    next_id: u64,
    capabilities: Value,
    /// What the server was told each open file holds, and its version.
    open: HashMap<PathBuf, (ContentHash, i64)>,
}

impl Server {
    /// Start a server at `root` and shake hands with it.
    pub fn start(spec: ServerSpec, root: &Path) -> Result<Self, LspError> {
        let root = root.canonicalize().map_err(|source| LspError::Root {
            path: root.to_path_buf(),
            source,
        })?;
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| LspError::Start {
                program: spec.program.clone(),
                source,
            })?;
        let no_pipe = || LspError::NoPipe {
            program: spec.program.clone(),
        };
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or_else(no_pipe)?));
        let stdout = child.stdout.take().ok_or_else(no_pipe)?;
        let (answers_tx, answers) = channel();
        let (status_tx, status) = channel();
        let writer = stdin.clone();
        let reader = Reader {
            configuration: spec.configuration.clone(),
            status_method: spec.status_method,
            answers: answers_tx,
            status: status_tx,
        };
        std::thread::spawn(move || reader.run(stdout, &writer));

        let mut server = Self {
            child,
            stdin,
            answers,
            status,
            next_id: 0,
            capabilities: Value::Null,
            open: HashMap::new(),
        };
        let uri = uri_of(&root);
        let init = server.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": uri,
                "workspaceFolders": [{"uri": uri, "name": "project"}],
                "initializationOptions": spec.configuration,
                "capabilities": spec.capabilities,
            }),
            LOAD_TIMEOUT,
        )?;
        server.capabilities = init["capabilities"].clone();
        server.notify("initialized", json!({}))?;
        Ok(server)
    }

    /// What the server said it can do.
    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

    /// The next state it reported, waiting up to `timeout`. `None` if none came.
    pub fn next_status(&self, timeout: Duration) -> Option<Value> {
        self.status.recv_timeout(timeout).ok()
    }

    /// Tell the server what a file holds now.
    pub fn update(&mut self, path: &Path, language_id: &str, text: &str) -> Result<(), LspError> {
        let hash = ContentHash::compute(text.as_bytes());
        let uri = uri_of(path);
        match self.open.get(path).copied() {
            Some((known, _)) if known == hash => Ok(()),
            Some((_, version)) => {
                self.open.insert(path.to_path_buf(), (hash, version + 1));
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {"uri": uri, "version": version + 1},
                        "contentChanges": [{"text": text}],
                    }),
                )
            }
            None => {
                self.open.insert(path.to_path_buf(), (hash, 1));
                self.notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": language_id,
                            "version": 1,
                            "text": text,
                        },
                    }),
                )
            }
        }
    }

    /// Say that a file is no longer open: the server reads it from disk from now on.
    pub fn close(&mut self, path: &Path) -> Result<(), LspError> {
        self.open.remove(path);
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri_of(path)}}),
        )
    }

    /// Say that files on disk changed. `1` is created, `2` changed and `3` deleted; a server takes
    /// any of them as a reason to read the file again.
    pub fn files_changed(&self, paths: &[PathBuf]) -> Result<(), LspError> {
        let changes: Vec<Value> = paths
            .iter()
            .map(|path| json!({"uri": uri_of(path), "type": 2}))
            .collect();
        self.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": changes}),
        )
    }

    /// The definitions of what is at a 0-based position.
    pub fn definition(
        &mut self,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<Location>, LspError> {
        self.locations("textDocument/definition", path, line, column)
    }

    fn locations(
        &mut self,
        method: &str,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<Location>, LspError> {
        let result = self.request(method, position(path, line, column), QUERY_TIMEOUT)?;
        Ok(locations_in(&result))
    }

    /// One method at many 0-based positions of a file, in order. A question and its answer take
    /// milliseconds each way, so asking one at a time is what is slow, not the server: these go out
    /// together.
    pub fn many(
        &mut self,
        method: &str,
        path: &Path,
        positions: &[(u32, u32)],
    ) -> Result<Vec<Vec<Location>>, LspError> {
        let mut out = Vec::with_capacity(positions.len());
        for chunk in positions.chunks(BATCH) {
            let first = self.next_id + 1;
            for &(line, column) in chunk {
                self.next_id += 1;
                write_message(
                    &self.stdin,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": self.next_id,
                        "method": method,
                        "params": position(path, line, column),
                    }),
                )?;
            }
            let mut answers: Vec<Option<Vec<Location>>> = chunk.iter().map(|_| None).collect();
            let mut waiting = chunk.len();
            let started = Instant::now();
            while waiting > 0 {
                let timed_out = || LspError::TimedOut {
                    method: method.to_string(),
                };
                let left = QUERY_TIMEOUT
                    .checked_sub(started.elapsed())
                    .ok_or_else(timed_out)?;
                let answer = self.answers.recv_timeout(left).map_err(|_| timed_out())?;
                let Some(index) = answer["id"]
                    .as_u64()
                    .and_then(|id| id.checked_sub(first))
                    .map(|index| index as usize)
                    .filter(|index| *index < chunk.len())
                else {
                    continue;
                };
                waiting -= 1;
                answers[index] = match answer["error"]["code"].as_i64() {
                    // Asked again alone below, with the waiting that takes.
                    Some(CONTENT_MODIFIED | CANCELLED) => None,
                    Some(_) => {
                        return Err(LspError::Failed {
                            method: method.to_string(),
                            message: answer["error"]["message"].to_string(),
                        });
                    }
                    None => Some(locations_in(&answer["result"])),
                };
            }
            for (answer, &(line, column)) in answers.into_iter().zip(chunk) {
                out.push(match answer {
                    Some(found) => found,
                    None => self.locations(method, path, line, column)?,
                });
            }
        }
        Ok(out)
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        let timed_out = || LspError::TimedOut {
            method: method.to_string(),
        };
        for attempt in 0..ATTEMPTS {
            self.next_id += 1;
            let id = self.next_id;
            write_message(
                &self.stdin,
                &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            )?;
            let started = Instant::now();
            loop {
                let left = timeout
                    .checked_sub(started.elapsed())
                    .ok_or_else(timed_out)?;
                let answer = self.answers.recv_timeout(left).map_err(|_| timed_out())?;
                if answer["id"] != id {
                    continue;
                }
                let code = answer["error"]["code"].as_i64();
                match code {
                    Some(CONTENT_MODIFIED | CANCELLED) => {
                        std::thread::sleep(Duration::from_millis(100 * (attempt as u64 + 1)));
                        break;
                    }
                    Some(_) => {
                        return Err(LspError::Failed {
                            method: method.to_string(),
                            message: answer["error"]["message"].to_string(),
                        });
                    }
                    None => return Ok(answer["result"].clone()),
                }
            }
        }
        Err(LspError::Cancelled {
            method: method.to_string(),
        })
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        write_message(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
    }
}

fn position(path: &Path, line: u32, column: u32) -> Value {
    json!({
        "textDocument": {"uri": uri_of(path)},
        "position": {"line": line, "character": column},
    })
}

fn locations_in(result: &Value) -> Vec<Location> {
    let one = std::slice::from_ref(result);
    let answers = result.as_array().map_or(one, Vec::as_slice);
    let number = |v: &Value| v.as_u64().map(|n| n as u32);
    answers
        .iter()
        .filter_map(|answer| {
            let range = &answer["range"];
            Some(Location {
                path: path_of(answer["uri"].as_str()?)?,
                line: number(&range["start"]["line"])?,
                column: number(&range["start"]["character"])?,
                end_column: number(&range["end"]["character"])?,
            })
        })
        .collect()
}

/// End a process. One that has already ended is what was wanted.
pub fn stop(child: &mut Child) {
    if let Err(e) = child.kill() {
        debug!("The process already ended: {e}");
    }
    if let Err(e) = child.wait() {
        debug!("The process could not be waited for: {e}");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        stop(&mut self.child);
    }
}
