//! Trace command - trace call chains for a symbol.

use anyhow::{Context, Result};
use mimir_graph::{CodeGraph, TraceOptions, TraceRelationship};
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_protocol::{TraceOptions as IpcTraceOptions, TraceRelation};
use mimir_resolve::{ProjectFile, link};
use mimir_types::{FileId, IdGenerator, Language, Symbol};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::app::{Config, find_project_root};
use crate::commands::sync::{load_graph, sync_project};
use crate::daemon::{DaemonClient, check_daemon_running};

/// Which implementation a trait call reaches is decided at runtime, so these are possibilities.
const VIA_TRAIT: &str = " [via trait, possible]";
const IMPLEMENTATION: &str = " [implementation, possible]";
const AS_VALUE: &str = " [as value, possible]";

/// What a trace shows.
struct TraceView {
    depth: usize,
    json: bool,
    show_code: bool,
    callers_only: bool,
    callees_only: bool,
}

/// Execute the trace command.
pub async fn execute(
    file: Option<&Path>,
    symbol_name: &str,
    depth: usize,
    json: bool,
    show_code: bool,
    callers_only: bool,
    callees_only: bool,
) -> Result<()> {
    let in_project = find_project_root().is_some_and(|root| Config::new(&root).is_initialized());
    match file {
        // Callers live in other files, so a file in a project is traced through the project.
        Some(f) if in_project => {
            execute_from_graph(
                Some(f),
                symbol_name,
                depth,
                json,
                show_code,
                callers_only,
                callees_only,
            )
            .await
        }
        Some(f) => {
            execute_single_file(
                f,
                symbol_name,
                depth,
                json,
                show_code,
                callers_only,
                callees_only,
            )
            .await
        }
        None => {
            execute_from_graph(
                None,
                symbol_name,
                depth,
                json,
                show_code,
                callers_only,
                callees_only,
            )
            .await
        }
    }
}

/// Execute trace on a single file.
async fn execute_single_file(
    file: &Path,
    symbol_name: &str,
    depth: usize,
    json: bool,
    show_code: bool,
    callers_only: bool,
    callees_only: bool,
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

    // Resolve the references within this file
    let project_file = ProjectFile {
        file_id,
        path: file.to_path_buf(),
        references: adapter.extract_references(&parsed, file_id),
        source: source.as_str().into(),
    };
    link(&graph, std::slice::from_ref(&project_file), &[]);

    // Find and trace the symbol
    let view = TraceView {
        depth,
        json,
        show_code,
        callers_only,
        callees_only,
    };
    trace_symbol(&graph, symbol_name, view, Some((file, file_id)))
}

/// Execute trace from the synced graph.
async fn execute_from_graph(
    file: Option<&Path>,
    symbol_name: &str,
    depth: usize,
    json: bool,
    show_code: bool,
    callers_only: bool,
    callees_only: bool,
) -> Result<()> {
    // Find project root
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    // Check if daemon is running - use it for automatic updates
    if file.is_none() && check_daemon_running(&config.agent_dir) {
        return execute_via_daemon(
            &config,
            symbol_name,
            depth,
            json,
            show_code,
            callers_only,
            callees_only,
        )
        .await;
    }

    // Fallback: sync and query locally
    let (graph, file_ids) = if config.sync.sync_on_query {
        // Sync before query to ensure graph is up-to-date
        let (graph, _ids, file_ids, _stats) = sync_project(&config, false).await?;
        (graph, file_ids)
    } else {
        // Load from disk
        let (graph, _ids, file_ids) = load_graph(&config)
            .await?
            .context("Graph not found. Run 'mimir sync' first.")?;
        (graph, file_ids)
    };

    let scope = file
        .map(|f| project_file_id(&config.root, f, &file_ids).map(|id| (f, id)))
        .transpose()?;

    // Find and trace the symbol
    let view = TraceView {
        depth,
        json,
        show_code,
        callers_only,
        callees_only,
    };
    trace_symbol(&graph, symbol_name, view, scope)
}

/// The graph's id for a file given as a path.
fn project_file_id(root: &Path, file: &Path, file_ids: &HashMap<String, FileId>) -> Result<FileId> {
    let absolute = file
        .canonicalize()
        .with_context(|| format!("File not found: {}", file.display()))?;
    let relative = absolute
        .strip_prefix(root.canonicalize()?)
        .unwrap_or(&absolute);
    file_ids
        .get(&*relative.to_string_lossy())
        .copied()
        .with_context(|| format!("{} is not indexed in this project", file.display()))
}

