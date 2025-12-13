//! GraphClient implementation using DaemonClient.
//!
//! This module wraps the DaemonClient to implement the GraphClient trait
//! from mimir-tools, allowing tools to use the daemon for semantic operations.

use super::DaemonClient;
use async_trait::async_trait;
use mimir_protocol::{FindOptions, SearchOptions, TraceOptions};
use mimir_tools::{GraphClient, GraphError, SearchResult, SymbolInfo, TraceResult};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// GraphClient implementation using the daemon.
pub struct DaemonGraphClient {
    client: Arc<Mutex<DaemonClient>>,
    project_root: PathBuf,
}

impl DaemonGraphClient {
    /// Create a new DaemonGraphClient.
    pub fn new(client: DaemonClient, project_root: PathBuf) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            project_root,
        }
    }

    /// Try to connect to the daemon.
    pub async fn connect(agent_dir: &std::path::Path, project_root: PathBuf) -> Result<Self, GraphError> {
        let client = DaemonClient::connect(agent_dir)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;
        Ok(Self::new(client, project_root))
    }
}

#[async_trait]
impl GraphClient for DaemonGraphClient {
    async fn search(
        &self,
        pattern: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<SearchResult, GraphError> {
        let mut client = self.client.lock().await;

        let options = SearchOptions {
            pattern: pattern.to_string(),
            kind_filter: kind.map(String::from),
            limit: Some(limit as u32),
        };

        let response = client
            .search(options)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;

        let symbols: Vec<SymbolInfo> = response
            .symbols
            .into_iter()
            .map(|s| SymbolInfo {
                name: s.name,
                qualified_name: s.qualified_name.unwrap_or_default(),
                kind: format!("{:?}", s.kind),
                file: s.file_path.to_string_lossy().to_string(),
                start_line: s.start_line,
                end_line: s.end_line,
                code: None,
            })
            .collect();

        Ok(SearchResult {
            total: response.total_count,
            symbols,
        })
    }

    async fn trace(&self, symbol: &str, depth: u32) -> Result<TraceResult, GraphError> {
        let mut client = self.client.lock().await;

        let options = TraceOptions {
            max_depth: Some(depth),
            callers_only: false,
            callees_only: false,
            cross_language: false,
        };

        let response = client
            .trace_by_name(symbol, options)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;

        // Convert root symbol
        let root = response.root;
        let symbol_info = SymbolInfo {
            name: root.name,
            qualified_name: root.qualified_name.unwrap_or_default(),
            kind: format!("{:?}", root.kind),
            file: root.file_path.to_string_lossy().to_string(),
            start_line: root.start_line,
            end_line: root.end_line,
            code: None,
        };

        // Convert callers
        let callers: Vec<SymbolInfo> = response
            .callers
            .into_iter()
            .map(|node| SymbolInfo {
                name: node.symbol.name,
                qualified_name: node.symbol.qualified_name.unwrap_or_default(),
                kind: format!("{:?}", node.symbol.kind),
                file: node.symbol.file_path.to_string_lossy().to_string(),
                start_line: node.symbol.start_line,
                end_line: node.symbol.end_line,
                code: None,
            })
            .collect();

        // Convert callees
        let callees: Vec<SymbolInfo> = response
            .callees
            .into_iter()
            .map(|node| SymbolInfo {
                name: node.symbol.name,
                qualified_name: node.symbol.qualified_name.unwrap_or_default(),
                kind: format!("{:?}", node.symbol.kind),
                file: node.symbol.file_path.to_string_lossy().to_string(),
                start_line: node.symbol.start_line,
                end_line: node.symbol.end_line,
                code: None,
            })
            .collect();

        Ok(TraceResult {
            symbol: symbol_info,
            callers,
            callees,
        })
    }

    async fn find_symbol(&self, name: &str) -> Result<Option<SymbolInfo>, GraphError> {
        let mut client = self.client.lock().await;

        // Use higher limit to find all matches, then pick best one
        let options = FindOptions {
            file_pattern: None,
            language: None,
            limit: Some(100),
            include_deps: false,
        };

        let response = client
            .find_symbol(name, options)
            .await
            .map_err(|e| GraphError::new(e.to_string()))?;

        // Priority: impl > struct > function > enum/trait > enum_variant
        // This ensures we get the main definition, not a variant with the same name
        let best = response
            .symbols
            .into_iter()
            .filter(|s| s.name == name) // Exact match only
            .max_by_key(|s| {
                let kind = format!("{:?}", s.kind).to_lowercase();
                match kind.as_str() {
                    "impl" => 100,
                    "struct" | "class" => 90,
                    "function" => 80,
                    "enum" | "trait" | "interface" => 70,
                    "enumvariant" | "enum_variant" => 10,
                    _ => 60,
                }
            });

        Ok(best.map(|s| SymbolInfo {
            name: s.name,
            qualified_name: s.qualified_name.unwrap_or_default(),
            kind: format!("{:?}", s.kind),
            file: s.file_path.to_string_lossy().to_string(),
            start_line: s.start_line,
            end_line: s.end_line,
            code: None,
        }))
    }

    async fn read_symbol(&self, name: &str) -> Result<Option<(SymbolInfo, String)>, GraphError> {
        // First find the symbol
        let info = match self.find_symbol(name).await? {
            Some(i) => i,
            None => return Ok(None),
        };

        // Read the source code from file
        let file_path = self.project_root.join(&info.file);
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| GraphError::new(format!("Failed to read file: {}", e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let start = (info.start_line as usize).saturating_sub(1);
        let end = (info.end_line as usize).min(lines.len());

        if start >= lines.len() {
            return Err(GraphError::new("Invalid line range"));
        }

        let code = lines[start..end].join("\n");
        Ok(Some((info, code)))
    }
}
