//! Sync command - build the code graph from project files.

use anyhow::{Context, Result};
use mimir_files::Scanner;
use mimir_graph::CodeGraph;
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_relations::{EdgeData, EdgeKind};
use mimir_resolve::{Analyzers, ProjectFile, Refresh, link, load_or_generate};
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

/// Sync result statistics.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SyncStats {
    /// Number of files processed.
    pub files_count: usize,
    /// Number of symbols indexed.
    pub symbols_count: usize,
    /// Number of references indexed.
    pub references_count: usize,
}

/// Sync the project and return the graph.
/// This is the core sync function that can be called by other commands.
pub async fn sync_project(
    config: &Config,
    verbose: bool,
) -> Result<(CodeGraph, IdGenerator, HashMap<String, FileId>, SyncStats)> {
    sync_project_with(config, verbose, Refresh::Edits).await
}

async fn sync_project_with(
    config: &Config,
    verbose: bool,
    refresh: Refresh,
) -> Result<(CodeGraph, IdGenerator, HashMap<String, FileId>, SyncStats)> {
    // Ensure graph directory exists
    tokio::fs::create_dir_all(&config.graph_dir).await?;

    if verbose {
        println!("Syncing project: {}", config.root.display());
    }

    // Setup adapter registry
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));

    // Create ID generator and graph
    let ids = IdGenerator::new();
    let graph = CodeGraph::new();

    // Track file IDs
    let mut file_ids: HashMap<String, FileId> = HashMap::new();

    // Use mimir-files Scanner for proper gitignore support
    let scanner = Scanner::new(&config.root);
    let scanned_files = scanner.scan().await?;

    // Filter to only supported languages (Rust for now)
    let rust_files: Vec<_> = scanned_files
        .iter()
        .filter(|f| f.language == Language::Rust)
        .collect();

    if verbose {
        println!(
            "Found {} Rust files (of {} total code files)",
            rust_files.len(),
            scanned_files.len()
        );
    }

    let mut total_symbols = 0;
    let mut project_files = Vec::with_capacity(rust_files.len());

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
                if verbose {
                    eprintln!("  Warning: Could not read {}: {}", rel_path, e);
                }
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
                if verbose {
                    eprintln!("  Warning: Could not parse {}", rel_path);
                }
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

        // Calls can point at any file, so they are resolved once all symbols are known
        project_files.push(ProjectFile {
            file_id,
            path: scanned.path.clone(),
            references: adapter.extract_references(&parsed, file_id),
            source: source.into(),
        });
    }

    let indexes = {
        let (root, cache_dir, files) = (
            config.root.clone(),
            config.graph_dir.clone(),
            project_files.clone(),
        );
        // Here the analyzer lives as long as this sync, which still saves most of a new index.
        tokio::task::spawn_blocking(move || {
            load_or_generate(
                &root,
                &cache_dir,
                &files,
                &mut Analyzers::default(),
                refresh,
            )
        })
        .await?
    };
    let linked = link(&graph, &project_files, &indexes);
    let total_refs =
        linked.semantic_calls + linked.syntax_calls + linked.implementations + linked.imports;

    if verbose {
        println!(
            "Indexed {} symbols, {} references",
            total_symbols, total_refs
        );
        println!(
            "Calls: {} semantic ({} files), {} from syntax only ({} files)",
            linked.semantic_calls, linked.semantic_files, linked.syntax_calls, linked.syntax_files
        );
        if linked.live_files > 0 {
            println!(
                "Edited since the index: {} files, from a running analyzer (`mimir sync --full` rewrites the index)",
                linked.live_files
            );
        }
        println!("Trait method implementations: {}", linked.implementations);
        println!("Functions used as values: {}", linked.function_values);
    }

    // Persist the graph
    persist_graph(&graph, &ids, &file_ids, &config.graph_dir).await?;

    if verbose {
        println!("Graph saved to {}", config.graph_dir.display());
    }

    let stats = SyncStats {
        files_count: rust_files.len(),
        symbols_count: total_symbols,
        references_count: total_refs,
    };

    Ok((graph, ids, file_ids, stats))
}

/// Execute the sync command.
pub async fn execute(full: bool) -> Result<()> {
    // Find project root
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    let (_graph, _ids, _file_ids, _stats) = sync_project_with(
        &config,
        true,
        if full { Refresh::Full } else { Refresh::Edits },
    )
    .await?;

    Ok(())
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
    use mimir_types::{Confidence, Span};
    let edges = persisted
        .edges
        .into_iter()
        .map(
            |(from_id, to_id, kind_str, span_start, span_end, confidence_str)| {
                let kind = match kind_str.as_str() {
                    "Imports" => EdgeKind::Imports,
                    "Implements" => EdgeKind::Implements,
                    "UsesAsValue" => EdgeKind::UsesAsValue,
                    _ => EdgeKind::Calls,
                };
                let confidence = match confidence_str.as_str() {
                    "Certain" => Confidence::Certain,
                    "High" => Confidence::High,
                    "Medium" => Confidence::Medium,
                    "Low" => Confidence::Low,
                    _ => Confidence::Medium,
                };
                (
                    mimir_types::SymbolId::from_u64(from_id),
                    mimir_types::SymbolId::from_u64(to_id),
                    EdgeData::new(kind, Span::new(span_start, span_end), confidence),
                )
            },
        )
        .collect();
    graph.replace_edges(edges);

    Ok(Some((graph, ids, file_ids)))
}