/// Execute trace via the daemon.
async fn execute_via_daemon(
    config: &Config,
    symbol_name: &str,
    depth: usize,
    json: bool,
    show_code: bool,
    callers_only: bool,
    callees_only: bool,
) -> Result<()> {
    let mut client = DaemonClient::connect(&config.agent_dir).await?;

    let options = IpcTraceOptions {
        max_depth: Some(depth as u32),
        callers_only,
        callees_only,
        cross_language: false,
    };

    let result = client.trace_by_name(symbol_name, options).await?;

    // Output results
    if json {
        let output = serde_json::to_string_pretty(&result)?;
        println!("{}", output);
    } else {
        println!("Trace for: {} ({:?})", result.root.name, result.root.kind);
        println!(
            "  Lines: {}-{}",
            result.root.start_line, result.root.end_line
        );
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

        if !callees_only && !result.callers.is_empty() {
            println!("Callers ({}):", result.callers.len());
            for node in &result.callers {
                let indent = "  ".repeat(node.depth as usize);
                let via = match node.relationship {
                    TraceRelation::CallerViaTrait => VIA_TRAIT,
                    TraceRelation::CallerByValue => AS_VALUE,
                    _ => "",
                };
                println!(
                    "{}← {} ({:?}){}",
                    indent, node.symbol.name, node.symbol.kind, via
                );
            }
            println!();
        }

        if !callers_only && !result.callees.is_empty() {
            println!("Callees ({}):", result.callees.len());
            for node in &result.callees {
                let indent = "  ".repeat(node.depth as usize);
                if node.relationship == TraceRelation::Implementation {
                    let name = node
                        .symbol
                        .qualified_name
                        .as_deref()
                        .unwrap_or(&node.symbol.name);
                    println!(
                        "{}→ {} ({:?}){}",
                        indent, name, node.symbol.kind, IMPLEMENTATION
                    );
                } else if node.relationship == TraceRelation::CalleeByValue {
                    println!(
                        "{}→ {} ({:?}){}",
                        indent, node.symbol.name, node.symbol.kind, AS_VALUE
                    );
                } else {
                    println!("{}→ {} ({:?})", indent, node.symbol.name, node.symbol.kind);
                }
            }
            println!();
        }

        println!(
            "Total: {} symbols, max depth: {}",
            result.total_symbols, result.max_depth_reached
        );
        if result.provisional {
            println!(
                "Partial: rust-analyzer is still working, so some calls may be missing. Run this again shortly."
            );
        }
    }

    Ok(())
}

/// Common trace logic.
fn trace_symbol(
    graph: &CodeGraph,
    symbol_name: &str,
    view: TraceView,
    scope: Option<(&Path, FileId)>,
) -> Result<()> {
    let TraceView {
        depth,
        json,
        show_code,
        callers_only,
        callees_only,
    } = view;
    let file = scope.map(|(path, _)| path);

    // Find the target symbol
    let mut symbols = graph.find_symbol(symbol_name);
    if let Some((_, file_id)) = scope {
        symbols.retain(|s| s.file_id == file_id);
    }
    if symbols.is_empty() {
        if let Some(f) = file {
            anyhow::bail!("Symbol '{}' not found in {}", symbol_name, f.display());
        } else {
            anyhow::bail!("Symbol '{}' not found in project", symbol_name);
        }
    }

    if symbols.len() > 1 {
        eprintln!(
            "note: {} symbols are named '{}'; tracing the first. Narrow it with -f <file>.",
            symbols.len(),
            symbol_name
        );
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
            println!(
                "  File: {}:{}-{}",
                f.display(),
                result.root.range.start_line,
                result.root.range.end_line
            );
        } else {
            println!(
                "  Lines: {}-{}",
                result.root.range.start_line, result.root.range.end_line
            );
        }

        // Show code snippet if requested
        if show_code
            && let Some(f) = file
            && let Ok(source) = std::fs::read_to_string(f)
        {
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
        println!();

        if !callees_only && !result.callers.is_empty() {
            println!("Callers ({}):", result.callers.len());
            for node in &result.callers {
                let indent = "  ".repeat(node.depth);
                let via = match node.relationship {
                    TraceRelationship::CallerViaTrait => VIA_TRAIT,
                    TraceRelationship::CallerByValue => AS_VALUE,
                    _ => "",
                };
                println!(
                    "{}← {} ({}){}",
                    indent, node.symbol.name, node.symbol.kind, via
                );
            }
            println!();
        }

        if !callers_only && !result.callees.is_empty() {
            println!("Callees ({}):", result.callees.len());
            for node in &result.callees {
                let indent = "  ".repeat(node.depth);
                if node.relationship == TraceRelationship::Implementation {
                    println!(
                        "{}→ {} ({}){}",
                        indent, node.symbol.qualified_name, node.symbol.kind, IMPLEMENTATION
                    );
                } else if node.relationship == TraceRelationship::CalleeByValue {
                    println!(
                        "{}→ {} ({}){}",
                        indent, node.symbol.name, node.symbol.kind, AS_VALUE
                    );
                } else {
                    println!("{}→ {} ({})", indent, node.symbol.name, node.symbol.kind);
                }
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
