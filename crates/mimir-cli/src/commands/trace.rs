//! Trace command - trace call chains for a symbol.

use anyhow::{Context, Result};
use mimir_graph::{CodeGraph, TraceOptions};
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_protocol::TraceOptions as IpcTraceOptions;
use mimir_types::{FileId, IdGenerator, Language, Symbol};
use std::path::Path;
use std::sync::Arc;

use crate::app::{Config, find_project_root};
use crate::commands::sync::{load_graph, sync_project};
use crate::daemon::{check_daemon_running, DaemonClient};

/// Execute the trace command.
pub async fn execute(
    file: Option<&Path>,
    symbol_name: &str,
    depth: usize,
    json: bool,
    show_code: bool,
) -> Result<()> {
    match file {
        Some(f) => execute_single_file(f, symbol_name, depth, json, show_code).await,
        None => execute_from_graph(symbol_name, depth, json, show_code).await,
    }
}

/// Execute trace on a single file.
async fn execute_single_file(
    file: &Path,
    symbol_name: &str,
    depth: usize,
    json: bool,
    show_code: bool,
) -> Result<()> {
    // Verify file exists
    if !file.exists() {
        anyhow::bail!("File not found: {}", file.display());
    }

    // Detect language
    let language = Language::from_path(file);
    if !language.is_known() {
        anyhow::bail!("Unsupported file type: {}", file.display());
    }

    // Setup adapter registry
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));

    let adapter = registry
        .get_by_language(language)
        .context("No adapter for language")?;

    // Read and parse the file
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("Failed to read {}", file.display()))?;

    let parsed = adapter.parse(&source).context("Failed to parse file")?;

    // Create ID generator and graph
    let ids = IdGenerator::new();
    let file_id: FileId = ids.next();
    let graph = CodeGraph::new();

    // Extract symbols
    let extracted = adapter.extract_symbols(&parsed, file_id);

    // Add symbols to graph
    for ext in &extracted {
        let symbol = Symbol {
            id: ids.next(),
            name: ext.name.clone(),
            qualified_name: ext.qualified_name.clone(),
            kind: ext.kind,
            language: ext.language,
            file_id,
            range: ext.range,
            span: ext.span,
            signature: ext.signature.clone(),
            visibility: ext.visibility,
            doc: ext.doc.clone(),
        };
        graph.add_symbol(symbol);
    }

    // Extract and add references
    let references = adapter.extract_references(&parsed, file_id);
    for reference in &references {
        // Find source symbol (containing function)
        if let Some(from_symbol) = find_containing_symbol(&graph, file_id, reference.span.start) {
            // Find target symbol by name
            let targets = graph.find_symbol(&reference.name);
            if let Some(to_symbol) = targets.first() {
                graph.add_call(
                    from_symbol.id,
                    to_symbol.id,
                    reference.span,
                    reference.confidence,
                );
            }
        }
    }

    // Find and trace the symbol
    trace_symbol(&graph, symbol_name, depth, json, show_code, Some(file))
}

/// Execute trace from the synced graph.
async fn execute_from_graph(symbol_name: &str, depth: usize, json: bool, show_code: bool) -> Result<()> {
    // Find project root
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    // Check if daemon is running - use it for automatic updates
    if check_daemon_running(&config.agent_dir) {
        return execute_via_daemon(&config, symbol_name, depth, json, show_code).await;
    }

    // Fallback: sync and query locally
    let graph = if config.sync.sync_on_query {
        // Sync before query to ensure graph is up-to-date
        let (graph, _ids, _file_ids, _stats) = sync_project(&config, false).await?;
        graph
    } else {
        // Load from disk
        let (graph, _ids, _file_ids) = load_graph(&config)
            .await?
            .context("Graph not found. Run 'mimir sync' first.")?;
        graph
    };

    // Find and trace the symbol
    trace_symbol(&graph, symbol_name, depth, json, show_code, None)
}

