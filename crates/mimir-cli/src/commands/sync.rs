//! Sync command - build the code graph from project files.

use anyhow::{Context, Result};
use mimir_files::Scanner;
use mimir_graph::CodeGraph;
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_types::{FileId, IdGenerator, Language, Symbol};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::app::{Config, find_project_root};

/// Persisted graph data.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedGraph {
    /// All symbols.
    symbols: Vec<Symbol>,
    /// Edges: (from_id, to_id, edge_kind, span_start, span_end, confidence)
    edges: Vec<(u64, u64, String, u32, u32, String)>,
    /// File path to FileId mapping.
    file_ids: HashMap<String, u64>,
    /// Next ID for the generator.
    next_id: u64,
}

/// Execute the sync command.
pub async fn execute() -> Result<()> {
    // Find project root
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    // Ensure graph directory exists
    tokio::fs::create_dir_all(&config.graph_dir).await?;

    println!("Syncing project: {}", root.display());

    // Setup adapter registry
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));

    // Create ID generator and graph
    let ids = IdGenerator::new();
    let graph = CodeGraph::new();

    // Track file IDs
    let mut file_ids: HashMap<String, FileId> = HashMap::new();

    // Use mimir-files Scanner for proper gitignore support
    let scanner = Scanner::new(&root);
    let scanned_files = scanner.scan().await?;

    // Filter to only supported languages (Rust for now)
    let rust_files: Vec<_> = scanned_files
        .iter()
        .filter(|f| f.language == Language::Rust)
        .collect();

    println!(
        "Found {} Rust files (of {} total code files)",
        rust_files.len(),
        scanned_files.len()
    );

    let mut total_symbols = 0;
    let mut total_refs = 0;

    for scanned in &rust_files {
        let rel_path = scanned.path.to_string_lossy().to_string();

        // Get or create FileId
        let file_id: FileId = *file_ids
            .entry(rel_path.clone())
            .or_insert_with(|| ids.next());

        // Read and parse
        let source = match std::fs::read_to_string(&scanned.absolute_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  Warning: Could not read {}: {}", rel_path, e);
                continue;
            }
        };

        let adapter = match registry.get_by_language(Language::Rust) {
            Some(a) => a,
            None => continue,
        };

        let parsed = match adapter.parse(&source) {
            Some(p) => p,
            None => {
                eprintln!("  Warning: Could not parse {}", rel_path);
                continue;
            }
        };

        // Extract symbols
        let extracted = adapter.extract_symbols(&parsed, file_id);
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
            total_symbols += 1;
        }

        // Extract references
        let references = adapter.extract_references(&parsed, file_id);
        for reference in &references {
            // Find source symbol (containing function)
            if let Some(from_symbol) = find_containing_symbol(&graph, file_id, reference.span.start)
            {
                // Find target symbol by name
                let targets = graph.find_symbol(&reference.name);
                if let Some(to_symbol) = targets.first() {
                    graph.add_call(
                        from_symbol.id,
                        to_symbol.id,
                        reference.span,
                        reference.confidence,
                    );
                    total_refs += 1;
                }
            }
        }
    }

    println!(
        "Indexed {} symbols, {} references",
        total_symbols, total_refs
    );

    // Persist the graph
    persist_graph(&graph, &ids, &file_ids, &config.graph_dir).await?;

    println!("Graph saved to {}", config.graph_dir.display());

    Ok(())
}

/// Find the symbol containing a given byte offset.
fn find_containing_symbol(graph: &CodeGraph, file_id: FileId, offset: u32) -> Option<Arc<Symbol>> {
    let symbols = graph.symbols_in_file(file_id);
    symbols
        .into_iter()
        .filter(|s| s.span.start <= offset && offset <= s.span.end)
        .min_by_key(|s| s.span.end - s.span.start)
}

/// Persist the graph to disk.
async fn persist_graph(
    graph: &CodeGraph,
    ids: &IdGenerator,
    file_ids: &HashMap<String, FileId>,
    graph_dir: &Path,
) -> Result<()> {
    // Collect all symbols
    let mut symbols = Vec::new();
    for file_id in file_ids.values() {
        for sym in graph.symbols_in_file(*file_id) {
            symbols.push((*sym).clone());
        }
    }

    // Collect edges
    let mut edges = Vec::new();
    for sym in &symbols {
        for (target, edge) in graph.get_outgoing(sym.id) {
            edges.push((
                sym.id.as_u64(),
                target.id.as_u64(),
                format!("{:?}", edge.kind),
                edge.span.start,
                edge.span.end,
                format!("{:?}", edge.confidence),
            ));
        }
    }

    // Convert file_ids
    let file_ids_serializable: HashMap<String, u64> = file_ids
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64()))
        .collect();

    let persisted = PersistedGraph {
        symbols,
        edges,
        file_ids: file_ids_serializable,
        next_id: ids.current(),
    };

    // Save to JSON
    let json = serde_json::to_string_pretty(&persisted)?;
    let graph_file = graph_dir.join("graph.json");
    tokio::fs::write(&graph_file, json).await?;

    Ok(())
}

/// Load the graph from disk.
pub async fn load_graph(
    config: &Config,
) -> Result<Option<(CodeGraph, IdGenerator, HashMap<String, FileId>)>> {
    let graph_file = config.graph_dir.join("graph.json");

    if !graph_file.exists() {
        return Ok(None);
    }

    let json = tokio::fs::read_to_string(&graph_file).await?;
    let persisted: PersistedGraph = serde_json::from_str(&json)?;

    // Rebuild ID generator
    let ids = IdGenerator::starting_from(persisted.next_id);

    // Rebuild file_ids
    let file_ids: HashMap<String, FileId> = persisted
        .file_ids
        .into_iter()
        .map(|(k, v)| (k, FileId::from_u64(v)))
        .collect();

    // Rebuild graph
    let graph = CodeGraph::new();

    // Add symbols
    for sym in persisted.symbols {
        graph.add_symbol(sym);
    }

    // Add edges
    for (from_id, to_id, kind_str, span_start, span_end, confidence_str) in persisted.edges {
        use mimir_types::{Confidence, Span};

        let from = mimir_types::SymbolId::from_u64(from_id);
        let to = mimir_types::SymbolId::from_u64(to_id);
        let span = Span::new(span_start, span_end);

        let confidence = match confidence_str.as_str() {
            "Certain" => Confidence::Certain,
            "High" => Confidence::High,
            "Medium" => Confidence::Medium,
            "Low" => Confidence::Low,
            _ => Confidence::Medium,
        };

        // For now, treat all as calls (kind_str reserved for future edge types)
        let _ = kind_str;
        graph.add_call(from, to, span, confidence);
    }

    Ok(Some((graph, ids, file_ids)))
}
