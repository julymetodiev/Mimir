//! Edited files, resolved by a running `rust-analyzer` instead of a new index.
//!
//! An index costs a whole run after any edit. A running analyzer recomputes what the edit
//! changed, so it answers for one file in well under a second. It answers one position at a
//! time and has no symbol names to give, so only the sites the parser found are asked about:
//! a function named as a value among a macro's tokens waits for the next index.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mimir_parse_core::ExtractedReference;
use mimir_types::{ContentHash, ReferenceKind};
use serde_json::{Value, json};
use tracing::{debug, info};

use mimir_resolve::{LiveDocument, ProjectFile, Target, type_name};

const LOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// The first question about a crate waits for that crate's analysis.
const QUERY_TIMEOUT: Duration = Duration::from_secs(300);
/// The server is still catching up with an edit.
const CONTENT_MODIFIED: i64 = -32801;
const CANCELLED: i64 = -32802;
const ATTEMPTS: usize = 6;
/// Going to the definition of these lands on what they use (`fmt`, `from_str`, `from`), not on
/// them. Those are not the calls written here, and an index does not report them.
const FORWARDING_METHODS: [&str; 4] = ["into", "try_into", "parse", "to_string"];

/// The running analyzers of a project, one per Cargo root.
#[derive(Default)]
pub struct Analyzers {
    running: HashMap<PathBuf, Analyzer>,
    last_used: Option<Instant>,
}

impl Analyzers {
    /// Stop every analyzer if none was used for `idle`: each holds gigabytes.
    pub fn stop_if_idle(&mut self, idle: Duration) {
        if self.last_used.is_some_and(|used| used.elapsed() >= idle) && !self.running.is_empty() {
            info!("Stopping {} idle rust-analyzer", self.running.len());
            self.running.clear();
            self.last_used = None;
        }
    }

    /// Ask about every reference in `stale`, and the trait of every impl, from the analyzer
    /// of the Cargo root at `root`, an absolute path. `files` are all the project's files.
    pub fn resolve(
        &mut self,
        project: &Path,
        root: &Path,
        files: &[ProjectFile],
        stale: &[&ProjectFile],
    ) -> Result<HashMap<PathBuf, LiveDocument>, String> {
        self.last_used = Some(Instant::now());
        if !self.running.contains_key(root) {
            let started = Instant::now();
            let analyzer = Analyzer::start(root)?;
            info!(
                "rust-analyzer ready for {:?} in {:?}",
                root,
                started.elapsed()
            );
            self.running.insert(root.to_path_buf(), analyzer);
        }
        let analyzer = self.running.get_mut(root).ok_or("no analyzer")?;
        let result = resolve_documents(analyzer, project, files, stale);
        if result.is_err() {
            self.running.remove(root);
        }
        self.last_used = Some(Instant::now());
        result
    }
}

fn resolve_documents(
    analyzer: &mut Analyzer,
    project: &Path,
    files: &[ProjectFile],
    stale: &[&ProjectFile],
) -> Result<HashMap<PathBuf, LiveDocument>, String> {
    let project = project.canonicalize().map_err(|e| e.to_string())?;
    let sources: HashMap<&Path, &str> = files
        .iter()
        .map(|f| (f.path.as_path(), &*f.source))
        .collect();
    for file in stale {
        analyzer.update(&project.join(&file.path), &file.source)?;
    }
    let target = |found: Location| {
        let path = found.path.strip_prefix(&project).ok()?.to_path_buf();
        let line = sources
            .get(path.as_path())?
            .split('\n')
            .nth(found.line as usize)?;
        let name = line
            .get(found.column as usize..found.end_column as usize)?
            .to_string();
        Some(Target {
            path,
            line: found.line + 1,
            name,
        })
    };

    let mut documents = HashMap::new();
    for file in stale {
        let path = project.join(&file.path);
        let mut document = LiveDocument::default();
        for reference in file.references.iter().filter(|r| {
            matches!(
                r.kind,
                ReferenceKind::Call | ReferenceKind::FunctionValue | ReferenceKind::Read
            )
        }) {
            let (line, column) = site_of(reference);
            let found = analyzer
                .definition(&path, line - 1, column)?
                .into_iter()
                .filter_map(target)
                .collect();
            if let Some(found) = choose(found, &reference.name)
                && (found.name == reference.name
                    || !FORWARDING_METHODS.contains(&reference.name.as_str()))
            {
                document.sites.insert((line, column), found);
            }
        }
        for (line, column) in impl_trait_names(&file.source) {
            let found = analyzer
                .definition(&path, line - 1, column)?
                .into_iter()
                .find_map(target);
            if let Some(found) = found {
                document.traits.insert(line, found);
            }
        }
        documents.insert(file.path.clone(), document);
    }
    Ok(documents)
}

