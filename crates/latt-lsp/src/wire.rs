//! The messages of a language server, and the URIs of files.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use serde_json::{Value, json};
use tracing::debug;

use crate::LspError;

pub fn write_message(stdin: &Mutex<impl Write>, message: &Value) -> Result<(), LspError> {
    let body = message.to_string();
    let mut stdin = stdin.lock().map_err(|_| LspError::Poisoned)?;
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|()| stdin.flush())
        .map_err(LspError::Stopped)
}

/// Answer something the server asked. Failing means it is gone, which the next request reports.
fn answer_server(stdin: &Mutex<impl Write>, message: &Value) {
    if let Err(e) = write_message(stdin, message) {
        debug!("Cannot answer the language server: {e}");
    }
}

/// Where a reader of a server's output hands things on.
pub struct Reader {
    pub configuration: Value,
    pub status_method: Option<String>,
    pub answers: Sender<Value>,
    pub status: Sender<Value>,
}

impl Reader {
    /// Hand out answers and status, and answer what the server asks of a client.
    pub fn run(&self, stdout: impl Read, stdin: &Mutex<impl Write>) {
        let mut reader = BufReader::new(stdout);
        while let Some(message) = read_message(&mut reader) {
            let method = message["method"].as_str();
            match (method, message.get("id")) {
                (Some("workspace/configuration"), Some(id)) => {
                    let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                    let result = vec![self.configuration.clone(); items];
                    answer_server(
                        stdin,
                        &json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    );
                }
                (Some(_), Some(id)) => {
                    answer_server(stdin, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
                }
                (Some(name), None) => {
                    if self.status_method.as_deref() == Some(name)
                        && self.status.send(message["params"].clone()).is_err()
                    {
                        debug!("Nobody is waiting for the language server's status any more");
                    }
                }
                (None, _) => {
                    if self.answers.send(message).is_err() {
                        debug!("Nobody is waiting for the language server's answers any more");
                    }
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

pub fn uri_of(path: &Path) -> String {
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

pub fn path_of(uri: &str) -> Option<PathBuf> {
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
    use std::sync::mpsc::channel;

    #[test]
    fn a_uri_survives_the_round_trip() {
        let path = Path::new("/tmp/a b/ü-1.rs");
        assert_eq!(path_of(&uri_of(path)).as_deref(), Some(path));
    }

    fn framed(message: &Value) -> Vec<u8> {
        let body = message.to_string();
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
    }

    #[test]
    fn messages_go_where_they_belong() {
        let mut input = Vec::new();
        for message in [
            json!({"jsonrpc": "2.0", "id": 1, "result": 7}),
            json!({"jsonrpc": "2.0", "method": "state", "params": {"ready": true}}),
            json!({"jsonrpc": "2.0", "method": "other", "params": {}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "workspace/configuration",
                   "params": {"items": [{}, {}]}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "client/registerCapability"}),
        ] {
            input.extend(framed(&message));
        }
        let (answers_tx, answers) = channel();
        let (status_tx, status) = channel();
        let written = Mutex::new(Vec::new());
        Reader {
            configuration: json!({"a": 1}),
            status_method: Some("state".to_string()),
            answers: answers_tx,
            status: status_tx,
        }
        .run(input.as_slice(), &written);

        assert_eq!(answers.try_iter().collect::<Vec<_>>().len(), 1);
        assert_eq!(status.try_iter().next(), Some(json!({"ready": true})));
        let written = String::from_utf8(written.into_inner().unwrap()).unwrap();
        assert!(
            written.contains(r#""result":[{"a":1},{"a":1}]"#),
            "{written}"
        );
        assert!(written.contains(r#""id":3"#), "{written}");
    }

    #[test]
    fn a_message_cut_short_ends_the_reading() {
        let mut bytes = framed(&json!({"id": 1}));
        bytes.truncate(bytes.len() - 1);
        assert_eq!(read_message(&mut bytes.as_slice()), None);
    }
}
