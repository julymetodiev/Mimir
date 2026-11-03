//! What `rust-analyzer` needs from a language server: its configuration, byte columns, and
//! waiting until it has loaded the project.

use std::path::Path;
use std::time::{Duration, Instant};

use mimir_lsp::{Location, Server, ServerSpec};
use serde_json::{Value, json};

const LOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// One `rust-analyzer` language server. Dropping it ends the process.
pub struct Analyzer {
    server: Server,
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
    pub fn start(root: &Path) -> Result<Self, String> {
        let spec = ServerSpec {
            program: "rust-analyzer".to_string(),
            args: Vec::new(),
            configuration: config(),
            capabilities: json!({
                "window": {"workDoneProgress": true},
                "workspace": {"configuration": true},
                "textDocument": {"definition": {"linkSupport": false}},
                // Mimir's columns are bytes, as this asks.
                "general": {"positionEncodings": ["utf-8"]},
                "experimental": {"serverStatusNotification": true},
            }),
            status_method: Some("experimental/serverStatus".to_string()),
        };
        let server = Server::start(spec, root).map_err(|e| e.to_string())?;
        if server.capabilities()["positionEncoding"] != "utf-8" {
            return Err("rust-analyzer does not offer byte columns".to_string());
        }
        let analyzer = Self { server };
        analyzer.wait_until_loaded()?;
        Ok(analyzer)
    }

    fn wait_until_loaded(&self) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let status = LOAD_TIMEOUT
                .checked_sub(started.elapsed())
                .and_then(|left| self.server.next_status(left))
                .ok_or("rust-analyzer did not load in time")?;
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
    pub fn update(&mut self, path: &Path, text: &str) -> Result<(), String> {
        self.server
            .update(path, "rust", text)
            .map_err(|e| e.to_string())
    }

    /// The definitions of what is at a 0-based position.
    pub fn definition(
        &mut self,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<Location>, String> {
        self.server
            .definition(path, line, column)
            .map_err(|e| e.to_string())
    }
}