/// The answer meant by a site. Inside a macro the server answers once per expansion, and those
/// can include whatever the macro calls; a different name only fits a renaming import.
fn choose(found: Vec<Target>, name: &str) -> Option<Target> {
    let mut found = found.into_iter();
    let first = found.next()?;
    if first.name == name {
        return Some(first);
    }
    found.find(|t| t.name == name).or(Some(first))
}

fn site_of(reference: &ExtractedReference) -> (u32, u32) {
    (reference.range.start_line, reference.range.start_col)
}

/// Where the trait is named in each single-line `impl Tr for T`: the `impl`'s line, 1-based,
/// and the name's column. An impl with a header across lines waits for the next index.
fn impl_trait_names(source: &str) -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    for (i, line) in source.split('\n').enumerate() {
        let text = line.trim_start();
        let text = text.strip_prefix("unsafe ").unwrap_or(text);
        let Some(rest) = text
            .strip_prefix("impl")
            .filter(|r| r.starts_with([' ', '<']))
        else {
            continue;
        };
        let rest = skip_generics(rest).trim_start();
        let Some((implemented, _)) = rest.split_once(" for ") else {
            continue;
        };
        let name = type_name(implemented);
        let at = rest.as_ptr() as usize - line.as_ptr() as usize;
        if let Some(offset) = implemented.find(name).filter(|_| !name.is_empty()) {
            // The last segment of the path, not an earlier one that contains its text.
            let offset = implemented
                .split('<')
                .next()
                .unwrap_or(implemented)
                .rfind(name)
                .unwrap_or(offset);
            found.push((i as u32 + 1, (at + offset) as u32));
        }
    }
    found
}

/// The text after a leading `<...>`.
fn skip_generics(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('<') {
        return text;
    }
    let (mut depth, mut previous) = (0, ' ');
    for (at, c) in trimmed.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if previous != '-' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return &trimmed[at + 1..];
        }
        previous = c;
    }
    ""
}

/// A definition's name: an absolute path, and a 0-based range on one line.
struct Location {
    path: PathBuf,
    line: u32,
    column: u32,
    end_column: u32,
}

/// One `rust-analyzer` language server. Dropping it ends the process.
struct Analyzer {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    answers: Receiver<Value>,
    status: Receiver<Value>,
    next_id: u64,
    /// What the server was told each open file holds, and its version.
    open: HashMap<PathBuf, (ContentHash, i64)>,
}

fn config() -> Value {
    json!({
        "checkOnSave": false,
        "cargo": {"allTargets": true, "buildScripts": {"enable": true}},
        "procMacro": {"enable": true},
        "cachePriming": {"enable": false},
        "diagnostics": {"enable": false},
        // Files changed by anyone else, and those added or removed.
        "files": {"watcher": "server"},
    })
}

impl Analyzer {
    fn start(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let mut child = Command::new("rust-analyzer")
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot run rust-analyzer: {e}"))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("no stdin")?));
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (answers_tx, answers) = channel();
        let (status_tx, status) = channel();
        let writer = stdin.clone();
        std::thread::spawn(move || read_messages(stdout, &writer, &answers_tx, &status_tx));

        let mut analyzer = Self {
            child,
            stdin,
            answers,
            status,
            next_id: 0,
            open: HashMap::new(),
        };
        let uri = uri_of(&root);
        let capabilities = json!({
            "window": {"workDoneProgress": true},
            "workspace": {"configuration": true},
            "textDocument": {"definition": {"linkSupport": false}},
            // Mimir's columns are bytes, as this asks.
            "general": {"positionEncodings": ["utf-8"]},
            "experimental": {"serverStatusNotification": true},
        });
        let init = analyzer.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": uri,
                "workspaceFolders": [{"uri": uri, "name": "project"}],
                "initializationOptions": config(),
                "capabilities": capabilities,
            }),
            LOAD_TIMEOUT,
        )?;
        if init["capabilities"]["positionEncoding"] != "utf-8" {
            return Err("rust-analyzer does not offer byte columns".to_string());
        }
        analyzer.notify("initialized", json!({}))?;
        analyzer.wait_until_loaded()?;
        Ok(analyzer)
    }

    fn wait_until_loaded(&self) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let left = LOAD_TIMEOUT
                .checked_sub(started.elapsed())
                .ok_or("rust-analyzer did not load in time")?;
            let status = self
                .status
                .recv_timeout(left)
                .map_err(|_| "rust-analyzer did not load in time")?;
            if status["quiescent"] == true {
                return match status["health"].as_str() {
                    Some("error") => Err(format!(
                        "rust-analyzer could not load: {}",
                        status["message"]
                    )),
                    _ => Ok(()),
                };
            }
        }
    }

    /// Tell the server what a file holds now.
    fn update(&mut self, path: &Path, text: &str) -> Result<(), String> {
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
                self.notify("textDocument/didOpen", json!({
                    "textDocument": {"uri": uri, "languageId": "rust", "version": 1, "text": text},
                }))
            }
        }
    }

    /// The definitions of what is at a 0-based position.
    fn definition(&mut self, path: &Path, line: u32, column: u32) -> Result<Vec<Location>, String> {
        let result = self.request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": uri_of(path)},
                "position": {"line": line, "character": column},
            }),
            QUERY_TIMEOUT,
        )?;
        let one = std::slice::from_ref(&result);
        let answers = result.as_array().map_or(one, Vec::as_slice);
        let number = |v: &Value| v.as_u64().map(|n| n as u32);
        Ok(answers
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
            .collect())
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
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
                    .ok_or_else(|| format!("{method} timed out"))?;
                let answer = self
                    .answers
                    .recv_timeout(left)
                    .map_err(|_| format!("{method} timed out"))?;
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
                        return Err(format!("{method} failed: {}", answer["error"]["message"]));
                    }
                    None => return Ok(answer["result"].clone()),
                }
            }
        }
        Err(format!("{method} kept being cancelled"))
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        write_message(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
    }
}

