//! The MCP server from outside, as a client speaks to it: JSON-RPC, one message per line.

use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use tempfile::TempDir;

struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    fn start(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mimir"))
            .arg("mcp")
            .current_dir(dir)
            .env("MIMIR_NO_SCIP", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut server = Self {
            child,
            input,
            output,
            next_id: 1,
        };
        server.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"},
            }),
        );
        server.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        server
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.input, "{message}").unwrap();
        self.input.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert_ne!(self.output.read_line(&mut line).unwrap(), 0, "server ended");
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == json!(id) {
                return message["result"].clone();
            }
        }
    }

    /// Call a tool; gives its text and whether it reported an error.
    fn call(&mut self, tool: &str, arguments: Value) -> (String, bool) {
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        (
            result["content"][0]["text"].as_str().unwrap().to_string(),
            result["isError"].as_bool().unwrap_or(false),
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn helper() -> u32 {\n    1\n}\n\npub fn compute() -> u32 {\n    helper()\n}\n",
    )
    .unwrap();
    dir
}

#[test]
fn test_tools_work_end_to_end() {
    let project = project();
    let mut server = Server::start(project.path());

    let tools = server.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for name in [
        "trace",
        "search",
        "read",
        "edit_symbols",
        "edit_lines",
        "sync",
        "status",
    ] {
        assert!(names.contains(&name), "{name} is missing from {names:?}");
    }

    let (trace, failed) = server.call("trace", json!({"symbol": "helper", "direction": "callers"}));
    assert!(!failed, "{trace}");
    let trace: Value = serde_json::from_str(&trace).unwrap();
    assert_eq!(trace["callers"][0]["name"], "compute");
    assert_eq!(trace["callers"][0]["at"], "src/lib.rs:5-7");
    assert!(trace["callers"][0].get("possible").is_none());
    assert!(trace.get("id").is_none());

    let (read, failed) = server.call("read", json!({"symbol": "compute"}));
    assert!(!failed && read.contains("helper()"), "{read}");

    let (found, failed) = server.call("search", json!({"pattern": "^comp"}));
    assert!(!failed && found.contains("compute"), "{found}");

    let (edited, failed) = server.call(
        "edit_symbols",
        json!({"edits": [{"symbol": "helper", "content": "pub fn helper() -> u32 {\n    2\n}"}]}),
    );
    assert!(!failed, "{edited}");
    assert!(
        fs::read_to_string(project.path().join("src/lib.rs"))
            .unwrap()
            .contains("    2\n")
    );

    let (missing, failed) = server.call("trace", json!({"symbol": "no_such_symbol"}));
    assert!(failed && missing.contains("no_such_symbol"), "{missing}");

    let (stats, failed) = server.call("status", json!({}));
    assert!(!failed && stats.contains("\"symbols\""), "{stats}");

    let stop = Command::new(env!("CARGO_BIN_EXE_mimir"))
        .args(["daemon", "stop"])
        .current_dir(project.path())
        .status()
        .unwrap();
    assert!(stop.success());
}

#[test]
fn test_outside_a_project_a_tool_says_so_and_the_server_stays() {
    let dir = TempDir::new().unwrap();
    let mut server = Server::start(dir.path());
    let (message, failed) = server.call("status", json!({}));
    assert!(failed && message.contains("mimir init"), "{message}");
    let tools = server.request("tools/list", json!({}));
    assert!(tools["tools"].is_array());
}