/// Execute trace via the daemon.
async fn execute_via_daemon(config: &Config, symbol_name: &str, depth: usize, json: bool, show_code: bool) -> Result<()> {
    let mut client = DaemonClient::connect(&config.agent_dir).await?;

    let options = IpcTraceOptions {
        max_depth: Some(depth as u32),
        callers_only: false,
        callees_only: false,
        cross_language: false,
    };

    let result = client.trace_by_name(symbol_name, options).await?;

    // Output results
    if json {
        let output = serde_json::to_string_pretty(&result)?;
        println!("{}", output);
    } else {
        println!("Trace for: {} ({:?})", result.root.name, result.root.kind);
        println!("  Lines: {}-{}", result.root.start_line, result.root.end_line);
        println!("  (via daemon)");

        // Show code snippet if requested
        if show_code && !result.root.file_path.as_os_str().is_empty() {
            let file_path = config.root.join(&result.root.file_path);
            if let Ok(source) = std::fs::read_to_string(&file_path) {
                let lines: Vec<&str> = source.lines().collect();
                let start = (result.root.start_line as usize).saturating_sub(1);
                let end = (result.root.end_line as usize).min(lines.len());
                println!();
                println!("```");
                for (i, line) in lines[start..end].iter().enumerate() {
                    println!("{:4}│ {}", start + i + 1, line);
                }
                println!("```");
            }
        }
        println!();

        if !result.callers.is_empty() {
            println!("Callers ({}):", result.callers.len());
            for node in &result.callers {
                let indent = "  ".repeat(node.depth as usize);
                println!("{}← {} ({:?})", indent, node.symbol.name, node.symbol.kind);
            }
            println!();
        }

        if !result.callees.is_empty() {
            println!("Callees ({}):", result.callees.len());
            for node in &result.callees {
                let indent = "  ".repeat(node.depth as usize);
                println!("{}→ {} ({:?})", indent, node.symbol.name, node.symbol.kind);
            }
            println!();
        }

        println!(
            "Total: {} symbols, max depth: {}",
            result.total_symbols, result.max_depth_reached
        );
    }

    Ok(())
}

/// Common trace logic.
fn trace_symbol(
    graph: &CodeGraph,
    symbol_name: &str,
    depth: usize,
    json: bool,
    show_code: bool,
    file: Option<&Path>,
) -> Result<()> {
    // Find the target symbol
    let symbols = graph.find_symbol(symbol_name);
    if symbols.is_empty() {
        if let Some(f) = file {
            anyhow::bail!("Symbol '{}' not found in {}", symbol_name, f.display());
        } else {
            anyhow::bail!("Symbol '{}' not found in project", symbol_name);
        }
    }

    let target = &symbols[0];

    // Trace the symbol
    let options = TraceOptions::new().with_max_depth(depth);
    let result = graph
        .trace(target.id, options)
        .context("Failed to trace symbol")?;

    // Output results
    if json {
        let output = serde_json::to_string_pretty(&result)?;
        println!("{}", output);
    } else {
        println!("Trace for: {} ({})", result.root.name, result.root.kind);
        if let Some(f) = file {
            println!("  File: {}:{}-{}", f.display(), result.root.range.start_line, result.root.range.end_line);
        } else {
            println!("  Lines: {}-{}", result.root.range.start_line, result.root.range.end_line);
        }

        // Show code snippet if requested
        if show_code {
            if let Some(f) = file {
                if let Ok(source) = std::fs::read_to_string(f) {
                    let lines: Vec<&str> = source.lines().collect();
                    let start = (result.root.range.start_line as usize).saturating_sub(1);
                    let end = (result.root.range.end_line as usize).min(lines.len());
                    println!();
                    println!("```");
                    for (i, line) in lines[start..end].iter().enumerate() {
                        println!("{:4}│ {}", start + i + 1, line);
                    }
                    println!("```");
                }
            }
        }
        println!();

        if !result.callers.is_empty() {
            println!("Callers ({}):", result.callers.len());
            for node in &result.callers {
                let indent = "  ".repeat(node.depth);
                println!("{}← {} ({})", indent, node.symbol.name, node.symbol.kind);
            }
            println!();
        }

        if !result.callees.is_empty() {
            println!("Callees ({}):", result.callees.len());
            for node in &result.callees {
                let indent = "  ".repeat(node.depth);
                println!("{}→ {} ({})", indent, node.symbol.name, node.symbol.kind);
            }
            println!();
        }

        println!(
            "Total: {} symbols, max depth: {}",
            result.total_count(),
            result.max_depth_reached
        );
    }

    Ok(())
}

/// Find the symbol containing a given byte offset.
fn find_containing_symbol(graph: &CodeGraph, file_id: FileId, offset: u32) -> Option<Arc<Symbol>> {
    let symbols = graph.symbols_in_file(file_id);
    symbols
        .into_iter()
        .filter(|s| s.span.start <= offset && offset <= s.span.end)
        .min_by_key(|s| s.span.end - s.span.start) // Smallest containing span
}