/// End a process. One that has already ended is what was wanted.
pub fn stop(child: &mut Child) {
    if let Err(e) = child.kill() {
        debug!("rust-analyzer already ended: {e}");
    }
    if let Err(e) = child.wait() {
        debug!("rust-analyzer could not be waited for: {e}");
    }
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        stop(&mut self.child);
    }
}

/// Answer something the analyzer asked. Failing means it is gone, which the next request reports.
fn answer_server(stdin: &Mutex<ChildStdin>, message: &Value) {
    if let Err(e) = write_message(stdin, message) {
        debug!("Cannot answer the analyzer: {e}");
    }
}

fn write_message(stdin: &Mutex<ChildStdin>, message: &Value) -> Result<(), String> {
    let body = message.to_string();
    let mut stdin = stdin.lock().map_err(|e| e.to_string())?;
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|()| stdin.flush())
        .map_err(|e| format!("rust-analyzer stopped: {e}"))
}

/// Hand out answers and status, and answer what the server asks of a client.
fn read_messages(
    stdout: impl Read,
    stdin: &Mutex<ChildStdin>,
    answers: &Sender<Value>,
    status: &Sender<Value>,
) {
    let mut reader = BufReader::new(stdout);
    while let Some(message) = read_message(&mut reader) {
        let method = message["method"].as_str();
        match (method, message.get("id")) {
            (Some("workspace/configuration"), Some(id)) => {
                let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                let result: Vec<Value> = (0..items).map(|_| config()).collect();
                answer_server(
                    stdin,
                    &json!({"jsonrpc": "2.0", "id": id, "result": result}),
                );
            }
            (Some(_), Some(id)) => {
                answer_server(stdin, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
            }
            (Some("experimental/serverStatus"), None) => {
                if status.send(message["params"].clone()).is_err() {
                    debug!("Nobody is waiting for the analyzer's status any more");
                }
            }
            (Some(_), None) => {}
            (None, _) => {
                if answers.send(message).is_err() {
                    debug!("Nobody is waiting for the analyzer's answers any more");
                }
            }
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
            && let Ok(found) = value.trim().parse::<usize>()
        {
            length = Some(found);
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn uri_of(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

fn path_of(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?.as_bytes();
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut i = 0;
    while i < encoded.len() {
        if encoded[i] == b'%' {
            let hex = std::str::from_utf8(encoded.get(i + 1..i + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            decoded.push(encoded[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(decoded).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_the_answer_with_the_name_at_the_site() {
        let target = |name: &str| Target {
            path: PathBuf::from("a.rs"),
            line: 1,
            name: name.to_string(),
        };
        let chosen = |found: &[&str], name| {
            choose(found.iter().map(|n| target(n)).collect(), name).map(|t| t.name)
        };

        assert_eq!(
            chosen(&["block_on", "helper"], "helper").as_deref(),
            Some("helper")
        );
        assert_eq!(
            chosen(&["original"], "renamed").as_deref(),
            Some("original")
        );
        assert_eq!(chosen(&[], "helper"), None);
    }

    #[test]
    fn a_uri_survives_the_round_trip() {
        let path = Path::new("/tmp/a b/ü-1.rs");
        assert_eq!(path_of(&uri_of(path)).as_deref(), Some(path));
    }

    #[test]
    fn finds_the_trait_in_an_impl_header() {
        let source = "impl Foo {}\nimpl Tr for Foo {}\n    impl<T> a::Tr<T> for Foo<T> {}\nunsafe impl<'a, F: Fn() -> u32> Tr for F {}\nimplement();\n";
        let lines: Vec<&str> = source.split('\n').collect();
        let names: Vec<(u32, &str)> = impl_trait_names(source)
            .into_iter()
            .map(|(line, column)| {
                (
                    line,
                    &lines[line as usize - 1][column as usize..column as usize + 2],
                )
            })
            .collect();
        assert_eq!(names, [(2, "Tr"), (3, "Tr"), (4, "Tr")]);
    }
}
